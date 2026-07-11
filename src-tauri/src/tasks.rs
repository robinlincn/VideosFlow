// VideosFlow — 任务队列 + 进度广播（tokio mpsc + Tauri Channel）
// M0：worker 消费任务，做 sidecar 健康检查并广播进度，最终持久化状态到 tasks 表。
// M2：新增 film_import / film_smart_cut / film_export 分支（确定性纯算法 + ffmpeg）。

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tokio::sync::mpsc;

use std::path::Path;

use reqwest::Client;
use sqlx::sqlite::SqlitePool;

use crate::db;
use crate::ffmpeg::{self, FfMpeg};
use crate::{cred, python};

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

/// 启动 worker（在 Tauri 异步运行时中消费队列）。
pub fn start(pool: SqlitePool, client: Client, port: u16, data_dir: std::path::PathBuf, rx: mpsc::Receiver<TaskJob>) {
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
    let emit = |m: ProgressMsg| {
        let _ = job.channel.send(m);
    };

    emit(ProgressMsg {
        task_id: job.id.clone(),
        progress: 5.0,
        status: "queued".into(),
        message: Some("任务入队".into()),
        payload: None,
    });
    db::task_update(pool, &job.id, "running", 10.0, "引擎健康检查").await.ok();

    // sidecar 连通性
    let alive = python::health(client, port).await;
    if !alive {
        db::task_update(
            pool,
            &job.id,
            "failed",
            30.0,
            "Python sidecar 未运行（请先启动 python-sidecar，或检查 VF_PYTHON/VF_SIDECAR_DIR）",
        )
        .await
        .ok();
        emit(ProgressMsg {
            task_id: job.id.clone(),
            progress: 100.0,
            status: "failed".into(),
            message: Some("sidecar 未运行".into()),
            payload: None,
        });
        return;
    }

    emit(ProgressMsg {
        task_id: job.id.clone(),
        progress: 60.0,
        status: "running".into(),
        message: Some("AI 引擎可达".into()),
        payload: None,
    });
    db::task_update(pool, &job.id, "running", 60.0, "AI 引擎可达").await.ok();

    match job.kind.as_str() {
        "chat" | "llm_chat" => {
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
            match run_film_import(pool, client, port, data_dir, &job, &emit).await {
                Ok(degraded) => {
                    db::task_update(pool, &job.id, "done", 100.0, "导入对齐完成").await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "done".into(),
                        message: Some(if degraded { "导入完成（ASR 未就绪，已生成草稿时间线）" } else { "导入对齐完成" }.into()),
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
            match run_film_smart_cut(pool, client, port, data_dir, &job, &emit).await {
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
            match run_film_export(pool, client, port, data_dir, &job, &emit).await {
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

/// 真实对话任务：取 llm provider 配置 + 系统凭据库 Key，经 sidecar /v1/chat 调用 Agnes。
async fn run_chat(pool: &SqlitePool, client: &Client, port: u16, job: &TaskJob) -> Result<String, String> {
    let row = db::get_by_kind(pool, "llm").await?;
    let key = cred::get_key("llm")?;
    let cfg = python::build_cfg(&row, key);
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
    let env = python::call_chat(client, port, &cfg, &prompt, max_tokens).await?;
    if env.ok {
        Ok(extract_chat_text(&env.data))
    } else {
        Err(env.message)
    }
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

fn parse_asr_data(data: &Option<serde_json::Value>) -> (Vec<db::AsrSegment>, String, f64) {
    let mut segs = Vec::new();
    let mut lang = "zh".to_string();
    let mut dur = 0.0;
    if let Some(d) = data {
        if let Some(arr) = d.get("segments").and_then(|v| v.as_array()) {
            for s in arr {
                segs.push(db::AsrSegment {
                    start: s.get("start").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    end: s.get("end").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    text: s.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    confidence: s.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.0),
                });
            }
        }
        if let Some(l) = d.get("language").and_then(|v| v.as_str()) {
            lang = l.to_string();
        }
        dur = d.get("duration").and_then(|v| v.as_f64()).unwrap_or(0.0);
    }
    (segs, lang, dur)
}

// ===========================================================================
// M2：异步任务实现
// ===========================================================================

/// 导入对齐：抽音轨 → ASR（降级不阻塞）→ 对齐 → 存草稿时间线。
async fn run_film_import(
    pool: &SqlitePool,
    client: &Client,
    port: u16,
    data_dir: &Path,
    job: &TaskJob,
    emit: &dyn Fn(ProgressMsg),
) -> Result<bool, String> {
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

    // ASR（降级：不可达/失败不阻塞导入）
    let (segments, _language, _duration, degraded) = if audio_path.is_empty() {
        (Vec::new(), "zh".to_string(), 0.0, true)
    } else {
        match db::get_by_kind(pool, "asr").await {
            Ok(row) => {
                let key = cred::get_key("asr").ok().flatten();
                let cfg = python::build_cfg(&row, key);
                match python::call_asr(client, port, &cfg, &audio_path, "zh").await {
                    Ok(env) if env.ok => {
                        let (segs, lang, dur) = parse_asr_data(&env.data);
                        (segs, lang, dur, false)
                    }
                    _ => (Vec::new(), "zh".to_string(), 0.0, true),
                }
            }
            Err(_) => (Vec::new(), "zh".to_string(), 0.0, true),
        }
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

    Ok(degraded)
}

/// 智能粗剪：载入时间线 → 分段+对齐 → 静音检测 → 生成多轨粗剪时间线。
async fn run_film_smart_cut(
    pool: &SqlitePool,
    client: &Client,
    port: u16,
    data_dir: &Path,
    job: &TaskJob,
    emit: &dyn Fn(ProgressMsg),
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
    port: u16,
    data_dir: &Path,
    job: &TaskJob,
    emit: &dyn Fn(ProgressMsg),
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
        if let Ok(row) = db::get_by_kind(pool, "tts").await {
            let key = cred::get_key("tts").ok().flatten();
            let cfg = python::build_cfg(&row, key);
            if let Ok(env) = python::call_tts(client, port, &cfg, &script, "default").await {
                if let Some(d) = &env.data {
                    if let Some(vp) = d.get("audioPath").and_then(|v| v.as_str()) {
                        let mixed = tmp.join("mixed.mp4");
                        ff.mux_cmd(final_video.to_str().unwrap(), vp, mixed.to_str().unwrap())
                            .output()
                            .map_err(|e| e.to_string())?;
                        final_video = mixed;
                    }
                }
            }
        }
    }

    let out = data_dir.join(format!("export_{project_id}.mp4"));
    ff.export_cmd(final_video.to_str().unwrap(), out.to_str().unwrap(), hw, &resolution)
        .output()
        .map_err(|e| e.to_string())?;

    Ok(out.to_string_lossy().to_string())
}
