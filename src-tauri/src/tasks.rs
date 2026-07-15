// VideosFlow — 任务队列 + 进度广播（tokio mpsc + Tauri Channel）
// M0：worker 消费任务，做 sidecar 健康检查并广播进度，最终持久化状态到 tasks 表。
// M2：新增 film_import / film_smart_cut / film_export 分支（确定性纯算法 + ffmpeg）。

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tokio::sync::mpsc;

use std::path::Path;
use std::sync::{Arc, OnceLock};

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
    let key = key.ok_or_else(|| "尚未保存 LLM（Agnes）API Key，请先在设置页保存 Key".to_string())?;
    if row.base_url.is_empty() {
        return Err("LLM 网关 base_url 未配置（请检查设置页 Agnes base_url）".into());
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
        .map_err(|e| format!("解析 Agnes 响应失败: {e}"))?;
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
    let script = std::env::var("VF_TRANSCRIBE_SCRIPT")
        .unwrap_or_else(|_| "python-sidecar/transcribe.py".to_string());
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
    let base = base_url.trim_end_matches('/');
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
        .map_err(|e| format!("Agnes 调用失败: {e}"))?;
    if !resp.status().is_success() {
        let st = resp.status();
        let txt = resp.text().await.unwrap_or_default();
        return Err(format!("Agnes 返回 {st}: {txt}"));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| format!("解析 Agnes 响应失败: {e}"))?;
    let text = v
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or_else(|| "Agnes 响应缺 choices[0].message.content".to_string())?;
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
        return Err("LLM 网关 base_url 未配置（请检查设置页 Agnes base_url）".into());
    }
    let key = cred::get_key(pool, "llm").await?
        .ok_or_else(|| "尚未保存 LLM（Agnes）API Key，请先在设置页保存 Key".to_string())?;
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
    let base = base_url.trim_end_matches('/');
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
        .map_err(|e| format!("Agnes 调用失败: {e}"))?;
    if !resp.status().is_success() {
        let st = resp.status();
        let txt = resp.text().await.unwrap_or_default();
        return Err(format!("Agnes 返回 {st}: {txt}"));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let text = v
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or_else(|| "Agnes 响应缺 choices[0].message.content".to_string())?;
    Ok(text.trim().to_string())
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

    // 目标段数：3-8 区间
    let n = target_segments.clamp(3, 8);
    // 每段约几句话
    let per_seg = (raw_sentences.len() + n - 1) / n;
    let total_duration = if total_duration > 0.0 { total_duration } else { (n as f64) * 60.0 };

    let mut out: Vec<db::TimelineClip> = Vec::new();
    for i in 0..n {
        let s_idx = i * per_seg;
        let e_idx = ((i + 1) * per_seg).min(raw_sentences.len());
        let text = if s_idx < e_idx {
            raw_sentences[s_idx..e_idx].join("。") + "。"
        } else {
            String::new()
        };
        let start = (i as f64 / n as f64) * total_duration;
        let end = ((i + 1) as f64 / n as f64) * total_duration;
        out.push(db::TimelineClip {
            id: format!("s{i}"),
            source: "narration".into(),
            timeline_start: start,
            timeline_end: end,
            src_start: start,
            src_end: end,
            label: String::new(),
            text,
            flower: String::new(),
            transition: "none".into(),
        });
    }
    let _ = json!({}); // 抑制 unused import
    out
}

/// M2.5：生成六段式 narrative prompt（开端/铺垫/冲突/高潮/反转/结局）
pub fn build_narration_prompt(
    title: &str,
    style: &str,
    duration_min: u32,
    asr_text: &str,
    asr_failed: bool,
) -> String {
    let target_chars = duration_min * 270; // 每分钟约 270 字
    let style_hint = match style {
        "movie" | "电影解说" => "电影解说风格，叙事沉稳、画面感强、善用比喻",
        "series" | "电视剧" => "电视剧风格，剧情连贯、悬念推进",
        "variety" | "综艺" => "综艺风格，活泼欢快、节奏明快",
        "anime" | "动漫" => "动漫风格，夸张表达、富有张力",
        "doc" | "纪录片" => "纪录片风格，平实严谨、考据详实",
        "horror" | "悬疑文案" => "悬疑风格，紧张氛围、层层递进",
        "funny" | "轻松搞笑" => "轻松搞笑风格，幽默诙谐、口语化",
        "emotion" | "激情解说" => "激情解说风格，情感浓烈、爆发力强",
        "knowledge" | "知识科普" => "知识科普风格，逻辑清晰、举例说明",
        _ => "通用解说风格，叙事自然、有画面感",
    };
    let asr_block = if asr_failed {
        "（无法获取视频原声，请仅依据以上标题与风格自由创作一段解说文案，不要编造具体剧情细节）".to_string()
    } else {
        format!("视频转写（来自 ASR，可能不完整）：\n{asr_text}")
    };
    format!(
        r#"请你为以下视频撰写一段时长约 {duration_min} 分钟的解说文案。
视频标题：{title}
解说风格：{style_hint}
视频时长：{duration_min} 分钟（≈ {target_chars} 字）

{asr_block}

要求：
1. 遵循六段式故事结构：开端 → 铺垫 → 冲突 → 高潮 → 反转 → 结局
2. 每段标注时间戳 [mm:ss-mm:ss]，段长 30-60 字
3. 风格语气自然，匹配观众期待
4. 字数 ≈ {target_chars} 字
5. 用词口语化，避免空话

返回严格 JSON 数组（不要任何额外文字）：
[{{"start": "0:00", "end": "0:30", "dialogue": "...", "section": "开端"}}, ...]
其中 section 必须是这 6 个之一：开端 / 铺垫 / 冲突 / 高潮 / 反转 / 结局
"#
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

        shots.push(db::TimelineClip {
            id: format!("s{i}"),
            source: "narration".into(),
            timeline_start: start_sec,
            timeline_end: end_sec,
            src_start: start_sec,
            src_end: end_sec,
            label: section.clone(),
            text: if !section.is_empty() && !dialogue.starts_with(&format!("[{}]", section)) {
                format!("[{}] {}", section, dialogue)
            } else {
                dialogue
            },
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
    let language = job.payload.get("language").and_then(|v| v.as_str()).unwrap_or("zh").to_string();
    let duration = job.payload.get("duration").and_then(|v| v.as_u64()).unwrap_or(180) as u32;
    let hint = job.payload.get("hint").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let title = job.payload.get("title").and_then(|v| v.as_str()).unwrap_or("未知").to_string();
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

    // 3) LLM 六段式生成
    emit(ProgressMsg {
        task_id: job.id.clone(),
        progress: 50.0,
        status: "running".into(),
        message: Some("Agnes LLM 生成六段式文案".into()),
        payload: None,
    });
    let (script, _degraded) = match llm_provider(pool).await {
        Ok((base_url, model, key)) => {
            let prompt = build_narration_prompt(&title, &style, duration, &asr_text, asr_failed);
            match run_llm_text(client, &base_url, &model, &key, &prompt).await {
                Ok(text) => {
                    match parse_narration_response(&text, duration as f64, &style) {
                        Ok(mut shots) => {
                            // 用 ASR 文本覆盖每段 dialogue（更准确）
                            let asr_sentences: Vec<&str> = asr_text
                                .split(|c: char| "。！？!?，,；;\n".contains(c))
                                .map(|s| s.trim())
                                .filter(|s| !s.is_empty())
                                .collect();
                            let per = if asr_sentences.is_empty() { 0 } else { (asr_sentences.len() + shots.len() - 1) / shots.len().max(1) };
                            for (i, s) in shots.iter_mut().enumerate() {
                                let s_idx = i * per;
                                let e_idx = ((i + 1) * per).min(asr_sentences.len());
                                if s_idx < e_idx {
                                    s.text = asr_sentences[s_idx..e_idx].join("。") + "。";
                                }
                            }
                            let script_text: String = shots.iter().map(|s| s.text.clone()).collect::<Vec<_>>().join("\n");
                            db::film_project_set_script(pool, &project_id, &script_text).await?;
                            (script_text, false)
                        }
                        Err(e) => {
                            // 降级 2：LLM 返回非 JSON，用 pre_sections
                            let _ = e;
                            let pre_text: String = pre_sections.iter().map(|s| s.text.clone()).collect::<Vec<_>>().join("\n");
                            db::film_project_set_script(pool, &project_id, &pre_text).await?;
                            (pre_text, true)
                        }
                    }
                }
                Err(_) => {
                    // 降级 2：LLM 失败，用 ASR 切分
                    let pre_text: String = pre_sections.iter().map(|s| s.text.clone()).collect::<Vec<_>>().join("\n");
                    db::film_project_set_script(pool, &project_id, &pre_text).await?;
                    (pre_text, true)
                }
            }
        }
        Err(_e) => {
            // 降级 1：缺 Key
            let pre_text: String = pre_sections.iter().map(|s| s.text.clone()).collect::<Vec<_>>().join("\n");
            db::film_project_set_script(pool, &project_id, &pre_text).await?;
            (pre_text, true)
        }
    };
    let _ = language; // 抑制 unused 警告
    Ok((script, asr_failed, asr_reason))
}
