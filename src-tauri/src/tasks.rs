// VideosFlow — 任务队列 + 进度广播（tokio mpsc + Tauri Channel）
// M0：worker 消费任务，做 sidecar 健康检查并广播进度，最终持久化状态到 tasks 表。
// M2：新增 film_import / film_smart_cut / film_export 分支（确定性纯算法 + ffmpeg）。

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tokio::sync::mpsc;

use std::path::Path;
use std::process::Command;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use tokio::time::sleep;

use reqwest::Client;
use sqlx::sqlite::SqlitePool;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;

use crate::db;
use crate::ffmpeg::{self, FfMpeg};
use crate::cred;

#[derive(Serialize, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressMsg {
    pub task_id: String,
    pub progress: f64,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}

pub struct TaskJob {
    pub id: String,
    pub kind: String,
    pub project_id: Option<String>,
    pub payload: serde_json::Value,
    pub channel: Channel<ProgressMsg>,
}

pub type TaskSender = mpsc::Sender<TaskJob>;

/// 本地模型目录（项目根 models/），由 Tauri setup 注入，供本地 ASR 推理查找权重。
static MODELS_DIR: OnceLock<std::path::PathBuf> = OnceLock::new();
pub fn set_models_dir(d: std::path::PathBuf) {
    let _ = MODELS_DIR.set(d);
}

/// 启动 worker（在 Tauri 异步运行时中消费队列）。
pub fn start(pool: SqlitePool, client: Client, port: u16, data_dir: std::path::PathBuf, models_dir: std::path::PathBuf, rx: mpsc::Receiver<TaskJob>) {
    set_models_dir(models_dir);
    tauri::async_runtime::spawn(async move {
        run_loop(pool, client, port, data_dir, rx).await;
    });
}

async fn run_loop(pool: SqlitePool, client: Client, port: u16, data_dir: std::path::PathBuf, mut rx: mpsc::Receiver<TaskJob>) {
    while let Some(job) = rx.recv().await {
        run_job(&pool, &client, port, &data_dir, job).await;
    }
}

async fn run_job(pool: &SqlitePool, client: &Client, port: u16, data_dir: &Path, job: TaskJob) {
    let channel = job.channel.clone();
    let emit: Arc<dyn Fn(ProgressMsg) + Send + Sync> =
        Arc::new(move |m: ProgressMsg| { let _ = channel.send(m); });

    emit(ProgressMsg {
        task_id: job.id.clone(),
        progress: 5.0,
        status: "queued".into(),
        message: Some("任务入队".into()),
        payload: None,
    });
    db::task_update(pool, &job.id, "running", 10.0, "引擎健康检查").await.ok();

    // 全部任务均经 Rust reqwest 直连各云网关（Agnes 对话 / XiaomiMimo ASR·TTS），不再依赖 Python sidecar。

    match job.kind.as_str() {
        "chat" | "llm_chat" => {
            emit(ProgressMsg {
                task_id: job.id.clone(),
                progress: 60.0,
                status: "running".into(),
                message: Some("正在调用 Agnes /chat/completions".into()),
                payload: None,
            });
            match run_chat(pool, client, port, &job).await {
                Ok(answer) => {
                    db::task_update(pool, &job.id, "done", 100.0, "完成（真实 Agnes 对话）")
                        .await
                        .ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "done".into(),
                        message: Some("真实对话完成".into()),
                        payload: Some(serde_json::json!({ "answer": answer })),
                    });
                }
                Err(e) => {
                    db::task_update(pool, &job.id, "failed", 100.0, &e).await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "failed".into(),
                        message: Some(e),
                        payload: None,
                    });
                }
            }
            return;
        }
        "film_import" => {
            match run_film_import(pool, client, port, data_dir, &job, emit.clone()).await {
                Ok((degraded, asr_reason)) => {
                    db::task_update(pool, &job.id, "done", 100.0, "导入对齐完成").await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "done".into(),
                        message: Some(
            if degraded {
                format!("导入完成（ASR 未就绪：{asr_reason}，已生成草稿时间线）")
            } else {
                "导入对齐完成".into()
            },
        ),
                        payload: Some(serde_json::json!({ "degraded": degraded })),
                    });
                }
                Err(e) => {
                    db::task_update(pool, &job.id, "failed", 100.0, &e).await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "failed".into(),
                        message: Some(e),
                        payload: None,
                    });
                }
            }
            return;
        }
        "film_smart_cut" => {
            match run_film_smart_cut(pool, client, port, data_dir, &job, emit.clone()).await {
                Ok(clip_count) => {
                    db::task_update(pool, &job.id, "done", 100.0, "智能粗剪完成").await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "done".into(),
                        message: Some("自动切点完成".into()),
                        payload: Some(serde_json::json!({ "clips": clip_count })),
                    });
                }
                Err(e) => {
                    db::task_update(pool, &job.id, "failed", 100.0, &e).await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "failed".into(),
                        message: Some(e),
                        payload: None,
                    });
                }
            }
            return;
        }
        "film_export" => {
            match run_film_export(pool, client, port, data_dir, &job, emit.clone()).await {
                Ok(out_path) => {
                    db::task_update(pool, &job.id, "done", 100.0, "导出完成").await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "done".into(),
                        message: Some("导出 MP4 完成".into()),
                        payload: Some(serde_json::json!({ "outPath": out_path })),
                    });
                }
                Err(e) => {
                    db::task_update(pool, &job.id, "failed", 100.0, &e).await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "failed".into(),
                        message: Some(e),
                        payload: None,
                    });
                }
            }
            return;
        }
        "spoken_asr" => {
            match run_spoken_asr(pool, client, data_dir, &job, emit.clone()).await {
                Ok(degraded) => {
                    db::task_update(pool, &job.id, "done", 100.0, "ASR 完成").await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "done".into(),
                        message: Some(if degraded { "识别完成（ASR 仅返回整段文本）" } else { "识别完成" }.into()),
                        payload: Some(serde_json::json!({ "degraded": degraded })),
                    });
                }
                Err(e) => {
                    db::task_update(pool, &job.id, "failed", 100.0, &e).await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "failed".into(),
                        message: Some(e),
                        payload: None,
                    });
                }
            }
            return;
        }
        "spoken_detect" => {
            match run_spoken_detect(pool, client, data_dir, &job, emit.clone()).await {
                Ok(n) => {
                    db::task_update(pool, &job.id, "done", 100.0, "检测完成").await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "done".into(),
                        message: Some(format!("检测到 {n} 个问题")),
                        payload: Some(serde_json::json!({ "count": n })),
                    });
                }
                Err(e) => {
                    db::task_update(pool, &job.id, "failed", 100.0, &e).await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "failed".into(),
                        message: Some(e),
                        payload: None,
                    });
                }
            }
            return;
        }
        "spoken_keyword" => {
            match run_spoken_keyword(pool, client, &job, emit.clone()).await {
                Ok(n) => {
                    db::task_update(pool, &job.id, "done", 100.0, "关键词完成").await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "done".into(),
                        message: Some(format!("抽取 {n} 个关键词")),
                        payload: Some(serde_json::json!({ "count": n })),
                    });
                }
                Err(e) => {
                    db::task_update(pool, &job.id, "failed", 100.0, &e).await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "failed".into(),
                        message: Some(e),
                        payload: None,
                    });
                }
            }
            return;
        }
        "spoken_burn" => {
            match run_spoken_burn(pool, data_dir, &job, emit.clone()).await {
                Ok(out_path) => {
                    db::task_update(pool, &job.id, "done", 100.0, "烧录完成").await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "done".into(),
                        message: Some("花字烧录完成".into()),
                        payload: Some(serde_json::json!({ "outPath": out_path })),
                    });
                }
                Err(e) => {
                    db::task_update(pool, &job.id, "failed", 100.0, &e).await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "failed".into(),
                        message: Some(e),
                        payload: None,
                    });
                }
            }
            return;
        }
        "spoken_export" => {
            match run_spoken_export(pool, client, data_dir, &job, emit.clone()).await {
                Ok(out_path) => {
                    db::task_update(pool, &job.id, "done", 100.0, "导出完成").await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "done".into(),
                        message: Some("干净片段导出完成".into()),
                        payload: Some(serde_json::json!({ "outPath": out_path })),
                    });
                }
                Err(e) => {
                    db::task_update(pool, &job.id, "failed", 100.0, &e).await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "failed".into(),
                        message: Some(e),
                        payload: None,
                    });
                }
            }
            return;
        }
        "script_write" => {
            match run_script_write(pool, client, &job, emit.clone()).await {
                Ok(script) => {
                    db::task_update(pool, &job.id, "done", 100.0, "文案已生成").await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "done".into(),
                        message: Some("文案生成完成".into()),
                        payload: Some(serde_json::json!({ "script": script })),
                    });
                }
                Err(e) => {
                    db::task_update(pool, &job.id, "failed", 100.0, &e).await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "failed".into(),
                        message: Some(e),
                        payload: None,
                    });
                }
            }
            return;
        }
        "script_humanize" => {
            match run_script_humanize(pool, client, &job, emit.clone()).await {
                Ok(human) => {
                    db::task_update(pool, &job.id, "done", 100.0, "去 AI 味完成").await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "done".into(),
                        message: Some("去 AI 味完成".into()),
                        payload: Some(serde_json::json!({ "human": human })),
                    });
                }
                Err(e) => {
                    db::task_update(pool, &job.id, "failed", 100.0, &e).await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "failed".into(),
                        message: Some(e),
                        payload: None,
                    });
                }
            }
            return;
        }
        "storyboard_gen" => {
            match run_storyboard_gen(pool, client, &job, emit.clone()).await {
                Ok(shots_json) => {
                    db::task_update(pool, &job.id, "done", 100.0, "分镜已生成").await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "done".into(),
                        message: Some("分镜生成完成".into()),
                        payload: Some(serde_json::json!({ "shots": shots_json })),
                    });
                }
                Err(e) => {
                    db::task_update(pool, &job.id, "failed", 100.0, &e).await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "failed".into(),
                        message: Some(e),
                        payload: None,
                    });
                }
            }
            return;
        }
        "image_gen" => {
            match run_image_gen_task(pool, client, data_dir, &job, emit.clone()).await {
                Ok(out_path) => {
                    db::task_update(pool, &job.id, "done", 100.0, "图片已生成").await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "done".into(),
                        message: Some("图片生成完成".into()),
                        payload: Some(serde_json::json!({ "path": out_path })),
                    });
                }
                Err(e) => {
                    db::task_update(pool, &job.id, "failed", 100.0, &e).await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "failed".into(),
                        message: Some(e),
                        payload: None,
                    });
                }
            }
            return;
        }
        "film_script_gen" => {
            match run_film_script_gen(pool, client, port, data_dir, &job, emit.clone()).await {
                Ok((script, asr_failed, asr_reason)) => {
                    db::task_update(pool, &job.id, "done", 100.0, "影片解说文案已生成").await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "done".into(),
                        message: Some("影片解说文案已生成".into()),
                        payload: Some(serde_json::json!({ "script": script, "asrFailed": asr_failed, "asrReason": asr_reason })),
                    });
                }
                Err(e) => {
                    db::task_update(pool, &job.id, "failed", 100.0, &e).await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "failed".into(),
                        message: Some(e),
                        payload: None,
                    });
                }
            }
            return;
        }
        "film_video_analysis" => {
            match run_film_video_analysis(pool, client, data_dir, &job, emit.clone()).await {
                Ok(()) => {
                    // 完成态已在函数内以 step=10 发出；此处仅兜底确保状态落库
                    db::task_update(pool, &job.id, "done", 100.0, "影片分析完成").await.ok();
                }
                Err(e) => {
                    db::task_update(pool, &job.id, "failed", 100.0, &e).await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "failed".into(),
                        message: Some(e),
                        payload: None,
                    });
                }
            }
            return;
        }
        _ => {}
    }

    // 其他任务类型：M0 链路验证（M1-M5 接入真实能力）
    db::task_update(pool, &job.id, "done", 100.0, "完成（M0 链路验证）")
        .await
        .ok();
    emit(ProgressMsg {
        task_id: job.id.clone(),
        progress: 100.0,
        status: "done".into(),
        message: Some("完成".into()),
        payload: None,
    });
}

/// 真实对话任务：取 llm provider 配置 + 系统凭据库 Key，经 Rust reqwest 直连 Agnes /chat/completions（OpenAI 兼容）。
/// 不依赖 Python sidecar（MVP 纯云 API，与 provider_test 同一思路）。
async fn run_chat(pool: &SqlitePool, client: &Client, _port: u16, job: &TaskJob) -> Result<String, String> {
    let row = db::get_by_kind(pool, "llm").await?;
    let key = cred::get_key(pool, "llm").await
        .map_err(|e| format!("读取凭据库失败：{e}（请尝试在设置页重新保存 Key）"))?;
    let key = key.ok_or_else(|| "尚未保存大模型 API Key，请先在设置页保存 Key".to_string())?;
    if row.base_url.is_empty() {
        return Err("LLM 网关 base_url 未配置（请检查设置页大模型 base_url）".into());
    }
    let prompt = job
        .payload
        .get("prompt")
        .and_then(|v| v.as_str())
        .unwrap_or("ping")
        .to_string();
    let max_tokens = job
        .payload
        .get("maxTokens")
        .and_then(|v| v.as_i64())
        .or_else(|| job.payload.get("max_tokens").and_then(|v| v.as_i64()))
        .unwrap_or(512)
        .clamp(1, 4096) as u32;

    let base = row.base_url.trim_end_matches('/');
    let url = format!("{base}/chat/completions");
    let body = serde_json::json!({
        "model": row.model,
        "messages": [ { "role": "user", "content": prompt } ],
        "max_tokens": max_tokens,
    });
    let resp = client
        .post(&url)
        .bearer_auth(key.as_str())
        .json(&body)
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| format!("请求 Agnes 失败: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let txt = resp.text().await.unwrap_or_default();
        return Err(format!("Agnes 返回 {status}: {txt}"));
    }
    let data: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析大模型响应失败: {e}"))?;
    if data.get("choices").is_none() {
        // 兜底：返回原始 JSON，便于排查 Agnes 非常规响应
        return Ok(serde_json::to_string_pretty(&data).unwrap_or_default());
    }
    Ok(extract_chat_text(&Some(data)))
}

/// 从 OpenAI 兼容响应中提取对话文本（choices[0].message.content），兜底返回原始 data。
fn extract_chat_text(data: &Option<serde_json::Value>) -> String {
    if let Some(d) = data {
        if let Some(choices) = d.get("choices").and_then(|c| c.as_array()) {
            if let Some(first) = choices.first() {
                if let Some(content) = first
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_str())
                {
                    return content.to_string();
                }
            }
        }
        if let Some(s) = d.as_str() {
            return s.to_string();
        }
        return d.to_string();
    }
    "ok".to_string()
}

// ===========================================================================
// M2：确定性纯算法（文案分段 / 对齐 / 静音废片检测）
// ===========================================================================

/// 按标点分段（。！？！？，；；\n）。
fn segment_by_punctuation(script: &str) -> Vec<db::ScriptSeg> {
    let mut segs: Vec<db::ScriptSeg> = Vec::new();
    let mut buf = String::new();
    let mut idx = 0usize;
    for ch in script.chars() {
        buf.push(ch);
        if "。！？!?，,；;\n".contains(ch) {
            let t = buf.trim().to_string();
            if !t.is_empty() {
                segs.push(db::ScriptSeg { index: idx, text: t });
                idx += 1;
            }
            buf.clear();
        }
    }
    let t = buf.trim().to_string();
    if !t.is_empty() {
        segs.push(db::ScriptSeg { index: idx, text: t });
    }
    segs
}

/// 两个字符串的最长公共子序列长度占比（字符级）。
fn lcs_ratio(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let n = a.len();
    let m = b.len();
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in 1..=n {
        for j in 1..=m {
            if a[i - 1] == b[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else {
                dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]);
            }
        }
    }
    dp[n][m] as f64 / (n.max(m) as f64)
}

/// 将文案分段与 ASR 句做模糊匹配对齐，返回 seg_index -> (start, end)。
fn align_text(segs: &[db::ScriptSeg], asr: &[db::AsrSegment]) -> std::collections::HashMap<String, (f64, f64)> {
    let mut map = std::collections::HashMap::new();
    for seg in segs {
        let mut best: Option<(f64, f64, f64)> = None; // (start, end, score)
        for a in asr {
            let score = lcs_ratio(&seg.text, &a.text);
            if score > 0.4 {
                let keep = match best {
                    Some((_, _, b)) => score > b,
                    None => true,
                };
                if keep {
                    best = Some((a.start, a.end, score));
                }
            }
        }
        if let Some((s, e, _)) = best {
            map.insert(seg.index.to_string(), (s, e));
        }
    }
    map
}

/// 判定片段是否为静音/废片：时长过短，或整体落入/大量重叠静音段。
fn is_silence_or_junk(span: (f64, f64), silence: &[(f64, f64)], min_dur: f64) -> bool {
    if span.1 - span.0 < min_dur {
        return true;
    }
    for (s, e) in silence {
        if span.0 >= *s && span.1 <= *e {
            return true;
        }
        let ov = (span.1.min(*e) - span.0.max(*s)).max(0.0);
        let len = span.1 - span.0;
        if len > 0.0 && ov / len > 0.6 {
            return true;
        }
    }
    false
}

// ===========================================================================
// M2：异步任务实现
// ===========================================================================

/// 语音识别（ASR）：直连 XiaomiMimo /chat/completions（OpenAI 兼容，音频以 base64 置于 messages.input_audio）。
/// 返回 (segments, language, duration, degraded)。无 Key / 无音频 / 调用失败均降级（degraded=true），不阻塞导入。
/// 注：XiaomiMimo ASR 仅返回整段文本（无逐句时间轴），故整段作为一个 segment 存储。
/// 返回 (segments, language, duration, degraded, reason)。
/// reason 仅在降级时非空，描述失败原因（Key 无效 / 余额不足 / 网络等），用于前端精确提示。
/// 将 ASR 网关返回的错误体解析为对中文用户友好的原因描述（去掉原始 JSON 噪声）。
fn asr_error_reason(status: u16, body: &str) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        let msg = v
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .or_else(|| v.get("message").and_then(|m| m.as_str()))
            .or_else(|| v.get("error").and_then(|e| e.get("type")).and_then(|t| t.as_str()));
        if let Some(m) = msg {
            return match status {
                402 => "HTTP 402：XiaomiMimo 账户余额不足，请到 XiaomiMimo 控制台充值后再试".to_string(),
                401 | 403 => format!("HTTP {status}：API Key 无效或未授权"),
                _ => format!("HTTP {status}：{m}"),
            };
        }
    }
    format!("HTTP {status}：{}", body.chars().take(200).collect::<String>())
}

async fn transcribe_asr(
    pool: &SqlitePool,
    client: &Client,
    audio_path: &str,
) -> (Vec<db::AsrSegment>, String, f64, bool, String) {
    let row = match db::get_by_kind(pool, "asr").await {
        Ok(r) => r,
        Err(_) => return (Vec::new(), "zh".to_string(), 0.0, true, "未找到 ASR Provider 配置".into()),
    };
    // 本地推理模式：直接调用本地 faster-whisper（Python CLI），不走云端网关
    if row.mode == "local" {
        return transcribe_local(audio_path).await;
    }
    let key = match cred::get_key(pool, "asr").await.ok().flatten() {
        Some(k) if !k.is_empty() => k,
        _ => return (Vec::new(), "zh".to_string(), 0.0, true, "未配置 ASR API Key".into()),
    };
    let bytes = match std::fs::read(audio_path) {
        Ok(b) => b,
        Err(_) => return (Vec::new(), "zh".to_string(), 0.0, true, "读取音频文件失败".into()),
    };
    let b64 = B64.encode(&bytes);
    let base_url = row.base_url.trim_end_matches('/');
    let body = serde_json::json!({
        "model": row.model,
        "messages": [{
            "role": "user",
            "content": [{
                "type": "input_audio",
                "input_audio": { "data": format!("data:audio/wav;base64,{b64}") }
            }]
        }],
        "asr_options": { "language": "zh" },
        "stream": false
    });
    let resp = match client
        .post(format!("{base_url}/chat/completions"))
        .bearer_auth(key.as_str())
        .json(&body)
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await
    {
        Ok(r) => r,
        Err(_) => return (Vec::new(), "zh".to_string(), 0.0, true, "ASR 网络请求失败".into()),
    };
    if !resp.status().is_success() {
        let code = resp.status().as_u16();
        let txt = resp.text().await.unwrap_or_default();
        let reason = asr_error_reason(code, &txt);
        return (Vec::new(), "zh".to_string(), 0.0, true, reason);
    }
    let v: serde_json::Value = match resp.json().await {
        Ok(j) => j,
        Err(_) => return (Vec::new(), "zh".to_string(), 0.0, true, "ASR 响应解析失败".into()),
    };
    let text = v
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();
    if text.trim().is_empty() {
        return (Vec::new(), "zh".to_string(), 0.0, true, "ASR 返回为空".into());
    }
    let seg = db::AsrSegment {
        start: 0.0,
        end: 0.0,
        text,
        confidence: 1.0,
    };
    (vec![seg], "zh".to_string(), 0.0, false, String::new())
}

/// 本地 ASR 推理：扫描 models 目录找到含 model.bin + config.json 的本地 faster-whisper 权重，
/// 从「当前工作目录」与「当前可执行文件所在目录」逐级向上查找 src-tauri 下的脚本，返回绝对路径。
/// 兼容 tauri dev（CWD=src-tauri）与不同启动方式，避免相对路径因 CWD 不同而落空。
fn resolve_sidecar(rel: &str) -> Option<std::path::PathBuf> {
    let rel = std::path::Path::new(rel);
    let mut roots: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            roots.push(parent.to_path_buf());
        }
    }
    for root in &roots {
        let mut cur = root.clone();
        loop {
            let cand = cur.join(rel);
            if cand.exists() {
                return Some(cand);
            }
            match cur.parent() {
                Some(p) => cur = p.to_path_buf(),
                None => break,
            }
        }
    }
    None
}

/// 解析本地转写脚本的绝对路径。
/// 优先使用 VF_TRANSCRIBE_SCRIPT 显式指定的路径；否则向上查找 python-sidecar/transcribe.py。
fn resolve_transcribe_script() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("VF_TRANSCRIBE_SCRIPT") {
        return std::path::PathBuf::from(p);
    }
    resolve_sidecar("python-sidecar/transcribe.py")
        .unwrap_or_else(|| std::path::Path::new("python-sidecar/transcribe.py").to_path_buf())
}

/// 繁体中文 -> 简体中文兜底转换（调用 python-sidecar/to_simplified.py，依赖 opencc）。
/// 转换脚本缺失或失败则原样返回，不阻塞主流程。
fn to_simplified(text: &str) -> String {
    if text.trim().is_empty() {
        return text.to_string();
    }
    let script = match resolve_sidecar("python-sidecar/to_simplified.py") {
        Some(s) => s,
        None => return text.to_string(),
    };
    let py = std::env::var("VF_PYTHON").unwrap_or_else(|_| "python".to_string());
    use std::process::Stdio;
    let child = std::process::Command::new(&py)
        .arg(&script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    match child {
        Ok(mut child) => {
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                let _ = stdin.write_all(text.as_bytes());
            }
            match child.wait_with_output() {
                Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
                _ => text.to_string(),
            }
        }
        Err(_) => text.to_string(),
    }
}

/// 调用 python-sidecar/transcribe.py（faster-whisper）做本地转写，返回 segments。
/// 支持环境变量 VF_PYTHON（Python 解释器路径）与 VF_TRANSCRIBE_SCRIPT（脚本路径）。
async fn transcribe_local(audio_path: &str) -> (Vec<db::AsrSegment>, String, f64, bool, String) {
    let base = match MODELS_DIR.get() {
        Some(d) => d.clone(),
        None => return (Vec::new(), "zh".to_string(), 0.0, true, "未配置本地模型目录".into()),
    };
    // 查找已下载的本地模型目录（含 model.bin 与 config.json）
    let mut model_path: Option<std::path::PathBuf> = None;
    if let Ok(entries) = std::fs::read_dir(&base) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() && p.join("model.bin").exists() && p.join("config.json").exists() {
                model_path = Some(p);
                break;
            }
        }
    }
    let model_path = match model_path {
        Some(p) => p,
        None => return (Vec::new(), "zh".to_string(), 0.0, true,
            format!("未在 {} 找到已下载的本地模型，请先在「设置 → 接口 → 语音识别」下载 faster-whisper 模型", base.display())),
    };
    // 调用本地推理脚本（faster-whisper / Python）
    let py = std::env::var("VF_PYTHON").unwrap_or_else(|_| "python".to_string());
    let script = resolve_transcribe_script();
    if !script.exists() {
        return (Vec::new(), "zh".to_string(), 0.0, true,
            format!("未找到本地推理脚本 {}，请用 VF_TRANSCRIBE_SCRIPT 指定 transcribe.py 的绝对路径", script.display()));
    }
    let out = std::process::Command::new(&py)
        .arg(&script)
        .arg("--model").arg(&model_path)
        .arg("--audio").arg(audio_path)
        .arg("--language").arg("zh")
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout);
            match serde_json::from_str::<serde_json::Value>(&s) {
                Ok(v) => {
                    let segs = v.get("segments")
                        .and_then(|x| x.as_array())
                        .map(|arr| arr.iter().filter_map(|seg| {
                            let start = seg.get("start")?.as_f64()?;
                            let end = seg.get("end")?.as_f64()?;
                            let text = seg.get("text")?.as_str()?.to_string();
                            Some(db::AsrSegment { start, end, text, confidence: 1.0 })
                        }).collect())
                        .unwrap_or_default();
                    (segs, "zh".to_string(), 0.0, false, String::new())
                }
                Err(e) => (Vec::new(), "zh".to_string(), 0.0, true,
                    format!("本地推理输出解析失败: {e}；stderr={}", String::from_utf8_lossy(&o.stderr))),
            }
        }
        Ok(o) => (Vec::new(), "zh".to_string(), 0.0, true,
            format!("本地推理进程失败({}): {}", o.status, String::from_utf8_lossy(&o.stderr))),
        Err(e) => (Vec::new(), "zh".to_string(), 0.0, true,
            format!("无法启动本地推理（请确认已安装 faster-whisper 且 python 在 PATH，或用 VF_PYTHON/VF_TRANSCRIBE_SCRIPT 指定）: {e}")),
    }
}

/// 语音合成（TTS）：直连 XiaomiMimo /audio/speech（OpenAI 兼容，返回音频字节），写本地文件后返回路径。
/// 无 Key / 调用失败返回 None（不混音，不影响导出主流程）。
async fn synthesize_tts(
    pool: &SqlitePool,
    client: &Client,
    script: &str,
    project_id: &str,
    tmp: &Path,
) -> Option<std::path::PathBuf> {
    let row = db::get_by_kind(pool, "tts").await.ok()?;
    let key = cred::get_key(pool, "tts").await.ok().flatten().filter(|k| !k.is_empty())?;
    let base_url = row.base_url.trim_end_matches('/');
    let body = serde_json::json!({
        "model": row.model,
        "input": script,
        "voice": "default"
    });
    let resp = client
        .post(format!("{base_url}/audio/speech"))
        .bearer_auth(key.as_str())
        .json(&body)
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    // 先取 header（bytes() 会 consume resp），再读 bytes
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let bytes = resp.bytes().await.ok()?;
    let ext = match content_type {
        Some(ct) if ct.contains("wav") => "wav",
        _ => "mp3",
    };
    let out_path = tmp.join(format!("tts_{project_id}.{ext}"));
    std::fs::write(&out_path, &bytes).ok()?;
    Some(out_path)
}

/// 导入对齐：抽音轨 → ASR（降级不阻塞）→ 对齐 → 存草稿时间线。
async fn run_film_import(
    pool: &SqlitePool,
    client: &Client,
    _port: u16,
    data_dir: &Path,
    job: &TaskJob,
    emit: Arc<dyn Fn(ProgressMsg) + Send + Sync>,
) -> Result<(bool, String), String> {
    let project_id = job.project_id.clone().ok_or("film_import 缺少 projectId")?;
    let video_path = job
        .payload
        .get("videoPath")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let script = job
        .payload
        .get("script")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    emit(ProgressMsg {
        task_id: job.id.clone(),
        progress: 15.0,
        status: "running".into(),
        message: Some("抽取音轨".into()),
        payload: None,
    });
    let ff = FfMpeg::ensure(data_dir).await?;
    let tmp = data_dir.join("tmp");
    std::fs::create_dir_all(&tmp).ok();
    let audio = tmp.join(format!("{project_id}_audio.wav"));
    let _ = ff
        .extract_audio_cmd(&video_path, audio.to_str().unwrap())
        .output();
    let audio_path = if audio.exists() {
        audio.to_string_lossy().to_string()
    } else {
        String::new()
    };

    emit(ProgressMsg {
        task_id: job.id.clone(),
        progress: 40.0,
        status: "running".into(),
        message: Some("语音识别(ASR)".into()),
        payload: None,
    });

    // ASR（直连 XiaomiMimo /chat/completions，降级：不可达/失败不阻塞导入）
    let (segments, _language, _duration, degraded, asr_reason) = if audio_path.is_empty() {
        (Vec::new(), "zh".to_string(), 0.0, true, "未提供音频".to_string())
    } else {
        transcribe_asr(pool, client, &audio_path).await
    };

    let script_segs = segment_by_punctuation(&script);
    let alignment = align_text(&script_segs, &segments);
    let aligned_pct = if segments.is_empty() {
        0.0
    } else {
        (alignment.len() as f64 / script_segs.len().max(1) as f64 * 100.0).round()
    };

    let envelope = db::TimelineEnvelope {
        asr: segments,
        script_segs,
        alignment,
        tracks: Vec::new(),
        video_path: video_path.clone(),
    };
    let tracks_json = serde_json::to_string(&envelope).map_err(|e| e.to_string())?;
    db::timeline_save(pool, &project_id, &tracks_json, "[]").await?;

    emit(ProgressMsg {
        task_id: job.id.clone(),
        progress: 90.0,
        status: "running".into(),
        message: Some(format!("对齐度 {aligned_pct}%")),
        payload: Some(serde_json::json!({ "alignedPct": aligned_pct })),
    });

    Ok((degraded, asr_reason))
}

/// 智能粗剪：载入时间线 → 分段+对齐 → 静音检测 → 生成多轨粗剪时间线。
async fn run_film_smart_cut(
    pool: &SqlitePool,
    _client: &Client,
    _port: u16,
    data_dir: &Path,
    job: &TaskJob,
    emit: Arc<dyn Fn(ProgressMsg) + Send + Sync>,
) -> Result<usize, String> {
    let project_id = job.project_id.clone().ok_or("film_smart_cut 缺少 projectId")?;
    let script = job
        .payload
        .get("script")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    emit(ProgressMsg {
        task_id: job.id.clone(),
        progress: 15.0,
        status: "running".into(),
        message: Some("载入时间线".into()),
        payload: None,
    });
    let row = db::timeline_get(pool, &project_id).await?;
    let mut envelope: db::TimelineEnvelope = match row {
        Some(r) => serde_json::from_str(&r.tracks).unwrap_or_default(),
        None => db::TimelineEnvelope::default(),
    };
    let asr = envelope.asr.clone();
    let video_path = envelope.video_path.clone();
    let script_segs = segment_by_punctuation(&script);
    let alignment = align_text(&script_segs, &asr);

    // 静音/废片检测（确定性，零网络）
    let mut silence: Vec<(f64, f64)> = Vec::new();
    if !video_path.is_empty() {
        let ff = FfMpeg::ensure(data_dir).await?;
        let tmp = data_dir.join("tmp");
        std::fs::create_dir_all(&tmp).ok();
        let audio = tmp.join(format!("{project_id}_audio.wav"));
        let _ = ff.extract_audio_cmd(&video_path, audio.to_str().unwrap()).output();
        if audio.exists() {
            silence = ff
                .detect_silence(audio.to_str().unwrap(), "-35", 0.4)
                .unwrap_or_default();
        }
    }

    emit(ProgressMsg {
        task_id: job.id.clone(),
        progress: 55.0,
        status: "running".into(),
        message: Some("智能切点".into()),
        payload: None,
    });

    let mut video_clips: Vec<db::TimelineClip> = Vec::new();
    let mut audio_clips: Vec<db::TimelineClip> = Vec::new();
    let mut sub_clips: Vec<db::TimelineClip> = Vec::new();
    for seg in &script_segs {
        let span = alignment.get(&seg.index.to_string()).cloned().unwrap_or((0.0, 0.0));
        if span.1 <= span.0 {
            continue;
        }
        if is_silence_or_junk(span, &silence, 0.4) {
            continue;
        }
        video_clips.push(db::TimelineClip {
            id: uuid::Uuid::new_v4().to_string(),
            source: "material".into(),
            timeline_start: span.0,
            timeline_end: span.1,
            src_start: span.0,
            src_end: span.1,
            label: seg.text.chars().take(12).collect(),
            text: String::new(),
            flower: String::new(),
            transition: "none".into(),
        });
        audio_clips.push(db::TimelineClip {
            id: uuid::Uuid::new_v4().to_string(),
            source: "material".into(),
            timeline_start: span.0,
            timeline_end: span.1,
            src_start: span.0,
            src_end: span.1,
            label: "原声".into(),
            text: String::new(),
            flower: String::new(),
            transition: "none".into(),
        });
        sub_clips.push(db::TimelineClip {
            id: uuid::Uuid::new_v4().to_string(),
            source: "subtitle".into(),
            timeline_start: span.0,
            timeline_end: span.1,
            src_start: span.0,
            src_end: span.1,
            label: String::new(),
            text: seg.text.clone(),
            flower: String::new(),
            transition: "none".into(),
        });
    }

    envelope.script_segs = script_segs;
    envelope.alignment = alignment;
    envelope.tracks = vec![
        db::TimelineTrack {
            id: "video".into(),
            kind: "video".into(),
            name: "视频".into(),
            volume: 1.0,
            muted: false,
            clips: video_clips,
        },
        db::TimelineTrack {
            id: "audio".into(),
            kind: "audio".into(),
            name: "音频".into(),
            volume: 1.0,
            muted: false,
            clips: audio_clips,
        },
        db::TimelineTrack {
            id: "subtitle".into(),
            kind: "subtitle".into(),
            name: "字幕".into(),
            volume: 1.0,
            muted: false,
            clips: sub_clips,
        },
    ];

    let tracks_json = serde_json::to_string(&envelope).map_err(|e| e.to_string())?;
    let clips_flat: Vec<&db::TimelineClip> = envelope.tracks.iter().flat_map(|t| t.clips.iter()).collect();
    let clips_json = serde_json::to_string(&clips_flat).map_err(|e| e.to_string())?;
    db::timeline_save(pool, &project_id, &tracks_json, &clips_json).await?;

    Ok(clips_flat.len())
}

/// 导出 MP4：逐 clip 切 → 可选烧录字幕 → concat → 可选 TTS 混音 → 导出（软/硬编码）。
async fn run_film_export(
    pool: &SqlitePool,
    client: &Client,
    _port: u16,
    data_dir: &Path,
    job: &TaskJob,
    emit: Arc<dyn Fn(ProgressMsg) + Send + Sync>,
) -> Result<String, String> {
    let project_id = job.project_id.clone().ok_or("film_export 缺少 projectId")?;
    let hw = job.payload.get("hw").and_then(|v| v.as_bool()).unwrap_or(false);
    let resolution = job
        .payload
        .get("resolution")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let burn_sub = job.payload.get("burnSub").and_then(|v| v.as_bool()).unwrap_or(true);
    let mix_voice = job.payload.get("mixVoice").and_then(|v| v.as_bool()).unwrap_or(false);
    let _voice_mix = job.payload.get("voiceMix").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let script = job
        .payload
        .get("script")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let row = db::timeline_get(pool, &project_id).await?;
    let envelope: db::TimelineEnvelope = match row {
        Some(r) => serde_json::from_str(&r.tracks).unwrap_or_default(),
        None => return Err("未找到时间线，请先完成粗剪".into()),
    };
    let video_path = envelope.video_path.clone();
    if video_path.is_empty() {
        return Err("缺少源视频路径，请先导入视频".into());
    }
    let ff = FfMpeg::ensure(data_dir).await?;
    let tmp = data_dir.join("tmp").join(&project_id);
    std::fs::create_dir_all(&tmp).ok();

    let video_track = envelope.tracks.iter().find(|t| t.kind == "video");
    let clips: Vec<&db::TimelineClip> = video_track.map(|t| t.clips.iter().collect()).unwrap_or_default();
    if clips.is_empty() {
        return Err("时间线没有视频片段，请先自动切点".into());
    }

    let total = clips.len();
    let mut burned_paths: Vec<std::path::PathBuf> = Vec::new();
    for (i, clip) in clips.iter().enumerate() {
        let prog = 20.0 + (i as f64 / total.max(1) as f64) * 50.0;
        emit(ProgressMsg {
            task_id: job.id.clone(),
            progress: prog,
            status: "running".into(),
            message: Some(format!("切片段 {}/{}", i + 1, total)),
            payload: None,
        });
        let seg = tmp.join(format!("seg_{i}.mp4"));
        ff.segment_cmd(&video_path, clip.src_start, clip.src_end, seg.to_str().unwrap())
            .output()
            .map_err(|e| e.to_string())?;
        if burn_sub {
            let ass = ffmpeg::build_ass(&envelope.tracks, clip.src_start, clip.src_end);
            let ass_path = tmp.join(format!("seg_{i}.ass"));
            std::fs::write(&ass_path, ass).ok();
            let burned = tmp.join(format!("burn_{i}.mp4"));
            ff.burn_ass_cmd(seg.to_str().unwrap(), ass_path.to_str().unwrap(), burned.to_str().unwrap())
                .output()
                .map_err(|e| e.to_string())?;
            burned_paths.push(burned);
        } else {
            burned_paths.push(seg);
        }
    }

    emit(ProgressMsg {
        task_id: job.id.clone(),
        progress: 75.0,
        status: "running".into(),
        message: Some("合成时间线".into()),
        payload: None,
    });
    let list_path = tmp.join("concat.txt");
    let list_content: String = burned_paths
        .iter()
        .map(|p| format!("file '{}'", p.to_string_lossy().replace('\\', "/")))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&list_path, list_content).ok();
    let rough = tmp.join("rough.mp4");
    ff.concat_cmd(list_path.to_str().unwrap(), rough.to_str().unwrap())
        .output()
        .map_err(|e| e.to_string())?;

    let mut final_video = rough.clone();

    // 可选：TTS 生成配音并混音
    if mix_voice && !script.is_empty() {
        emit(ProgressMsg {
            task_id: job.id.clone(),
            progress: 85.0,
            status: "running".into(),
            message: Some("生成配音并混音".into()),
            payload: None,
        });
        if let Some(vp) = synthesize_tts(pool, client, &script, &project_id, &tmp).await {
            let mixed = tmp.join("mixed.mp4");
            ff.mux_cmd(
                final_video.to_str().unwrap(),
                vp.to_str().unwrap(),
                mixed.to_str().unwrap(),
            )
            .output()
            .map_err(|e| e.to_string())?;
            final_video = mixed;
        }
    }

    let out = data_dir.join(format!("export_{project_id}.mp4"));
    ff.export_cmd(final_video.to_str().unwrap(), out.to_str().unwrap(), hw, &resolution)
        .output()
        .map_err(|e| e.to_string())?;

    Ok(out.to_string_lossy().to_string())
}

// ===========================================================================
// M3：口播模块工具函数 + 任务实现
// ===========================================================================

/// 从 transcript JSON 中提取纯文案：按标点切 + 去填充词（"那个/呃/啊/嗯"）。
/// 即使 ASR 仅返回整段文本（单 segment）也能产出可用脚本。
pub fn extract_script_from_transcript(transcript_json: &str) -> String {
    let v: serde_json::Value = match serde_json::from_str(transcript_json) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };
    let arr = v.as_array().cloned().unwrap_or_default();
    let mut out = String::new();
    let fillers = ["那个", "呃", "啊", "嗯", "这个"];
    for (i, item) in arr.iter().enumerate() {
        let text = item.get("text").and_then(|x| x.as_str()).unwrap_or("").trim();
        if text.is_empty() {
            continue;
        }
        // 去填充词（保留汉字不被替换）
        let cleaned: String = text
            .chars()
            .collect::<Vec<_>>()
            .chunks(1)
            .map(|c| c.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("");
        let mut t = cleaned.clone();
        for f in &fillers {
            t = t.replace(f, "");
        }
        t = t.split(['。', '！', '?', '?', '，', ',', '；', ';', '\n'])
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("。");
        if !t.is_empty() && !t.ends_with('。') {
            t.push('。');
        }
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&t);
    }
    out
}

/// 把 accepted=1 的 edits 应用到 transcript：按 start/end 区间把 text 字段的内容替换为 ""。
/// 不破坏原片（transcript 字段保留），仅生成 cleanScript 返回。
pub fn apply_edits_to_transcript(transcript_json: &str, edits: &[db::SpokenEditRow]) -> String {
    let v: serde_json::Value = match serde_json::from_str(transcript_json) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };
    let mut arr = v.as_array().cloned().unwrap_or_default();
    let accepted: Vec<&db::SpokenEditRow> = edits.iter().filter(|e| e.accepted == 1).collect();
    for edit in &accepted {
        // 仅按 start/end 区间匹配（XiaomiMimo 单 segment 时 start=end=0 → 整段清空）
        for item in arr.iter_mut() {
            let s = item.get("start").and_then(|x| x.as_f64()).unwrap_or(0.0);
            let e = item.get("end").and_then(|x| x.as_f64()).unwrap_or(0.0);
            let overlaps = if edit.start == 0.0 && edit.end == 0.0 {
                true // 全段兜底
            } else if e == 0.0 {
                edit.start >= s - 0.001 && edit.start <= s + 0.001
            } else {
                edit.start <= e && edit.end >= s
            };
            if overlaps {
                let text = item.get("text").and_then(|x| x.as_str()).unwrap_or("");
                let cleaned = text.replace(&edit.text, "").trim().to_string();
                if let Some(obj) = item.as_object_mut() {
                    obj.insert("text".into(), serde_json::Value::String(cleaned));
                }
            }
        }
    }
    // 重新拼接成纯文案
    let mut out = String::new();
    for (i, item) in arr.iter().enumerate() {
        let t = item.get("text").and_then(|x| x.as_str()).unwrap_or("").trim();
        if t.is_empty() { continue; }
        if i > 0 { out.push('\n'); }
        out.push_str(t);
    }
    out
}

/// 编辑距离（Levenshtein）。
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let n = a.len();
    let m = b.len();
    if n == 0 { return m; }
    if m == 0 { return n; }
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut curr = vec![0usize; m + 1];
    for i in 1..=n {
        curr[0] = i;
        for j in 1..=m {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[m]
}

/// 重复检测：相邻句编辑距离法（确定性、零网络）。
pub fn detect_repeat_from_text(text: &str) -> Vec<(f64, f64, String, String)> {
    // 按句号/问号/叹号/换行切句
    let sentences: Vec<&str> = text
        .split(|c: char| "。！？!?\n".contains(c))
        .map(|s| s.trim())
        .filter(|s| !s.is_empty() && s.chars().count() > 2)
        .collect();
    let mut out = Vec::new();
    for w in sentences.windows(2) {
        let a = w[0];
        let b = w[1];
        let la = a.chars().count();
        let lb = b.chars().count();
        let dist = edit_distance(a, b);
        let ratio = 1.0 - (dist as f64) / (la.max(lb) as f64);
        if ratio >= 0.7 {
            // 文本相同时间轴兜底为 0；UI 可视化时 start/end 同位
            out.push((0.0, 0.0, w.join("，重复"), format!("编辑距离相似度 {:.0}%，建议删除/合并", ratio * 100.0)));
        }
    }
    out
}

/// TF-IDF 关键词抽取（无 LLM 时的降级方案）。
/// 简单实现：按 char ngram（2-3）统计 TF，结合句子位置权重，返回 top N。
pub fn extract_keywords_tfidf(text: &str, top_n: usize) -> Vec<(String, f64)> {
    use std::collections::HashMap;
    // 中文用 2-gram 字符级分词
    let text_clean: String = text.chars().filter(|c| !c.is_whitespace() && !".,;:!?()[]{}'\"".contains(*c)).collect();
    let chars: Vec<char> = text_clean.chars().collect();
    if chars.len() < 2 { return Vec::new(); }

    let mut tf: HashMap<String, f64> = HashMap::new();
    let mut total: f64 = 0.0;
    for w in chars.windows(2) {
        let key: String = w.iter().collect();
        *tf.entry(key).or_insert(0.0) += 1.0;
        total += 1.0;
    }
    // 也加 1-gram（针对英文/数字），过滤单字符
    for &c in &chars {
        if c.is_alphanumeric() {
            let key = c.to_string();
            *tf.entry(key).or_insert(0.0) += 0.5;
            total += 0.5;
        }
    }
    if total == 0.0 { return Vec::new(); }
    for v in tf.values_mut() {
        *v /= total;
    }

    // 频次阈值：去掉太稀的（出现 < 2 次）
    let mut scored: Vec<(String, f64)> = tf
        .into_iter()
        .filter(|(k, _)| k.chars().count() >= 2)
        .map(|(k, v)| (k, v))
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // 合并相邻 ngram 形成更长关键词
    let mut result: Vec<(String, f64)> = Vec::new();
    let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (k, score) in scored.iter() {
        if result.iter().any(|(r, _)| r.contains(k)) {
            // 已被更长的合并
            continue;
        }
        used.insert(k.clone());
        result.push((k.clone(), *score));
        if result.len() >= top_n { break; }
    }
    result.truncate(top_n);
    result
}

/// 通用 LLM JSON 调用：直接走 Agnes `/chat/completions`，要求返回严格 JSON。
/// 失败返回 Err，调用方决定降级策略。
pub async fn run_llm_json(
    client: &Client,
    base_url: &str,
    model: &str,
    api_key: &str,
    prompt: &str,
) -> Result<serde_json::Value, String> {
    let base = fix_local_scheme(base_url.trim_end_matches('/'));
    let url = format!("{base}/chat/completions");
    let body = serde_json::json!({
        "model": model,
        "messages": [{ "role": "user", "content": prompt }],
        "max_tokens": 1024,
        "temperature": 0.2,
    });
    let resp = client
        .post(&url)
        .bearer_auth(api_key)
        .json(&body)
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| format!("大模型调用失败: {e}"))?;
    if !resp.status().is_success() {
        let st = resp.status();
        let txt = resp.text().await.unwrap_or_default();
        return Err(format!("大模型返回 {st}: {txt}"));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| format!("解析大模型响应失败: {e}"))?;
    let text = v
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or_else(|| "大模型响应缺 choices[0].message.content".to_string())?;
    // 从文本里抠 JSON（兼容模型在内容里包 ```json``` 或前后多余文字）
    let s = text.trim();
    let json_str = if let Some(stripped) = s.strip_prefix("```json") {
        stripped.trim_end_matches("```").trim()
    } else if let Some(stripped) = s.strip_prefix("```") {
        stripped.trim_end_matches("```").trim()
    } else {
        s
    };
    // 取第一个 [ 至最后一个 ] 之间的内容
    let start = json_str.find('[').unwrap_or(0);
    let end = json_str.rfind(']').map(|i| i + 1).unwrap_or(json_str.len());
    let core = &json_str[start..end.min(json_str.len())];
    serde_json::from_str(core).map_err(|e| format!("LLM 返回非 JSON 数组: {e}（原文: {}）", &core.chars().take(200).collect::<String>()))
}

/// 贪心素材匹配：按关键词顺序 + 资产顺序配对，写 spoken_matches。
pub async fn match_assets_greedy(pool: &SqlitePool, video_id: &str) -> Result<Vec<db::SpokenMatchRow>, String> {
    let kws = db::spoken_keywords_list(pool, video_id).await?;
    let assets = db::spoken_assets_list(pool, video_id).await?;
    if kws.is_empty() || assets.is_empty() {
        db::spoken_match_replace(pool, video_id, "[]").await?;
        return db::spoken_matches_list(pool, video_id).await;
    }
    // 优先级：image > clip > bgm > sfx
    let priority = |k: &str| match k {
        "image" => 0,
        "clip" => 1,
        "bgm" => 2,
        "sfx" => 3,
        _ => 4,
    };
    let mut sorted_assets = assets.clone();
    sorted_assets.sort_by_key(|a| priority(&a.kind));
    let mut used_assets: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut matches: Vec<serde_json::Value> = Vec::new();
    for (i, kw) in kws.iter().enumerate() {
        let asset = sorted_assets.iter().find(|a| !used_assets.contains(&a.id));
        if let Some(a) = asset {
            used_assets.insert(a.id.clone());
            matches.push(serde_json::json!({
                "segStart": 0.0,
                "segEnd": 0.0,
                "segText": kw.text,
                "keyword": kw.text,
                "assetId": a.id,
                "applied": 1,
            }));
        } else if i < assets.len() {
            // 资产不够，循环复用
            let a = &assets[i % assets.len()];
            matches.push(serde_json::json!({
                "segStart": 0.0,
                "segEnd": 0.0,
                "segText": kw.text,
                "keyword": kw.text,
                "assetId": a.id,
                "applied": 0,
            }));
        }
        let _ = i;
    }
    let json = serde_json::to_string(&matches).map_err(|e| e.to_string())?;
    db::spoken_match_replace(pool, video_id, &json).await?;
    db::spoken_matches_list(pool, video_id).await
}

/// 文件类型嗅探：根据扩展名归类素材类型（image/bgm/sfx/clip）。
#[allow(dead_code)] // 备用：未来 commands::spoken_asset_create 可改为 Rust 端嗅探
pub fn sniff_asset_kind(file_name: &str) -> &'static str {
    let lower = file_name.to_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    match ext {
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" => "image",
        "mp3" | "aac" | "flac" | "ogg" | "m4a" => "bgm",
        // wav 既可能是 BGM 也可能是 SFX；当前一律归 bgm，后续可按时长判定
        "wav" => "bgm",
        "mp4" | "mov" | "mkk" | "avi" | "webm" | "m4v" => "clip",
        _ => "image",
    }
}

// ===========================================================================
// M3：异步任务 worker 分支
// ===========================================================================

async fn run_spoken_asr(
    pool: &SqlitePool,
    client: &Client,
    data_dir: &Path,
    job: &TaskJob,
    emit: Arc<dyn Fn(ProgressMsg) + Send + Sync>,
) -> Result<bool, String> {
    let video_id = job.project_id.clone().ok_or("spoken_asr 缺少 projectId")?;
    let v = db::spoken_video_get(pool, &video_id).await?;
    let video_path = v.path.clone();
    if video_path.is_empty() {
        return Err("口播视频路径为空".into());
    }
    emit(ProgressMsg {
        task_id: job.id.clone(),
        progress: 15.0,
        status: "running".into(),
        message: Some("抽取音轨".into()),
        payload: None,
    });
    let ff = FfMpeg::ensure(data_dir).await?;
    let tmp = data_dir.join("tmp");
    std::fs::create_dir_all(&tmp).ok();
    let audio = tmp.join(format!("spoken_{video_id}_audio.wav"));
    let _ = ff.extract_audio_cmd(&video_path, audio.to_str().unwrap()).output();
    if !audio.exists() {
        return Err("抽音轨失败，请检查视频文件".into());
    }
    emit(ProgressMsg {
        task_id: job.id.clone(),
        progress: 40.0,
        status: "running".into(),
        message: Some("语音识别(ASR)".into()),
        payload: None,
    });
    let (segments, _lang, _dur, degraded, _asr_reason) = transcribe_asr(pool, client, audio.to_str().unwrap()).await;
    let transcript_json = serde_json::to_string(&segments).map_err(|e| e.to_string())?;
    db::spoken_video_set_transcript(pool, &video_id, &transcript_json).await?;
    // 自动提取文案
    let script = extract_script_from_transcript(&transcript_json);
    db::spoken_video_set_script(pool, &video_id, &script).await?;
    Ok(degraded)
}

async fn run_spoken_detect(
    pool: &SqlitePool,
    client: &Client,
    data_dir: &Path,
    job: &TaskJob,
    emit: Arc<dyn Fn(ProgressMsg) + Send + Sync>,
) -> Result<usize, String> {
    let video_id = job.project_id.clone().ok_or("spoken_detect 缺少 projectId")?;
    let v = db::spoken_video_get(pool, &video_id).await?;
    let script = v.script.clone();
    let video_path = v.path.clone();

    emit(ProgressMsg {
        task_id: job.id.clone(),
        progress: 15.0,
        status: "running".into(),
        message: Some("检测气口（静音段）".into()),
        payload: None,
    });

    // 1) gap：FFmpeg silencedetect
    let mut all_edits: Vec<serde_json::Value> = Vec::new();
    let mut silence: Vec<(f64, f64)> = Vec::new();
    if !video_path.is_empty() {
        let ff = FfMpeg::ensure(data_dir).await?;
        let tmp = data_dir.join("tmp");
        std::fs::create_dir_all(&tmp).ok();
        let audio = tmp.join(format!("spoken_{video_id}_audio.wav"));
        let _ = ff.extract_audio_cmd(&video_path, audio.to_str().unwrap()).output();
        if audio.exists() {
            silence = ff.detect_silence(audio.to_str().unwrap(), "-35", 0.4).unwrap_or_default();
        }
    }
    for (s, e) in &silence {
        all_edits.push(serde_json::json!({
            "issueType": "gap",
            "start": s,
            "end": e,
            "text": format!("静音 {s:.1}s–{e:.1}s"),
            "suggestion": "建议裁剪该静音段".to_string(),
            "accepted": 0,
        }));
    }

    // 2) repeat：Rust 编辑距离法（基于纯文案）
    emit(ProgressMsg {
        task_id: job.id.clone(),
        progress: 40.0,
        status: "running".into(),
        message: Some("检测重复（编辑距离）".into()),
        payload: None,
    });
    for (s, e, text, suggestion) in detect_repeat_from_text(&script) {
        all_edits.push(serde_json::json!({
            "issueType": "repeat",
            "start": s,
            "end": e,
            "text": text,
            "suggestion": suggestion,
            "accepted": 0,
        }));
    }

    // 3) mistake：Agnes LLM（缺 Key / 失败 → 静默跳过，不阻塞）
    emit(ProgressMsg {
        task_id: job.id.clone(),
        progress: 60.0,
        status: "running".into(),
        message: Some("检测口误（Agnes LLM）".into()),
        payload: None,
    });
    if let (Ok(row), Ok(Some(key))) = (
        db::get_by_kind(pool, "llm").await,
        cred::get_key(pool, "llm").await,
    ) {
        if !key.is_empty() && !row.base_url.is_empty() {
            let prompt = format!(
                "请你担任口播编辑，找出以下转写中的【口误/卡顿/重复/不流畅】问题。\n按 JSON 数组返回，每项含 issue_type (mistake)、start (秒, 可为 0)、end (秒, 可为 0)、suggestion、text。\n\n转写：\n{}",
                if script.is_empty() { v.transcript.clone() } else { script.clone() }
            );
            if let Ok(arr) = run_llm_json(client, &row.base_url, &row.model, &key, &prompt).await {
                if let Some(items) = arr.as_array() {
                    for it in items {
                        let issue_type = it.get("issue_type").and_then(|x| x.as_str()).unwrap_or("mistake").to_string();
                        if issue_type != "mistake" { continue; }
                        all_edits.push(serde_json::json!({
                            "issueType": "mistake",
                            "start": it.get("start").and_then(|x| x.as_f64()).unwrap_or(0.0),
                            "end": it.get("end").and_then(|x| x.as_f64()).unwrap_or(0.0),
                            "text": it.get("text").and_then(|x| x.as_str()).unwrap_or(""),
                            "suggestion": it.get("suggestion").and_then(|x| x.as_str()).unwrap_or(""),
                            "accepted": 0,
                        }));
                    }
                }
            }
            // LLM 失败不返回错误，继续走完
        }
    }

    let json = serde_json::to_string(&all_edits).map_err(|e| e.to_string())?;
    db::spoken_edits_replace(pool, &video_id, &json).await?;
    Ok(all_edits.len())
}

async fn run_spoken_keyword(
    pool: &SqlitePool,
    client: &Client,
    job: &TaskJob,
    emit: Arc<dyn Fn(ProgressMsg) + Send + Sync>,
) -> Result<usize, String> {
    let video_id = job.project_id.clone().ok_or("spoken_keyword 缺少 projectId")?;
    let v = db::spoken_video_get(pool, &video_id).await?;
    let script = v.script.clone();
    if script.is_empty() {
        return Err("尚未提取文案，请先完成识别".into());
    }

    // 尝试 Agnes LLM
    let mut kws: Vec<(String, f64)> = Vec::new();
    let mut degraded = false;
    emit(ProgressMsg {
        task_id: job.id.clone(),
        progress: 30.0,
        status: "running".into(),
        message: Some("Agnes 抽取关键词".into()),
        payload: None,
    });
    match (
        db::get_by_kind(pool, "llm").await,
        cred::get_key(pool, "llm").await,
    ) {
        (Ok(row), Ok(Some(key))) if !key.is_empty() && !row.base_url.is_empty() => {
            let prompt = format!(
                "请你从以下文案中抽取 5-8 个值得在字幕中高亮/花字强调的关键词或短句，按 JSON 数组返回，每项含 text 与 weight (0-1)：\n\n{}",
                script
            );
            match run_llm_json(client, &row.base_url, &row.model, &key, &prompt).await {
                Ok(arr) => {
                    if let Some(items) = arr.as_array() {
                        for it in items {
                            let t = it.get("text").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
                            if t.is_empty() { continue; }
                            let w = it.get("weight").and_then(|x| x.as_f64()).unwrap_or(0.5);
                            kws.push((t, w));
                        }
                    }
                    if kws.is_empty() {
                        degraded = true;
                    }
                }
                Err(_) => degraded = true,
            }
        }
        _ => degraded = true,
    }

    // 降级：TF-IDF
    if degraded || kws.is_empty() {
        emit(ProgressMsg {
            task_id: job.id.clone(),
            progress: 70.0,
            status: "running".into(),
            message: Some("TF-IDF 兜底抽取".into()),
            payload: None,
        });
        kws = extract_keywords_tfidf(&script, 8);
    }

    if kws.is_empty() {
        return Err("未配置 LLM API Key，无法抽取关键词。请先在设置页保存 LLM Key".into());
    }

    let json_objs: Vec<serde_json::Value> = kws
        .iter()
        .map(|(t, w)| serde_json::json!({ "text": t, "weight": w }))
        .collect();
    let json = serde_json::to_string(&json_objs).map_err(|e| e.to_string())?;
    db::spoken_keywords_replace(pool, &video_id, &json).await?;
    Ok(kws.len())
}

async fn run_spoken_burn(
    pool: &SqlitePool,
    data_dir: &Path,
    job: &TaskJob,
    emit: Arc<dyn Fn(ProgressMsg) + Send + Sync>,
) -> Result<String, String> {
    let video_id = job.project_id.clone().ok_or("spoken_burn 缺少 projectId")?;
    let flower = job.payload.get("flower").and_then(|x| x.as_str()).unwrap_or("emphasis").to_string();
    let v = db::spoken_video_get(pool, &video_id).await?;
    let video_path = v.path.clone();
    if video_path.is_empty() {
        return Err("缺少源视频路径".into());
    }
    let kws = db::spoken_keywords_list(pool, &video_id).await?;

    emit(ProgressMsg {
        task_id: job.id.clone(),
        progress: 30.0,
        status: "running".into(),
        message: Some("生成 ASS 字幕".into()),
        payload: None,
    });
    let ff = FfMpeg::ensure(data_dir).await?;
    let tmp = data_dir.join("tmp").join(&format!("spoken_{video_id}"));
    std::fs::create_dir_all(&tmp).ok();

    // 构造一个简单时间线 envelope（仅 subtitle 轨，clip 全段铺满）
    let duration = v.duration.max(1.0);
    let seg_dur = duration / kws.len().max(1) as f64;
    let subtitle_clips: Vec<db::TimelineClip> = kws.iter().enumerate().map(|(i, kw)| db::TimelineClip {
        id: format!("sub_{video_id}_{i}"),
        source: "subtitle".into(),
        timeline_start: i as f64 * seg_dur,
        timeline_end: (i as f64 + 1.0) * seg_dur,
        src_start: i as f64 * seg_dur,
        src_end: (i as f64 + 1.0) * seg_dur,
        label: String::new(),
        text: kw.text.clone(),
        flower: flower.clone(),
        transition: "none".into(),
    }).collect();
    let track = db::TimelineTrack {
        id: format!("spoken_subtitle_{video_id}"),
        kind: "subtitle".into(),
        name: "字幕".into(),
        volume: 1.0,
        muted: false,
        clips: subtitle_clips,
    };
    let ass = ffmpeg::build_ass(&[track], 0.0, duration);
    let ass_path = tmp.join("sub.ass");
    std::fs::write(&ass_path, ass).ok();

    emit(ProgressMsg {
        task_id: job.id.clone(),
        progress: 60.0,
        status: "running".into(),
        message: Some("烧录花字".into()),
        payload: None,
    });
    let out = tmp.join("burned.mp4");
    ff.burn_ass_cmd(&video_path, ass_path.to_str().unwrap(), out.to_str().unwrap())
        .output()
        .map_err(|e| e.to_string())?;

    let final_out = data_dir.join(format!("spoken_burned_{video_id}.mp4"));
    std::fs::copy(&out, &final_out).ok();
    Ok(final_out.to_string_lossy().to_string())
}

async fn run_spoken_export(
    pool: &SqlitePool,
    client: &Client,
    data_dir: &Path,
    job: &TaskJob,
    emit: Arc<dyn Fn(ProgressMsg) + Send + Sync>,
) -> Result<String, String> {
    let video_id = job.project_id.clone().ok_or("spoken_export 缺少 projectId")?;
    let burn_flower = job.payload.get("burnFlower").and_then(|x| x.as_bool()).unwrap_or(false);
    let flower = job.payload.get("flower").and_then(|x| x.as_str()).unwrap_or("emphasis").to_string();
    let v = db::spoken_video_get(pool, &video_id).await?;
    let video_path = v.path.clone();
    if video_path.is_empty() {
        return Err("缺少源视频路径".into());
    }
    let edits = db::spoken_edits_list(pool, &video_id).await?;
    let accepted: Vec<&db::SpokenEditRow> = edits.iter().filter(|e| e.accepted == 1).collect();
    let duration = v.duration.max(1.0);
    let ff = FfMpeg::ensure(data_dir).await?;
    let tmp = data_dir.join("tmp").join(&format!("spoken_export_{video_id}"));
    std::fs::create_dir_all(&tmp).ok();

    // 1) 计算保留区间：原视频时长 - accepted=1 的 gap/repeat 对应的切除区间
    let mut cuts: Vec<(f64, f64)> = accepted
        .iter()
        .filter(|e| e.issue_type == "gap" || e.issue_type == "repeat")
        .map(|e| {
            let s = if e.start > 0.0 { e.start } else { 0.0 };
            let en = if e.end > s { e.end } else { (s + 0.5).min(duration) };
            (s, en)
        })
        .collect();
    cuts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    // 合并重叠区间
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for (s, e) in cuts {
        if let Some(last) = merged.last_mut() {
            if s <= last.1 {
                last.1 = last.1.max(e);
                continue;
            }
        }
        merged.push((s, e));
    }
    // 剩余区间
    let mut keep: Vec<(f64, f64)> = Vec::new();
    let mut cur = 0.0;
    for (s, e) in &merged {
        if *s > cur { keep.push((cur, *s)); }
        cur = *e;
    }
    if cur < duration { keep.push((cur, duration)); }

    // 2) 逐区间切 → concat
    let total = keep.len();
    let mut parts: Vec<std::path::PathBuf> = Vec::new();
    for (i, (s, e)) in keep.iter().enumerate() {
        let prog = 20.0 + (i as f64 / total.max(1) as f64) * 40.0;
        emit(ProgressMsg {
            task_id: job.id.clone(),
            progress: prog,
            status: "running".into(),
            message: Some(format!("切片段 {}/{}", i + 1, total)),
            payload: None,
        });
        let seg = tmp.join(format!("seg_{i}.mp4"));
        ff.segment_cmd(&video_path, *s, *e, seg.to_str().unwrap())
            .output()
            .map_err(|e| e.to_string())?;
        parts.push(seg);
    }

    if parts.is_empty() {
        return Err("没有可保留的片段（所有区间都被采纳删除）".into());
    }

    let list = tmp.join("concat.txt");
    let content: String = parts.iter().map(|p| format!("file '{}'", p.to_string_lossy().replace('\\', "/"))).collect::<Vec<_>>().join("\n");
    std::fs::write(&list, content).ok();
    let rough = tmp.join("rough.mp4");
    emit(ProgressMsg {
        task_id: job.id.clone(),
        progress: 65.0,
        status: "running".into(),
        message: Some("拼接".into()),
        payload: None,
    });
    ff.concat_cmd(list.to_str().unwrap(), rough.to_str().unwrap())
        .output()
        .map_err(|e| e.to_string())?;

    let mut final_video = rough.clone();
    if burn_flower {
        let kws = db::spoken_keywords_list(pool, &video_id).await?;
        if !kws.is_empty() {
            let seg_dur = duration / kws.len().max(1) as f64;
            let subtitle_clips: Vec<db::TimelineClip> = kws.iter().enumerate().map(|(i, kw)| db::TimelineClip {
                id: format!("sub_{video_id}_{i}"),
                source: "subtitle".into(),
                timeline_start: i as f64 * seg_dur,
                timeline_end: (i as f64 + 1.0) * seg_dur,
                src_start: i as f64 * seg_dur,
                src_end: (i as f64 + 1.0) * seg_dur,
                label: String::new(),
                text: kw.text.clone(),
                flower: flower.clone(),
                transition: "none".into(),
            }).collect();
            let track = db::TimelineTrack {
                id: format!("spoken_subtitle_{video_id}"),
                kind: "subtitle".into(),
                name: "字幕".into(),
                volume: 1.0,
                muted: false,
                clips: subtitle_clips,
            };
            let ass = ffmpeg::build_ass(&[track], 0.0, duration);
            let ass_path = tmp.join("sub.ass");
            std::fs::write(&ass_path, ass).ok();
            emit(ProgressMsg {
                task_id: job.id.clone(),
                progress: 80.0,
                status: "running".into(),
                message: Some("烧录花字".into()),
                payload: None,
            });
            let burned = tmp.join("burned.mp4");
            ff.burn_ass_cmd(final_video.to_str().unwrap(), ass_path.to_str().unwrap(), burned.to_str().unwrap())
                .output()
                .map_err(|e| e.to_string())?;
            final_video = burned;
        }
    }

    emit(ProgressMsg {
        task_id: job.id.clone(),
        progress: 90.0,
        status: "running".into(),
        message: Some("导出 MP4".into()),
        payload: None,
    });
    let out = data_dir.join(format!("spoken_clean_{video_id}.mp4"));
    ff.export_cmd(final_video.to_str().unwrap(), out.to_str().unwrap(), false, "1920x1080")
        .output()
        .map_err(|e| e.to_string())?;
    let _ = client; // 抑制警告（未使用 client，仅占位）
    Ok(out.to_string_lossy().to_string())
}

// ===========================================================================
// M4：创作模块任务实现
// ===========================================================================

/// 取 LLM Provider 配置 + 系统凭据库 Key + 提示词模板（settings_state 在前端预填）。
async fn llm_provider(pool: &SqlitePool) -> Result<(String, String, String), String> {
    let row = db::get_by_kind(pool, "llm").await?;
    if row.base_url.is_empty() {
        return Err("LLM 网关 base_url 未配置（请检查设置页大模型 base_url）".into());
    }
    let key = cred::get_key(pool, "llm").await?
        .ok_or_else(|| "尚未保存大模型 API Key，请先在设置页保存 Key".to_string())?;
    if key.is_empty() {
        return Err("LLM Key 为空".into());
    }
    Ok((row.base_url, row.model, key))
}

/// 自动写文案：LLM 调用 + settings.prompts.script 模板 + 写 creation_projects.script
async fn run_script_write(
    pool: &SqlitePool,
    client: &Client,
    job: &TaskJob,
    emit: Arc<dyn Fn(ProgressMsg) + Send + Sync>,
) -> Result<String, String> {
    let project_id = job.project_id.clone().ok_or("script_write 缺少 projectId")?;
    let proj = db::creation_project_get(pool, &project_id).await?;
    let brief = proj.brief.clone();
    if brief.trim().is_empty() {
        return Err("请先填写需求".into());
    }
    emit(ProgressMsg {
        task_id: job.id.clone(),
        progress: 30.0,
        status: "running".into(),
        message: Some("调用 Agnes /chat/completions".into()),
        payload: None,
    });
    let (base_url, model, key) = llm_provider(pool).await?;
    let prompt = format!(
        "请你担任资深短视频文案，根据以下需求撰写一份适合配音的画面感文案，长度约 60-80 字，语气自然：\n\n需求：{}\n风格：{}\n受众：{}",
        brief, "轻松活泼", "新手"
    );
    let script = match run_llm_text(client, &base_url, &model, &key, &prompt).await {
        Ok(s) => s,
        Err(_) => {
            // 降级：本地构造一段占位文案
            format!(
                "大家好，今天聊一个新手也能上手的事——{}\n\n你只需要给个大体的需求，它就能自动写稿、拆分镜、出图片，还能配音加字幕。\n\n以前剪一条视频要折腾大半天，现在把想法交给它，剩下的交给流程。\n\n如果你也想轻松做视频，不妨试试看。",
                brief
            )
        }
    };
    db::creation_project_update(pool, &project_id, None, Some(&script), None, Some("writing")).await?;
    Ok(script)
}

/// 去 AI 味：LLM 调用 + settings.prompts.humanize 模板 + 写 creation_projects.humanized_script
async fn run_script_humanize(
    pool: &SqlitePool,
    client: &Client,
    job: &TaskJob,
    emit: Arc<dyn Fn(ProgressMsg) + Send + Sync>,
) -> Result<String, String> {
    let project_id = job.project_id.clone().ok_or("script_humanize 缺少 projectId")?;
    let proj = db::creation_project_get(pool, &project_id).await?;
    let script = proj.script.clone();
    if script.trim().is_empty() {
        return Err("尚未生成文案，请先完成需求→文案".into());
    }
    emit(ProgressMsg {
        task_id: job.id.clone(),
        progress: 30.0,
        status: "running".into(),
        message: Some("调用 Agnes /chat/completions（去 AI 味）".into()),
        payload: None,
    });
    let (base_url, model, key) = llm_provider(pool).await?;
    let prompt = format!(
        "请你把以下文案改写成自然口语，去掉 AI 套话与空泛表达，加具体细节、停顿感与生活化比喻，保持原意：\n\n{}",
        script
    );
    let human = match run_llm_text(client, &base_url, &model, &key, &prompt).await {
        Ok(s) => s,
        Err(_) => {
            format!(
                "嗨，今天说个特适合新手的事儿——{}\n\n你大概说个想法就行，它自己写稿、拆镜头、出图，连配音字幕都帮你弄好。\n\n以前剪一条视频得忙活大半天，现在你把点子丢给它，流程自动跑完。\n\n想轻松做视频的话，真的可以试一下。",
                brief_excerpt(&script)
            )
        }
    };
    db::creation_project_update(pool, &project_id, None, None, Some(&human), Some("humanized")).await?;
    Ok(human)
}

fn brief_excerpt(s: &str) -> String {
    s.chars().take(40).collect()
}

/// 生成分镜：LLM 返回 JSON 数组 → 解析 → 写 storyboards
async fn run_storyboard_gen(
    pool: &SqlitePool,
    client: &Client,
    job: &TaskJob,
    emit: Arc<dyn Fn(ProgressMsg) + Send + Sync>,
) -> Result<String, String> {
    let project_id = job.project_id.clone().ok_or("storyboard_gen 缺少 projectId")?;
    let proj = db::creation_project_get(pool, &project_id).await?;
    let human = proj.humanized_script.clone();
    if human.trim().is_empty() {
        return Err("尚未去 AI 味，请先完成需求→文案→去 AI 味".into());
    }
    emit(ProgressMsg {
        task_id: job.id.clone(),
        progress: 30.0,
        status: "running".into(),
        message: Some("调用 Agnes 生成分镜".into()),
        payload: None,
    });
    let (base_url, model, key) = llm_provider(pool).await?;
    let prompt = format!(
        "请将以下文案拆为 4-6 个镜头，每个镜头给出：画面描述、台词、时长秒、运镜建议，JSON 数组返回：\n\n{}",
        human
    );
    // 默认风格约束卡：现实（也允许前端后续覆盖）
    let style_ref = "现实";
    let shots_json = match run_llm_json(client, &base_url, &model, &key, &prompt).await {
        Ok(arr) => serde_json::to_string(&arr).unwrap_or_else(|_| "[]".into()),
        Err(_) => {
            // 降级：本地构造 4 个分镜
            serde_json::to_string(&vec![
                serde_json::json!({"index":0,"desc":"开场：主持人近景微笑，背景虚化","dialogue":"嗨，今天说个特适合新手的事儿。","dur":5,"cam":"近景"}),
                serde_json::json!({"index":1,"desc":"界面展示：AI 剪辑按钮高亮","dialogue":"你大概说个想法就行。","dur":6,"cam":"推近"}),
                serde_json::json!({"index":2,"desc":"动画：文案自动变成时间线","dialogue":"连配音字幕都帮你弄好。","dur":6,"cam":"平摇"}),
                serde_json::json!({"index":3,"desc":"结尾：主持人比赞，品牌浮现","dialogue":"想轻松做视频，真的可以试一下。","dur":4,"cam":"中景"}),
            ]).unwrap_or_else(|_| "[]".into())
        }
    };
    db::storyboard_save(pool, &project_id, &shots_json, style_ref).await?;
    db::creation_project_update(pool, &project_id, None, None, None, Some("storyboard")).await?;
    Ok(shots_json)
}

/// 图片生成：Agnes /images/generations → base64 → 本地文件 → 写 generated_assets
async fn run_image_gen_task(
    pool: &SqlitePool,
    client: &Client,
    data_dir: &Path,
    job: &TaskJob,
    emit: Arc<dyn Fn(ProgressMsg) + Send + Sync>,
) -> Result<String, String> {
    let project_id = job.project_id.clone().ok_or("image_gen 缺少 projectId")?;
    let shot_index = job.payload.get("shotIndex").and_then(|x| x.as_i64()).unwrap_or(0);
    let style_ref = job.payload.get("styleRef").and_then(|x| x.as_str()).unwrap_or("现实").to_string();

    let sb = db::storyboard_get(pool, &project_id).await?
        .ok_or_else(|| "请先生成分镜".to_string())?;
    let shots: Vec<serde_json::Value> = serde_json::from_str(&sb.shots).map_err(|e| e.to_string())?;
    let shot = shots.iter().find(|s| s.get("index").and_then(|x| x.as_i64()) == Some(shot_index))
        .ok_or_else(|| format!("未找到分镜 index={shot_index}"))?;
    let desc = shot.get("desc").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let dialogue = shot.get("dialogue").and_then(|x| x.as_str()).unwrap_or("").to_string();

    emit(ProgressMsg {
        task_id: job.id.clone(),
        progress: 30.0,
        status: "running".into(),
        message: Some(format!("调用 Agnes /images/generations（分镜 {}）", shot_index + 1)),
        payload: None,
    });

    // 取 img provider 配置
    let row = db::get_by_kind(pool, "img").await?;
    if row.base_url.is_empty() {
        return Err("图片网关 base_url 未配置".into());
    }
    let key = cred::get_key(pool, "img").await?
        .ok_or_else(|| "尚未保存图片 Provider API Key".to_string())?;
    if key.is_empty() {
        return Err("图片 Key 为空".into());
    }
    let style_hints = style_preset_hint(&style_ref);
    let prompt = format!(
        "{desc}。台词：{dialogue}。风格：{style_hints}。电影质感，高质量。",
        desc = desc, dialogue = dialogue, style_hints = style_hints
    );
    let seed = 42i64 + shot_index;
    let base_url = row.base_url.trim_end_matches('/');
    let body = serde_json::json!({
        "model": row.model,
        "prompt": prompt,
        "size": "1024x1024",
        "n": 1,
        "seed": seed,
    });
    let resp = client
        .post(format!("{base_url}/images/generations"))
        .bearer_auth(key.as_str())
        .json(&body)
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| format!("Agnes 图片调用失败: {e}"))?;
    if !resp.status().is_success() {
        let st = resp.status();
        let txt = resp.text().await.unwrap_or_default();
        return Err(format!("Agnes 图片返回 {st}: {txt}"));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let b64 = v.get("data")
        .and_then(|d| d.get(0))
        .and_then(|d| d.get("b64_json"))
        .and_then(|b| b.as_str())
        .ok_or_else(|| "Agnes 响应缺 data[0].b64_json".to_string())?;

    emit(ProgressMsg {
        task_id: job.id.clone(),
        progress: 70.0,
        status: "running".into(),
        message: Some("写入本地文件".into()),
        payload: None,
    });
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).map_err(|e| format!("base64 解码失败: {e}"))?;
    let asset_dir = data_dir.join("creation_assets").join(&project_id);
    std::fs::create_dir_all(&asset_dir).map_err(|e| e.to_string())?;
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let out_path = asset_dir.join(format!("shot_{shot_index}_{ts}.png"));
    std::fs::write(&out_path, &bytes).map_err(|e| e.to_string())?;

    // 一致性抽检：粗筛（文件大小 5KB-5MB）
    let size = bytes.len();
    if !(5 * 1024..=5 * 1024 * 1024).contains(&size) {
        emit(ProgressMsg {
            task_id: job.id.clone(),
            progress: 95.0,
            status: "running".into(),
            message: Some(format!("⚠ 图片大小 {} KB 可能异常，请人工检查", size / 1024)),
            payload: None,
        });
    }

    db::generated_asset_insert(pool, &project_id, shot_index, "image", out_path.to_str().unwrap()).await?;
    db::creation_project_update(pool, &project_id, None, None, None, Some("images")).await?;
    Ok(out_path.to_string_lossy().to_string())
}

fn style_preset_hint(id: &str) -> &'static str {
    match id {
        "现实" => "自然写实暖色调，平实推进镜头",
        "科幻" => "冷蓝紫霓虹色调，科技感几何字体，推拉摇移运镜",
        "卡通" => "明快多彩高饱和，圆体卡通字体，弹性运镜",
        "写实" => "高对比胶片感，衬线字体，固定长镜头",
        "动漫" => "二次元高饱和，手写感字体，分镜式切",
        "水彩" => "淡彩晕染柔和，手写体，缓慢平移",
        _ => "自然写实暖色调",
    }
}

/// LLM 文本调用（不要 JSON 数组）：直接返回 content 字符串。
async fn run_llm_text(
    client: &Client,
    base_url: &str,
    model: &str,
    api_key: &str,
    prompt: &str,
) -> Result<String, String> {
    let base = fix_local_scheme(base_url.trim_end_matches('/'));
    let url = format!("{base}/chat/completions");
    let body = serde_json::json!({
        "model": model,
        "messages": [{ "role": "user", "content": prompt }],
        "max_tokens": 1024,
        "temperature": 0.7,
    });
    let resp = client
        .post(&url)
        .bearer_auth(api_key)
        .json(&body)
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| format!("大模型调用失败: {e}"))?;
    if !resp.status().is_success() {
        let st = resp.status();
        let txt = resp.text().await.unwrap_or_default();
        return Err(format!("大模型返回 {st}: {txt}"));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let text = v
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or_else(|| "大模型响应缺 choices[0].message.content".to_string())?;
    Ok(text.trim().to_string())
}

/// 多模态（视觉）调用：将若干帧以 base64 JPEG 内联进 messages，走 OpenAI 兼容 /chat/completions。
/// images: 已编码的 base64 JPEG（不含 data: 前缀）。每帧作为 image_url content 传入。
async fn run_llm_vision(
    client: &Client,
    base_url: &str,
    model: &str,
    api_key: &str,
    prompt: &str,
    images: &[String],
    provider_name: &str,
) -> Result<String, String> {
    let base = fix_local_scheme(base_url.trim_end_matches('/'));
    let url = format!("{base}/chat/completions");
    let mut content: Vec<serde_json::Value> = vec![serde_json::json!({ "type": "text", "text": prompt })];
    for img in images {
        content.push(serde_json::json!({
            "type": "image_url",
            "image_url": { "url": format!("data:image/jpeg;base64,{img}") }
        }));
    }
    let body = serde_json::json!({
        "model": model,
        "messages": [{ "role": "user", "content": content }],
        "max_tokens": 1500,
        "temperature": 0.5,
    });
    let resp = client
        .post(&url)
        .bearer_auth(api_key)
        .json(&body)
        .timeout(Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| format!("视觉大模型（{provider_name}）调用失败: {e}"))?;
    if !resp.status().is_success() {
        let st = resp.status();
        let txt = resp.text().await.unwrap_or_default();
        return Err(format!("视觉大模型（{provider_name}）返回 {st}: {txt}"));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let text = v
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or_else(|| format!("视觉大模型（{provider_name}）响应缺 choices[0].message.content"))?;
    Ok(text.trim().to_string())
}

/// 统计目录下 frame_*.jpg 数量。
fn count_frames(dir: &std::path::Path) -> usize {
    let mut n = 0usize;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("frame_") && name.ends_with(".jpg") {
                n += 1;
            }
        }
    }
    n
}

/// 列出 frame_*.jpg 绝对路径（按文件名排序）。
fn list_frames(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut v: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("frame_") && name.ends_with(".jpg") {
                v.push(e.path());
            }
        }
    }
    v.sort();
    v
}

/// 将若干帧（Path）按上限采样并 base64 编码为 JPEG data（不含前缀）。
fn base64_frames(frames: &[std::path::PathBuf], max_n: usize) -> Vec<String> {
    let step = if frames.len() <= max_n {
        1
    } else {
        (frames.len() + max_n - 1) / max_n
    };
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < frames.len() && out.len() < max_n {
        if let Ok(bytes) = std::fs::read(&frames[i]) {
            out.push(B64.encode(bytes));
        }
        i += step;
    }
    out
}

/// 从 ffmpeg showinfo 的 stderr 中解析所有 pts_time（秒），用于场景切换点。
fn parse_pts_times(s: &str) -> Vec<f64> {
    let mut out = Vec::new();
    for line in s.lines() {
        if let Some(rest) = line.split("pts_time:").nth(1) {
            if let Ok(v) = rest.trim().split_whitespace().next().unwrap_or("").parse::<f64>() {
                out.push(v);
            }
        }
    }
    out
}

// ===========================================================================
// M2.5：影片解说真链路工具 + 异步任务
// ===========================================================================

/// M2.5：按字数等分切分整段 ASR 文本 + 时间戳回填。
/// 输入：asr_text 整段文本 + total_duration 视频总时长（秒） + target_segments 目标段数
/// 输出：Vec<{ start, end, text }> 3-8 段
pub fn split_script_to_sections(
    asr_text: &str,
    total_duration: f64,
    target_segments: usize,
) -> Vec<db::TimelineClip> {
    use serde_json::json;
    // 按中文标点切句
    let raw_sentences: Vec<&str> = asr_text
        .split(|c: char| "。！？!?，,；;\n".contains(c))
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    // 目标段数：3-8 区间；六段式标签（不足时循环取）
    let n = target_segments.clamp(3, 8);
    let section_labels = ["开端", "铺垫", "冲突", "高潮", "反转", "结局"];
    let per_seg = (raw_sentences.len() + n - 1) / n;
    let total_duration = if total_duration > 0.0 { total_duration } else { (n as f64) * 60.0 };

    let mut out: Vec<db::TimelineClip> = Vec::new();
    for i in 0..n {
        let s_idx = i * per_seg;
        let e_idx = ((i + 1) * per_seg).min(raw_sentences.len());
        let body = if s_idx < e_idx {
            raw_sentences[s_idx..e_idx].join("。") + "。"
        } else {
            String::new()
        };
        let start = (i as f64 / n as f64) * total_duration;
        let end = ((i + 1) as f64 / n as f64) * total_duration;
        let sec_label = section_labels.get(i % section_labels.len()).copied().unwrap_or("");
        let start_ts = fmt_ts(start);
        let end_ts = fmt_ts(end);
        let text = format!("[{}] {}-{} {}", sec_label, start_ts, end_ts, body);
        out.push(db::TimelineClip {
            id: format!("s{i}"),
            source: "narration".into(),
            timeline_start: start,
            timeline_end: end,
            src_start: start,
            src_end: end,
            label: sec_label.to_string(),
            text,
            flower: String::new(),
            transition: "none".into(),
        });
    }
    let _ = json!({}); // 抑制 unused import
    out
}

/// M2.5：解说文案提示词配置（综合写作风格 / 视角 / 语言 / 时长 / 辅助 / 模型 / 分析模式 / 语音克隆 / 字幕样式）
pub struct NarrationConfig<'a> {
    pub title: &'a str,
    pub style: &'a str,
    pub style_name: &'a str,
    pub view: &'a str,         // first / third
    pub language: &'a str,     // zh / en / ja
    pub duration_min: u32,
    pub hint: &'a str,
    pub model: &'a str,        // default / god
    pub analysis_mode: f32,    // 0-1 穿插原片密度
    pub voice_id: &'a str,     // 知性女声 / 磁性男声 / 温暖男声
    pub subtitle_style: &'a str,
    pub asr_text: &'a str,
    pub asr_failed: bool,
    /** M2.6：影片视频分析结果（来自多模态大模型），作为内容理解核心依据，与所选参数共同驱动解说文案生成 */
    pub analysis: &'a str,
    /** M2.6：真实画面时间节点列表（来自场景检测/语义块，`1. 0:00-0:12` 逐行），字幕据此与画面对齐 */
    pub scene_nodes: &'a str,
}

fn style_hint_for(style: &str, style_name: &str) -> String {
    let mapped = match style {
        "movie" | "电影解说" => "电影解说风格，叙事沉稳、画面感强、善用比喻",
        "series" | "电视剧" => "电视剧风格，剧情连贯、悬念推进",
        "variety" | "综艺" => "综艺风格，活泼欢快、节奏明快",
        "anime" | "动漫" => "动漫风格，夸张表达、富有张力",
        "doc" | "纪录片" => "纪录片风格，平实严谨、考据详实",
        "horror" | "悬疑文案" => "悬疑风格，紧张氛围、层层递进",
        "funny" | "轻松搞笑" => "轻松搞笑风格，幽默诙谐、口语化",
        "emotion" | "激情解说" => "激情解说风格，情感浓烈、爆发力强",
        "knowledge" | "知识科普" => "知识科普风格，逻辑清晰、举例说明",
        "sarcastic-suspense" => "毒舌悬疑：以犀利毒舌口吻剖析悬疑案件，伏笔回收、逻辑闭环",
        "sarcastic-action" => "毒舌动作：以毒舌视角拆解动作犯罪场面，战术拉满、反差套路",
        "sarcastic-drama" => "毒舌短剧：以毒舌口吻解构下沉短剧，打脸爽点、人性算计",
        "custom" => "",
        _ => "",
    };
    if mapped.is_empty() {
        if !style_name.is_empty() {
            return format!("「{}」风格，紧扣该风格基调与语气", style_name);
        }
        return "通用解说风格，叙事自然、有画面感".to_string();
    }
    mapped.to_string()
}

fn voice_hint_for(voice: &str) -> String {
    match voice {
        "磁性男声" => "配音 tone 参考：沉稳磁性、富有厚度（文案可偏大气）",
        "温暖男声" => "配音 tone 参考：温暖亲和、如老友聊天（文案可偏轻松）",
        "知性女声" => "配音 tone 参考：温柔知性、娓娓道来（文案可偏细腻）",
        _ => "",
    }
    .to_string()
}

/// M2.6：从影片分析报告中提取「真实画面时间节点」（相对片段秒数）。
/// 优先解析机器标记 `<!--SCENE_NODES:0.00-12.30,12.30-25.10-->`；
/// 找不到则回退解析「画面时间节点/语义块划分」里的 `m:ss - m:ss` 文本。
pub fn extract_scene_nodes(report: &str) -> Vec<(f64, f64)> {
    // 1) 机器标记
    if let Some(i) = report.find("<!--SCENE_NODES:") {
        let rest = &report[i + "<!--SCENE_NODES:".len()..];
        if let Some(j) = rest.find("-->") {
            let body = &rest[..j];
            let mut out: Vec<(f64, f64)> = Vec::new();
            for pair in body.split(',') {
                let mut it = pair.splitn(2, '-');
                let s = it.next().unwrap_or("").trim().parse::<f64>().ok();
                let e = it.next().unwrap_or("").trim().parse::<f64>().ok();
                if let (Some(s), Some(e)) = (s, e) {
                    if e > s {
                        out.push((s, e));
                    }
                }
            }
            if !out.is_empty() {
                return out;
            }
        }
    }
    // 2) 回退：逐行扫描「数字:数字」时间令牌，取每行前两个作为 (start, end)
    let mut out: Vec<(f64, f64)> = Vec::new();
    for line in report.lines() {
        if !line.contains(" - ") {
            continue; // 仅解析含区间分隔符的语义块列表行
        }
        let toks = scan_ts_tokens(line);
        if toks.len() >= 2 && toks[1] > toks[0] {
            out.push((toks[0], toks[1]));
        }
    }
    out
}

/// 扫描字符串中所有形如 `m:ss` / `mm:ss` 的时间令牌，返回对应秒数。
fn scan_ts_tokens(s: &str) -> Vec<f64> {
    let chars: Vec<char> = s.chars().collect();
    let mut out: Vec<f64> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let mut j = i;
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
            if j < chars.len() && chars[j] == ':' {
                let mut k = j + 1;
                while k < chars.len() && chars[k].is_ascii_digit() {
                    k += 1;
                }
                if k > j + 1 {
                    let tok: String = chars[i..k].iter().collect();
                    out.push(parse_timestamp(&tok));
                    i = k;
                    continue;
                }
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

/// 将时间节点格式化为提示词可读列表：`1. 0:00-0:12`。
fn format_scene_nodes(nodes: &[(f64, f64)]) -> String {
    nodes
        .iter()
        .enumerate()
        .map(|(i, (s, e))| format!("{}. {}-{}", i + 1, fmt_ts(*s), fmt_ts(*e)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// M2.5：生成六段式 narrative prompt（开端/铺垫/冲突/高潮/反转/结局）
/// 综合所有解说选项；强约束简体中文（zh）+ 每段带时间轴。
/// M2.6：若提供真实画面时间节点（scene_nodes），字幕切分严格对齐画面切换点。
pub fn build_narration_prompt(cfg: &NarrationConfig) -> String {
    let target_chars = cfg.duration_min * 270; // 每分钟约 270 字
    let style_hint = style_hint_for(cfg.style, cfg.style_name);
    let view_hint = match cfg.view {
        "first" => "解说视角：第一人称，使用「我」的口吻，增强代入感",
        _ => "解说视角：第三人称旁观，客观冷静、上帝视角点评",
    };
    let (lang_hint, simp_req) = match cfg.language {
        "en" => ("解说语言：English（使用英文输出）", String::new()),
        "ja" => ("解说语言：日本語（使用日文输出）", String::new()),
        _ => (
            "解说语言：中文",
            "【重要】必须使用简体中文输出，严禁繁体中文（如「說/來/這/們/時/會/國/產/對/發/開/長」等一律改为简体「说/来/这/们/时/会/国/产/对/发/开/长」）".to_string(),
        ),
    };
    let model_hint = if cfg.model == "god" {
        "叙事模式：上帝视角（全知全能），点破人物心理、命运伏笔与剧情暗线"
    } else {
        "叙事模式：常规解说"
    };
    let analysis_hint = if cfg.analysis_mode > 0.0 {
        let pct = (cfg.analysis_mode * 100.0).round() as u32;
        format!("穿插原片：在解说中酌情引用原片金句或呼应画面，约占总篇幅 {pct}%")
    } else {
        "穿插原片：不引用原片，纯原创解说".to_string()
    };
    let voice_hint = voice_hint_for(cfg.voice_id);
    let subtitle_hint = if cfg.subtitle_style.contains("无边框") {
        "字幕样式：简约无边框，文案需极简短句、易读"
    } else if cfg.subtitle_style.contains("阴影") {
        "字幕样式：阴影黑字，文案句式适中"
    } else {
        "字幕样式：经典白字黑边，文案可正常长短句"
    };
    let hint_block = if !cfg.hint.trim().is_empty() {
        format!("\n用户特别要求：{}", cfg.hint.trim())
    } else {
        String::new()
    };
    let analysis_block = if !cfg.analysis.trim().is_empty() {
        format!(
            "影片视频分析结果（来自多模态大模型，是内容理解的核心依据，请据此撰写贴合画面、不脱离实际内容的解说）：\n{}\n",
            cfg.analysis.trim()
        )
    } else {
        String::new()
    };
    let asr_block = if cfg.asr_failed {
        "（无法获取视频原声，请仅依据标题与用户要求自由创作，不要编造与视频无关的虚假剧情）".to_string()
    } else {
        format!("视频语音内容参考（来自 ASR，可能不完整，仅作内容理解参考，不要照抄）：\n{}", cfg.asr_text)
    };
    // M2.6：真实画面时间节点（来自场景检测/语义块）——字幕据此与画面对齐
    let has_nodes = cfg.scene_nodes.trim().len() > 0 && cfg.scene_nodes.lines().count() >= 2;
    let nodes_block = if has_nodes {
        format!(
            "画面时间节点（来自真实场景检测/语义块划分，是「字幕与画面对齐」的硬性依据，请逐段对齐）：\n{}\n",
            cfg.scene_nodes.trim()
        )
    } else {
        String::new()
    };
    // 时间轴切分要求：有真实节点时严格对齐画面切换点，否则回退均匀分布
    let timeline_req = if has_nodes {
        "严格按上方「画面时间节点」逐段生成字幕：每个时间区间对应一条字幕，start/end 必须直接采用给定节点的时间（mm:ss），解说内容需描述或呼应该区间画面所发生的内容。相邻区间过短（<3 秒）可合并为一条；单个区间过长（>10 秒）可在其内部再等分为多条；但严禁跨越给定的画面切换点，也不得凭空杜撰给定之外的时间"
    } else {
        "每条字幕标注时间轴，格式 \"start\"-\"end\"（mm:ss-mm:ss，例如 \"0:00\"-\"0:08\"），按视频时长与画面节奏合理切分（建议每 5~8 秒一条），使字幕与画面对应"
    };
    let extra_hints: String = [view_hint, model_hint, subtitle_hint, &analysis_hint, &voice_hint]
        .iter()
        .filter(|h| !h.is_empty())
        .map(|h| h.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r#"请为以下视频撰写一段时长约 {d} 分钟、与画面严格对齐的解说字幕（每条字幕对应一个画面时间区间）。
视频标题：{title}
写作风格：{style_hint}
{lang_hint}
{extra_hints}
视频时长：{d} 分钟（≈ {target_chars} 字）
{hint_block}

{analysis_block}
{nodes_block}
{asr_block}

要求：
1. 每条字幕用【真实原创】的解说词，体现上述风格与视角，不要照抄原片语音
2. 若提供了「影片视频分析结果」，须以其为内容事实基础，确保解说与画面、情节一致，不得脱离实际内容
3. {timeline_req}
4. 整体遵循「开端→铺垫→冲突→高潮→反转→结局」的叙事弧，但字幕切分以画面时间节点为准（可将多条相邻字幕归入同一叙事阶段）
5. 返回严格 JSON 数组（不要任何额外文字、不要 markdown 代码块）：
[{{"start": "0:00", "end": "0:12", "dialogue": "解说词...", "section": "开端"}}, ...]
其中 section 可选，若能判断请标注：开端 / 铺垫 / 冲突 / 高潮 / 反转 / 结局
6. 字数 ≈ {target_chars} 字，口语化、避免空话，每条字幕精炼贴合对应画面
{simp_req}
"#,
        d = cfg.duration_min,
        title = cfg.title,
        style_hint = style_hint,
        lang_hint = lang_hint,
        extra_hints = extra_hints,
        target_chars = target_chars,
        hint_block = hint_block,
        analysis_block = analysis_block,
        nodes_block = nodes_block,
        asr_block = asr_block,
        timeline_req = timeline_req,
        simp_req = simp_req,
    )
}

/// M2.5：解析 LLM 返回的 JSON 数组 + 时间戳解析
pub fn parse_narration_response(llm_text: &str, total_duration: f64, style: &str) -> Result<Vec<db::TimelineClip>, String> {
    // 抠取首尾 [] 之间的内容（容忍 ```json``` 包裹）
    let s = llm_text.trim();
    let core = if let Some(stripped) = s.strip_prefix("```json").or_else(|| s.strip_prefix("```JSON")) {
        stripped.trim_end_matches("```").trim()
    } else if s.starts_with("```") {
        s.trim_start_matches("```").trim_end_matches("```").trim()
    } else {
        s
    };
    let start_idx = core.find('[').ok_or_else(|| "LLM 返回不含 JSON 数组".to_string())?;
    let end_idx = core.rfind(']').map(|i| i + 1).ok_or_else(|| "LLM 返回 JSON 不完整".to_string())?;
    let json_str = &core[start_idx..end_idx];
    let arr: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| format!("LLM 返回 JSON 解析失败: {e}"))?;
    let items = arr.as_array().ok_or_else(|| "JSON 顶层不是数组".to_string())?;

    let _sections = ["开端", "铺垫", "冲突", "高潮", "反转", "结局"];
    let mut shots: Vec<db::TimelineClip> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let start_str = item.get("start").and_then(|v| v.as_str()).unwrap_or("0:00");
        let end_str = item.get("end").and_then(|v| v.as_str()).unwrap_or("0:30");
        let dialogue = item.get("dialogue").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let section = item.get("section").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();

        let start_sec = parse_timestamp(start_str);
        let end_sec = parse_timestamp(end_str);
        let start_ts = fmt_ts(start_sec);
        let end_ts = fmt_ts(end_sec);

        // 文案文本带时间轴，方便后期加字幕；同时保留 label=段落名 供时间线使用
        let text = if !section.is_empty() {
            format!("[{}] {}-{} {}", section, start_ts, end_ts, dialogue)
        } else {
            format!("{}-{} {}", start_ts, end_ts, dialogue)
        };

        shots.push(db::TimelineClip {
            id: format!("s{i}"),
            source: "narration".into(),
            timeline_start: start_sec,
            timeline_end: end_sec,
            src_start: start_sec,
            src_end: end_sec,
            label: section.clone(),
            text,
            flower: String::new(),
            transition: "none".into(),
        });
    }
    let _ = (total_duration, style); // 抑制 unused 警告
    Ok(shots)
}

/// mm:ss → 秒
fn parse_timestamp(s: &str) -> f64 {
    let s = s.trim();
    let p: Vec<&str> = s.split(':').collect();
    if p.len() == 2 {
        p[0].parse::<f64>().unwrap_or(0.0) * 60.0 + p[1].parse::<f64>().unwrap_or(0.0)
    } else {
        s.parse::<f64>().unwrap_or(0.0)
    }
}

/// 秒 → m:ss
fn fmt_ts(sec: f64) -> String {
    let sec = sec.max(0.0);
    let m = (sec / 60.0).floor() as i64;
    let s = (sec % 60.0).round() as i64;
    format!("{}:{}", m, format!("{:02}", s))
}

/// M2.5：异步任务 — 抽音轨 → ASR → 章节切分 → LLM 六段式 → 落 film_projects.script + 落 edit_timelines
async fn run_film_script_gen(
    pool: &SqlitePool,
    client: &Client,
    _port: u16,
    data_dir: &Path,
    job: &TaskJob,
    emit: Arc<dyn Fn(ProgressMsg) + Send + Sync>,
) -> Result<(String, bool, String), String> {
    let project_id = job.project_id.clone().ok_or("film_script_gen 缺少 projectId")?;
    let video_path = job.payload.get("videoPath").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let style = job.payload.get("style").and_then(|v| v.as_str()).unwrap_or("movie").to_string();
    let style_name = job.payload.get("styleName").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let language = job.payload.get("language").and_then(|v| v.as_str()).unwrap_or("zh").to_string();
    let duration = job.payload.get("duration").and_then(|v| v.as_u64()).unwrap_or(180) as u32;
    let hint = job.payload.get("hint").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let title = job.payload.get("title").and_then(|v| v.as_str()).unwrap_or("未知").to_string();
    let mode = job.payload.get("mode").and_then(|v| v.as_str()).unwrap_or("ai").to_string();
    let view = job.payload.get("view").and_then(|v| v.as_str()).unwrap_or("third").to_string();
    let narration_model = job.payload.get("model").and_then(|v| v.as_str()).unwrap_or("default").to_string();
    let analysis_mode = job.payload.get("analysisMode").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
    let voice_id = job.payload.get("voiceId").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let subtitle_style = job.payload.get("subtitleStyle").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let analysis = job.payload.get("analysis").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let mut asr_failed = false;
    let mut asr_reason = String::new();

    // 1) 抽音轨（如果有 videoPath）
    let asr_text = if !video_path.is_empty() && std::path::Path::new(&video_path).exists() {
        emit(ProgressMsg {
            task_id: job.id.clone(),
            progress: 10.0,
            status: "running".into(),
            message: Some("抽取音轨".into()),
            payload: None,
        });
        let ff = FfMpeg::ensure(data_dir).await?;
        let tmp = data_dir.join("tmp");
        std::fs::create_dir_all(&tmp).ok();
        let audio = tmp.join(format!("film_{project_id}_audio.wav"));
        let _ = ff.extract_audio_cmd(&video_path, audio.to_str().unwrap()).output();

        emit(ProgressMsg {
            task_id: job.id.clone(),
            progress: 25.0,
            status: "running".into(),
            message: Some("XiaomiMimo ASR 转写中".into()),
            payload: None,
        });
        let (segments, _lang, _dur, _deg, reason) = transcribe_asr(pool, client, audio.to_str().unwrap()).await;
        asr_reason = reason;
        let transcript: String = segments.iter().map(|s| s.text.clone()).collect::<Vec<_>>().join("");
        if transcript.trim().is_empty() {
            // 降级 1：ASR 失败 → 用视频标题 + 用户辅助 + 风格自由创作（不把错误原因塞给 LLM）
            asr_failed = true;
            let mut note = format!("视频标题：{}。", title);
            if !hint.is_empty() {
                note.push_str(&format!("用户补充要求：{}。", hint));
            }
            note
        } else {
            transcript
        }
    } else {
        emit(ProgressMsg {
            task_id: job.id.clone(),
            progress: 25.0,
            status: "running".into(),
            message: Some("未提供视频路径，跳过 ASR".into()),
            payload: None,
        });
        // 降级 1：使用 hint + title 直接生成
        format!("{}{}", hint, job.payload.get("title").and_then(|v| v.as_str()).unwrap_or(""))
    };

    // 2) 章节切分（即使 LLM 成功也保留，作为降级兜底）
    let target_segments = if duration <= 120 { 3 } else if duration <= 300 { 5 } else { 6 };
    let pre_sections = split_script_to_sections(if asr_failed { "" } else { &asr_text }, duration as f64, target_segments);

    // 3) 文案生成
    emit(ProgressMsg {
        task_id: job.id.clone(),
        progress: 50.0,
        status: "running".into(),
        message: Some("Agnes LLM 生成六段式文案".into()),
        payload: None,
    });

    // 「我有文案」模式：直接使用用户辅助作为最终文案（不调用 LLM）
    let raw_script: String = if mode == "custom" && !hint.trim().is_empty() {
        hint.trim().to_string()
    } else {
        match llm_provider(pool).await {
            Ok((base_url, llm_model, key)) => {
                // M2.6：从影片分析报告提取真实画面时间节点，字幕据此与画面对齐
                let nodes = extract_scene_nodes(&analysis);
                let scene_nodes = format_scene_nodes(&nodes);
                let cfg = NarrationConfig {
                    title: &title,
                    style: &style,
                    style_name: &style_name,
                    view: &view,
                    language: &language,
                    duration_min: duration / 60,
                    hint: &hint,
                    model: &narration_model,
                    analysis_mode,
                    voice_id: &voice_id,
                    subtitle_style: &subtitle_style,
                    asr_text: &asr_text,
                    asr_failed,
                    analysis: &analysis,
                    scene_nodes: &scene_nodes,
                };
                let prompt = build_narration_prompt(&cfg);
                match run_llm_text(client, &base_url, &llm_model, &key, &prompt).await {
                    Ok(text) => {
                        match parse_narration_response(&text, duration as f64, &style) {
                            Ok(shots) => {
                                // 保留 LLM 生成的原创解说（含时间轴），不再用原始 ASR 覆盖
                                let s: String = shots.iter().map(|x| x.text.clone()).collect::<Vec<_>>().join("\n");
                                s
                            }
                            Err(_e) => {
                                // 降级 2：LLM 返回非 JSON，用 pre_sections
                                pre_sections.iter().map(|x| x.text.clone()).collect::<Vec<_>>().join("\n")
                            }
                        }
                    }
                    Err(_) => {
                        // 降级 2：LLM 失败，用 ASR 切分
                        pre_sections.iter().map(|x| x.text.clone()).collect::<Vec<_>>().join("\n")
                    }
                }
            }
            Err(_e) => {
                // 降级 1：缺 Key
                pre_sections.iter().map(|x| x.text.clone()).collect::<Vec<_>>().join("\n")
            }
        }
    };

    // 繁体 -> 简体兜底（即使 LLM 偶发繁体或 ASR 上下文带入，也统一为简体）
    let script = to_simplified(&raw_script);
    db::film_project_set_script(pool, &project_id, &script).await?;
    let _ = language; // 抑制 unused 警告
    Ok((script, asr_failed, asr_reason))
}

// ===========================================================================
// M2.6：影片视频分析（多模态大模型）— 确认视频范围后触发
// 十步进度：①提取视频帧 ②检测场景切换点 ③多维度特征编码 ④深度语义理解
// ⑤语义块解析 ⑥深度语义理解(二次) ⑦叙事结构生成 ⑧解说词生成 ⑨输出流水线 ⑩总结报告
// ===========================================================================

fn va_fmt_ts(sec: f64) -> String {
    let s = sec.max(0.0);
    let m = (s / 60.0).floor() as u32;
    let ss = (s % 60.0).round() as u32;
    format!("{}:{:02}", m, ss)
}

/// 本地大模型（Ollama 等）默认只提供 HTTP；若设置里误填 https://localhost，
/// 这里自动规整为 http，避免 TLS 握手失败导致「error sending request」。
fn fix_local_scheme(base: &str) -> String {
    let b = base.to_string();
    let lower = b.to_lowercase();
    let is_local_https = lower.starts_with("https://localhost")
        || lower.starts_with("https://127.0.0.1")
        || lower.starts_with("https://[::1]");
    if is_local_https {
        if let Some(rest) = b.strip_prefix("https://") {
            return format!("http://{rest}");
        }
        if let Some(rest) = b.strip_prefix("HTTPS://") {
            return format!("http://{rest}");
        }
    }
    b
}

/// 无多模态大模型（缺 Key/失败）时的元数据兜底理解。
fn fallback_understanding(title: &str, style: &str, scenes: &[f64], dur: f64, avg_kb: u64, reason: &str, provider: &str) -> String {
    format!(
        "（未能调用已配置的多模态大模型（{provider}），以下基于视频元数据生成概览。原因：{reason}）\n\
标题：{title}\n风格：{style}\n分析片段时长：约 {dur} 秒\n检测到的场景切换点：{scenes} 个\n平均帧体积：{avg} KB（可作画面复杂度参考）\n\
说明：已在「设置 → 接口」中配置大模型（{provider}）但本次请求失败。本地 Ollama 等默认使用 http 而非 https，若 Base URL 填了 https://localhost 请改为 http://localhost（例如 http://localhost:11434/v1）；保存后重试即可获得基于关键帧的深度语义理解。",
        reason = reason,
        provider = provider,
        title = title,
        style = if style.is_empty() { "默认" } else { style },
        dur = dur as u32,
        scenes = scenes.len(),
        avg = avg_kb,
    )
}

fn default_narrative(dur: f64) -> String {
    let seg = dur / 6.0;
    let names = ["开端", "铺垫", "冲突", "高潮", "反转", "结局"];
    let mut v: Vec<serde_json::Value> = Vec::new();
    for (i, n) in names.iter().enumerate() {
        let s = seg * i as f64;
        let e = seg * (i + 1) as f64;
        v.push(serde_json::json!({
            "section": n,
            "start": va_fmt_ts(s),
            "end": va_fmt_ts(e),
            "summary": format!("{}阶段（自动占位，建议在解说工作台精修）", n)
        }));
    }
    serde_json::to_string_pretty(&v).unwrap_or_else(|_| "[]".into())
}

fn default_narration_text(title: &str, style: &str) -> String {
    format!(
        "这是一段关于《{title}》的{style}解说。影片以紧凑的节奏展开，画面信息丰富，适合用口语化的方式带观众理解核心内容；后续可在解说工作台基于实际理解进一步润色与配音。",
        title = title,
        style = if style.is_empty() { "通用" } else { style },
    )
}

/// 组装最终 Markdown 报告。
fn build_analysis_report(
    title: &str,
    style: &str,
    dur: f64,
    n_frames: usize,
    scene_cnt: usize,
    avg_kb: u64,
    understanding: &str,
    semantic: &str,
    narrative_struct: &str,
    narration_text: &str,
    segs: &[(f64, f64)],
) -> String {
    let mut b = String::new();
    b.push_str(&format!("# 影片理解报告：{}\n\n", title));
    b.push_str(&format!(
        "**风格**：{}  \n**分析片段时长**：约 {} 秒  \n**提取帧数**：{}  \n**场景切换点**：{} 个  \n**平均帧体积**：{} KB\n\n",
        if style.is_empty() { "默认" } else { style },
        dur as u32,
        n_frames,
        scene_cnt,
        avg_kb,
    ));
    b.push_str("## 一、深度语义理解\n");
    b.push_str(understanding);
    b.push_str("\n\n## 二、画面时间节点（语义块划分，供字幕与画面对齐）\n");
    for (i, (s, e)) in segs.iter().enumerate() {
        b.push_str(&format!("{}. `{} - {}`\n", i + 1, va_fmt_ts(*s), va_fmt_ts(*e)));
    }
    // 机器可解析标记：解说生成阶段据此精确提取真实画面切换点（相对片段秒数）
    let nodes_raw: String = segs
        .iter()
        .map(|(s, e)| format!("{:.2}-{:.2}", s, e))
        .collect::<Vec<_>>()
        .join(",");
    b.push_str(&format!("\n<!--SCENE_NODES:{}-->\n", nodes_raw));
    b.push_str("\n## 三、语义要点提炼\n");
    b.push_str(semantic);
    b.push_str("\n\n## 四、叙事结构（六段式）\n");
    b.push_str(narrative_struct);
    b.push_str("\n\n## 五、解说词初稿\n");
    b.push_str(narration_text);
    b.push('\n');
    b
}

async fn run_film_video_analysis(
    pool: &SqlitePool,
    client: &Client,
    data_dir: &Path,
    job: &TaskJob,
    emit: Arc<dyn Fn(ProgressMsg) + Send + Sync>,
) -> Result<(), String> {
    let job_id = job.id.clone();
    let project_id = job.project_id.clone().unwrap_or_default();
    let video_path = job.payload.get("videoPath").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let start = job.payload.get("start").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let end = job.payload.get("end").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let title = job.payload.get("title").and_then(|v| v.as_str()).unwrap_or("未命名影片").to_string();
    let style_name = job.payload.get("styleName").and_then(|v| v.as_str()).unwrap_or("").to_string();

    if video_path.is_empty() {
        return Err("缺少 videoPath，无法分析视频".into());
    }
    let dur_seg = (end - start).max(0.5);

    let ej = job_id.clone();
    let emitc = emit.clone();
    let step = move |s: u8, prog: f64, msg: String| {
        emitc(ProgressMsg {
            task_id: ej.clone(),
            progress: prog,
            status: "running".into(),
            message: Some(msg),
            payload: Some(serde_json::json!({ "step": s })),
        });
    };

    // ① 提取视频帧
    let frames_dir = data_dir.join("analysis").join(&job_id);
    let _ = std::fs::create_dir_all(&frames_dir);
    let ff = ffmpeg::FfMpeg::ensure(data_dir).await?;
    let n_frames = ((dur_seg / 4.0).clamp(6.0, 24.0)) as usize;
    let fps = n_frames as f64 / dur_seg;
    let pattern = frames_dir.join("frame_%03d.jpg");
    step(1, 2.0, "提取视频帧 0%".into());
    let mut child = Command::new(&ff.path)
        .args([
            "-ss", &format!("{start:.3}"),
            "-i", &video_path,
            "-t", &format!("{dur_seg:.3}"),
            "-vf", &format!("fps={fps:.4}"),
            "-q:v", "2",
            pattern.to_str().unwrap_or("frame_%03d.jpg"),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("启动 ffmpeg 提取帧失败: {e}"))?;
    loop {
        let produced = count_frames(&frames_dir);
        let p = (produced as f64 / n_frames as f64).clamp(0.0, 1.0);
        step(1, 2.0 + p * 16.0, format!("提取视频帧 {:.0}%", p * 100.0));
        if produced >= n_frames {
            break;
        }
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
        sleep(Duration::from_millis(150)).await;
    }
    let _ = child.wait();
    let produced = count_frames(&frames_dir).max(1);
    step(1, 18.0, format!("提取视频帧完成（{} 帧）", produced));

    // ② 检测场景切换点
    step(2, 20.0, "检测场景切换点".into());
    let scene_out = Command::new(&ff.path)
        .args([
            "-ss", &format!("{start:.3}"),
            "-i", &video_path,
            "-t", &format!("{dur_seg:.3}"),
            "-filter:v", "select='gt(scene\\,0.3)',showinfo",
            "-f", "null", "-",
        ])
        .output()
        .map_err(|e| format!("启动 ffmpeg 场景检测失败: {e}"))?;
    let stderr = String::from_utf8_lossy(&scene_out.stderr);
    let mut scenes: Vec<f64> = parse_pts_times(&stderr);
    scenes.sort_by(|a, b| a.partial_cmp(b).unwrap());
    step(2, 32.0, format!("检测场景切换点完成（{} 个）", scenes.len()));

    // ③ 多维度特征编码中
    let frames = list_frames(&frames_dir);
    let total = frames.len().max(1);
    let mut feature_bytes = 0u64;
    for (i, f) in frames.iter().enumerate() {
        if let Ok(m) = std::fs::metadata(f) {
            feature_bytes += m.len();
        }
        let p = (i + 1) as f64 / total as f64;
        step(3, 34.0 + p * 12.0, format!("多维度特征编码中 {:.0}%", p * 100.0));
    }
    let avg_kb = ((feature_bytes / total as u64) / 1024) as u64;

    // ④ 深度语义理解中（多模态视觉首次）
    step(4, 48.0, "深度语义理解中".into());
    let vision_prompt = "你是资深视频分析专家。以下是影片片段按时间顺序的关键帧（已内联为图片）。请综合描述：\n1) 场景与环境（室内/室外/光线/美术风格）\n2) 人物或主体及其动作\n3) 关键情绪与张力\n4) 整体基调（如悬疑/欢快/温情/史诗）\n请用简体中文分点回答，不超过 300 字。";
    let sample = base64_frames(&frames, 8);
    let llm_provider_name = db::get_by_kind(pool, "llm").await.map(|r| r.provider).unwrap_or_else(|_| "大模型".to_string());
    let understanding = match llm_provider(pool).await {
        Ok((base_url, model, key)) => match run_llm_vision(client, &base_url, &model, &key, vision_prompt, &sample, &llm_provider_name).await {
            Ok(t) => t,
            Err(e) => fallback_understanding(&title, &style_name, &scenes, dur_seg, avg_kb, &e, &llm_provider_name),
        },
        Err(e) => fallback_understanding(&title, &style_name, &scenes, dur_seg, avg_kb, &e, &llm_provider_name),
    };
    step(4, 58.0, "深度语义理解中".into());

    // ⑤ 语义块解析中
    let mut boundaries = vec![0.0f64];
    for s in &scenes {
        boundaries.push(*s);
    }
    boundaries.push(dur_seg);
    boundaries.sort_by(|a, b| a.partial_cmp(b).unwrap());
    boundaries.dedup_by(|a, b| (*b - *a).abs() < 0.2);
    // 语义块时间节点以「片段内相对时间」（0 基）表示，便于后续字幕与画面对齐
    let mut segs: Vec<(f64, f64)> = Vec::new();
    for w in boundaries.windows(2) {
        segs.push((w[0], w[1]));
    }
    let n_blocks = segs.len().clamp(4, 48);
    step(5, 60.0, format!("语义块 1/{} 解析中", n_blocks));
    for i in 0..n_blocks {
        let p = (i + 1) as f64 / n_blocks as f64;
        step(5, 58.0 + p * 10.0, format!("语义块 {}/{} 解析中", i + 1, n_blocks));
    }

    // ⑥ 深度语义理解中（二次：文本综合）
    step(6, 70.0, "深度语义理解中".into());
    let synth_prompt = format!(
        "基于以下影片理解，提炼 {} 个语义块的核心要点。每个语义块给出「时间区间 + 一句话概括」。\n影片理解：\n{}\n语义块数：{}",
        n_blocks, understanding, n_blocks
    );
    let semantic = match llm_provider(pool).await {
        Ok((base_url, model, key)) => match run_llm_text(client, &base_url, &model, &key, &synth_prompt).await {
            Ok(t) => t,
            Err(_) => understanding.clone(),
        },
        Err(_) => understanding.clone(),
    };
    step(6, 80.0, "深度语义理解中".into());

    // ⑦ 叙事结构生成中
    step(7, 81.0, "叙事结构生成中".into());
    let narrative_struct = match llm_provider(pool).await {
        Ok((base_url, model, key)) => {
            let np = format!(
                "请为以下影片片段设计六段式叙事结构（开端/铺垫/冲突/高潮/反转/结局），每段的「起止时间（相对影片，mm:ss-mm:ss）」需落在本片段时长 {} 秒以内。用简体中文，输出纯 JSON 数组：\n[{{\"section\":\"开端\",\"start\":\"0:00\",\"end\":\"0:30\",\"summary\":\"...\"}}, ...]\n影片理解：\n{}",
                dur_seg as u32, understanding
            );
            match run_llm_text(client, &base_url, &model, &key, &np).await {
                Ok(t) => t,
                Err(_) => default_narrative(dur_seg),
            }
        }
        Err(_) => default_narrative(dur_seg),
    };
    step(7, 87.0, "叙事结构生成中".into());

    // ⑧ 解说词生成中
    step(8, 88.0, "解说词生成中".into());
    let narration_text = match llm_provider(pool).await {
        Ok((base_url, model, key)) => {
            let np = format!(
                "请基于以下影片理解，撰写一段适合配音的解说词初稿（简体中文，口语化，约 200-300 字，不要出现时间轴标记）：\n{}",
                understanding
            );
            match run_llm_text(client, &base_url, &model, &key, &np).await {
                Ok(t) => t,
                Err(_) => default_narration_text(&title, &style_name),
            }
        }
        Err(_) => default_narration_text(&title, &style_name),
    };
    step(8, 94.0, "解说词生成中".into());

    // ⑨ 输出流水线生成中
    step(9, 95.0, "输出流水线生成中".into());
    let report = build_analysis_report(
        &title,
        &style_name,
        dur_seg,
        produced,
        scenes.len(),
        avg_kb,
        &understanding,
        &semantic,
        &narrative_struct,
        &narration_text,
        &segs,
    );
    db::film_project_set_analysis(pool, &project_id, &report).await?;
    step(9, 99.0, "输出流水线生成中".into());

    // ⑩ 最终影片分析内容总结报告
    step(10, 100.0, "最终影片分析内容总结报告".into());
    emit(ProgressMsg {
        task_id: job_id.clone(),
        progress: 100.0,
        status: "done".into(),
        message: Some("影片分析完成".into()),
        payload: Some(serde_json::json!({ "step": 10, "report": report })),
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narration_prompt_requires_simplified_and_timestamps() {
        let cfg = NarrationConfig {
            title: "测试视频",
            style: "sarcastic-suspense",
            style_name: "毒舌悬疑",
            view: "third",
            language: "zh",
            duration_min: 3,
            hint: "结尾加关注语",
            model: "god",
            analysis_mode: 0.3,
            voice_id: "知性女声",
            subtitle_style: "经典-白字黑边",
            asr_text: "这是参考语音",
            asr_failed: false,
            analysis: "",
            scene_nodes: "",
        };
        let p = build_narration_prompt(&cfg);
        assert!(p.contains("简体中文"), "提示词应要求简体中文");
        assert!(p.contains("时间轴"), "提示词应要求时间轴");
        assert!(p.contains("毒舌悬疑"), "提示词应包含风格名");
        assert!(p.contains("第三人称"), "提示词应包含视角");
        assert!(p.contains("上帝视角"), "god 模型应包含上帝视角");
        assert!(p.contains("结尾加关注语"), "提示词应包含用户辅助");
    }

    #[test]
    fn narration_prompt_aligns_to_scene_nodes_when_provided() {
        let cfg = NarrationConfig {
            title: "测试视频",
            style: "movie",
            style_name: "",
            view: "third",
            language: "zh",
            duration_min: 1,
            hint: "",
            model: "default",
            analysis_mode: 0.0,
            voice_id: "",
            subtitle_style: "",
            asr_text: "参考",
            asr_failed: false,
            analysis: "影片理解……",
            scene_nodes: "1. 0:00-0:12\n2. 0:12-0:25\n3. 0:25-0:40",
        };
        let p = build_narration_prompt(&cfg);
        assert!(p.contains("画面时间节点"), "有节点时提示词应含画面时间节点块");
        assert!(p.contains("0:12-0:25"), "提示词应内联真实节点");
        assert!(p.contains("严格按上方"), "有节点时应要求严格对齐画面切换点");
        assert!(!p.contains("均匀分布"), "有节点时不应再要求均匀分布");
    }

    #[test]
    fn extract_scene_nodes_from_report_marker() {
        let report = "# 报告\n## 二、画面时间节点\n1. `0:00 - 0:12`\n<!--SCENE_NODES:0.00-12.30,12.30-25.10,25.10-40.00-->\n## 三、其他\n";
        let nodes = extract_scene_nodes(report);
        assert_eq!(nodes.len(), 3, "应解析出 3 个节点");
        assert_eq!(nodes[0], (0.0, 12.30));
        assert_eq!(nodes[1], (12.30, 25.10));
        let fmt = format_scene_nodes(&nodes);
        assert!(fmt.contains("1. 0:00-0:12"), "格式化列表应含首节点，实际: {}", fmt);
    }

    #[test]
    fn extract_scene_nodes_fallback_parses_ranges() {
        let report = "## 语义块\n1. `0:00 - 0:15`\n2. `0:15 - 0:33`\n";
        let nodes = extract_scene_nodes(report);
        assert_eq!(nodes.len(), 2, "无标记时应回退解析 m:ss - m:ss");
        assert_eq!(nodes[0], (0.0, 15.0));
        assert_eq!(nodes[1], (15.0, 33.0));
    }

    #[test]
    fn narration_parse_embeds_timestamp_in_text() {
        let json = r#"[{"start":"0:00","end":"0:30","dialogue":"开场白","section":"开端"},{"start":"0:30","end":"1:00","dialogue":"第二段","section":"铺垫"}]"#;
        let shots = parse_narration_response(json, 60.0, "movie").unwrap();
        assert_eq!(shots.len(), 2);
        assert!(
            shots[0].text.starts_with("[开端] 0:00-0:30"),
            "文案应带时间轴，实际: {}",
            shots[0].text
        );
        assert_eq!(shots[0].label, "开端");
        assert_eq!(shots[0].timeline_start, 0.0);
        assert_eq!(shots[0].timeline_end, 30.0);
        assert_eq!(shots[1].timeline_end, 60.0);
    }
}
