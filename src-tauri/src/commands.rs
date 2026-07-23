// VideosFlow — Tauri 命令实现
// M0：ping / provider 读写 / 连接测试 / 任务提交 / 任务状态。
// M2：film_category_* / film_project_* / film_timeline_save / film_timeline_load /
//     film_import / film_smart_cut / film_export（后三者内部提交异步任务并经 Channel 推进度）。

use base64::Engine;
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::State;

use crate::{cred, db, tasks, AppState};
use tasks::{ProgressMsg, TaskJob};

#[tauri::command]
pub fn ping() -> String {
    "pong".into()
}

#[tauri::command]
pub async fn provider_list(state: State<'_, AppState>) -> Result<Vec<db::ProviderRow>, String> {
    let mut rows = db::list(&state.pool).await?;
    // DB 的 has_key 列为权威标记（密钥写入成功即置位）；仅在 DB 为 false 时回退加密存储探测，
    // 兼容历史已存密钥。
    for r in rows.iter_mut() {
        if !r.has_key {
            r.has_key = cred::has_key(&state.pool, &r.kind).await;
        }
    }
    Ok(rows)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn provider_upsert(
    state: State<'_, AppState>,
    kind: String,
    name: String,
    provider: String,
    base_url: String,
    model: String,
    enabled: bool,
    mode: String,
) -> Result<(), String> {
    db::upsert(&state.pool, &kind, &name, &provider, &base_url, &model, enabled, &mode).await
}

#[tauri::command]
pub async fn provider_key_set(
    state: State<'_, AppState>,
    kind: String,
    key: String,
) -> Result<(), String> {
    if key.trim().is_empty() {
        // 清空密钥：从加密存储删除，并把 DB 标记复位
        cred::delete_key(&state.pool, &kind).await?;
        db::set_has_key(&state.pool, &kind, false).await?;
        return Ok(());
    }
    // 写入加密存储；成功后以 DB 布尔标记记录「已保存」，作为 UI 提示权威来源。
    // 密钥本体加密后存 SQLite（不落明文）。
    cred::set_key(&state.pool, &kind, &key).await?;
    db::set_has_key(&state.pool, &kind, true).await?;
    Ok(())
}

#[tauri::command]
pub async fn provider_key_get(
    state: State<'_, AppState>,
    kind: String,
) -> Result<Option<String>, String> {
    cred::get_key(&state.pool, &kind).await
}

/// 返回本地模型目录（资源文件夹下的 models 子目录），用于放置 faster-whisper / Whisper 等本地大模型权重。
/// 目录在首次调用时自动创建。
#[tauri::command(rename_all = "camelCase")]
pub async fn get_models_dir(state: State<'_, AppState>) -> Result<String, String> {
    let dir = &state.models_dir;
    std::fs::create_dir_all(dir).map_err(|e| format!("创建模型目录失败: {e}"))?;
    Ok(dir.to_string_lossy().into_owned())
}

/// 下载本地 ASR 模型（faster-whisper CTranslate2 权重）到项目内 models 目录。
/// `model` 为尺寸（tiny/base/small/medium/large-v3），`source` 为下载源
/// （hf-mirror=HuggingFace 国内镜像，huggingface=直连官方）。通过 Channel 实时回报进度。
#[tauri::command(rename_all = "camelCase")]
pub async fn download_model(
    state: State<'_, AppState>,
    model: String,
    source: String,
    on_progress: Channel<serde_json::Value>,
) -> Result<String, String> {
    let size = model.trim();
    let repo = format!("Systran/faster-whisper-{size}");
    let base = if source == "huggingface" {
        "https://huggingface.co"
    } else {
        "https://hf-mirror.com"
    };
    let local_dir = state.models_dir.join(size);
    std::fs::create_dir_all(&local_dir).map_err(|e| e.to_string())?;

    // 1) 列举模型文件树
    let tree_url = format!("{base}/api/models/{repo}/tree/main");
    on_progress.send(serde_json::json!({"phase": "listing", "repo": repo})).ok();
    let resp = state
        .client
        .get(&tree_url)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| format!("列举文件失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "列举文件失败 HTTP {}（源 {base} 可能不可达，请切换下载源）",
            resp.status()
        ));
    }
    let files: Vec<serde_json::Value> = resp.json().await.map_err(|e| e.to_string())?;
    // 过滤掉 .gitattributes（无需下载）；其余（config.json/model.bin/tokenizer.json/vocabulary.txt/README 等）全部下载
    let to_download: Vec<(String, u64)> = files
        .iter()
        .filter_map(|f| {
            let path = f.get("path")?.as_str()?.to_string();
            if path == ".gitattributes" {
                return None;
            }
            let sz = f.get("size").and_then(|s| s.as_u64()).unwrap_or(0);
            Some((path, sz))
        })
        .collect();
    if to_download.is_empty() {
        return Err("未从源获取到任何模型文件".into());
    }
    let total: u64 = to_download.iter().map(|(_, s)| *s).sum();
    let mut done: u64 = 0;
    for (path, _fsize) in to_download {
        let file_url = format!("{base}/{repo}/resolve/main/{path}");
        let out_path = local_dir.join(&path);
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let mut resp = state
            .client
            .get(&file_url)
            .timeout(Duration::from_secs(1800))
            .send()
            .await
            .map_err(|e| format!("下载 {path} 失败: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("下载 {path} 失败 HTTP {}", resp.status()));
        }
        use std::io::Write;
        let mut f = std::fs::File::create(&out_path).map_err(|e| e.to_string())?;
        while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
            f.write_all(&chunk).map_err(|e| e.to_string())?;
            done += chunk.len() as u64;
            on_progress
                .send(serde_json::json!({
                    "phase": "downloading",
                    "file": path,
                    "current": done,
                    "total": total
                }))
                .ok();
        }
    }
    on_progress
        .send(serde_json::json!({"phase": "done", "dir": local_dir.display().to_string()}))
        .ok();
    Ok(local_dir.display().to_string())
}

/// 检查指定尺寸的本地模型是否已下载（models/{model} 下同时存在 model.bin 与 config.json）。
#[tauri::command(rename_all = "camelCase")]
pub async fn check_local_model(
    state: State<'_, AppState>,
    model: String,
) -> Result<bool, String> {
    let dir = state.models_dir.join(model.trim());
    Ok(dir.join("model.bin").exists() && dir.join("config.json").exists())
}

/// 连接测试：Rust 端直接用 reqwest 探测。
/// - ASR / TTS：直连真实端点做「功能级」验证（用静音样本 / 短文本真正请求一次），
///   只有端点返回 2xx 且不是鉴权错误，才视为 Key 有效 —— 避免「仅连通性」假阳性。
/// - 其它（LLM/Image/Video，OpenAI 兼容）：优先 /models 验证鉴权，否则退回基础连通性。
/// `api_key` 为可选：传入时优先用「正在填写、尚未保存」的 Key 直接测，无需先点保存。
#[tauri::command(rename_all = "camelCase")]
pub async fn provider_test(
    state: State<'_, AppState>,
    kind: String,
    api_key: Option<String>,
) -> Result<String, String> {
    let row = db::get_by_kind(&state.pool, &kind).await?;
    // 本地推理模式无需联网测试
    if row.mode == "local" {
        return Ok("local".into());
    }
    let stored = cred::get_key(&state.pool, &kind).await?;
    let key = api_key
        .filter(|k| !k.is_empty())
        .or_else(|| stored.filter(|k| !k.is_empty()));
    let base = row.base_url.trim().to_string();
    // 本地模式（如 faster-whisper / Whisper）：无云端，无需连接测试，直接返回 'local'
    if row.mode == "local" {
        return Ok("local".into());
    }
    if base.is_empty() {
        return Err("请先填写 Base URL 再测试连接".into());
    }
    let client = &state.client;
    let has_key = key.is_some();

    // 真实功能测试：ASR / TTS 直连真实端点，验证 Key 对该能力确实有效
    match kind.as_str() {
        "asr" => return test_asr(client, &base, &row.model, key.as_deref()).await,
        "tts" => return test_tts(client, &base, &row.model, key.as_deref()).await,
        _ => {}
    }

    // 通用 OpenAI 兼容（LLM/Image/Video）：优先 /models 验证鉴权，否则退回连通性
    let models_url = format!("{}/models", base.trim_end_matches('/'));
    let mut req = client.get(&models_url).timeout(Duration::from_secs(8));
    if let Some(k) = &key {
        req = req.bearer_auth(k);
    }
    match req.send().await {
        Ok(resp) => {
            let code = resp.status().as_u16();
            if resp.status().is_success() {
                return Ok("ok".into());
            }
            if code == 401 || code == 403 {
                return Err(if has_key {
                    "API Key 无效或未授权（HTTP 401/403），请检查密钥".into()
                } else {
                    "缺少 API Key，请先填写 Key 再测试连接".into()
                });
            }
            // 其他业务状态码（如 404/405/500）→ 退回基础连通性判定
        }
        Err(_) => {
            // /models 网络层失败（网关可能不支持该路径）→ 退回对根地址探测
        }
    }
    // 基础连通性：对根地址发请求，只要收到任何 HTTP 响应即视为可达
    match client.get(&base).timeout(Duration::from_secs(8)).send().await {
        Ok(_) => Ok("ok".into()),
        Err(e) => Err(format!("无法连接到 {base}：{e}")),
    }
}

/// 生成一个极小的静音 WAV（PCM16 单声道 8kHz，0.3s），用于 ASR 探测时作为有效音频载荷。
fn make_probe_wav() -> Vec<u8> {
    let sample_rate: u32 = 8000;
    let num_samples: u32 = sample_rate * 3 / 10; // 0.3s
    let data_len = num_samples * 2;
    let mut wav: Vec<u8> = Vec::with_capacity(44 + data_len as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // 单声道
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    wav.extend_from_slice(&2u16.to_le_bytes()); // block align
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for _ in 0..num_samples {
        wav.extend_from_slice(&0i16.to_le_bytes());
    }
    wav
}

/// ASR 功能测试：用静音 WAV 真实请求一次 /chat/completions。
/// 通过标准：HTTP 2xx 且响应体非「鉴权错误」。静音转写为空属正常，不算失败。
async fn test_asr(client: &Client, base: &str, model: &str, key: Option<&str>) -> Result<String, String> {
    let key = key.ok_or("缺少 API Key，请先填写 Key 再测试 ASR")?;
    let wav = make_probe_wav();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&wav);
    let base_url = base.trim_end_matches('/');
    let body = serde_json::json!({
        "model": model,
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
    let resp = client
        .post(format!("{base_url}/chat/completions"))
        .bearer_auth(key)
        .json(&body)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("ASR 请求失败：{e}"))?;
    let code = resp.status().as_u16();
    if !resp.status().is_success() {
        if code == 401 || code == 403 {
            return Err("ASR API Key 无效或未授权（HTTP 401/403），请检查密钥".into());
        }
        let txt = resp.text().await.unwrap_or_default();
        return Err(format!("ASR 端点返回错误（HTTP {code}）：{txt}"));
    }
    // 2xx：鉴权通过。若响应体携带 error 字段则提示（部分网关以 200 包裹错误）。
    if let Ok(v) = resp.json::<serde_json::Value>().await {
        if let Some(err) = v.get("error") {
            return Err(format!("ASR 端点返回错误：{err}"));
        }
    }
    Ok("ok".into())
}

/// TTS 功能测试：用一句短文本真实请求一次 Chat Completions 协议，校验返回音频。
async fn test_tts(client: &Client, base: &str, model: &str, key: Option<&str>) -> Result<String, String> {
    let key = key.ok_or("缺少 API Key，请先填写 Key 再测试 TTS")?;
    let body = serde_json::json!({
        "model": model,
        "messages": [ { "role": "assistant", "content": "你好。" } ],
        "audio": { "format": "wav", "voice": "mimo_default" },
        "stream": false,
    });
    let resp = client
        .post(format!("{}/chat/completions", base.trim_end_matches('/')))
        .header("api-key", key)
        .bearer_auth(key)
        .json(&body)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("TTS 请求失败：{e}"))?;
    let code = resp.status().as_u16();
    if !resp.status().is_success() {
        if code == 401 || code == 403 {
            return Err("TTS API Key 无效或未授权（HTTP 401/403），请检查密钥".into());
        }
        let txt = resp.text().await.unwrap_or_default();
        return Err(format!("TTS 端点返回错误（HTTP {code}）：{txt}"));
    }
    let j: serde_json::Value = resp.json().await.map_err(|e| format!("TTS 响应解析失败：{e}"))?;
    let has_audio = j
        .get("choices").and_then(|c| c.get(0))
        .and_then(|m| m.get("message"))
        .and_then(|m| m.get("audio"))
        .and_then(|a| a.get("data"))
        .and_then(|d| d.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    if !has_audio {
        return Err("TTS 返回中缺少 audio.data，请确认模型/密钥是否支持 TTS".into());
    }
    Ok("ok".into())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn task_submit(
    state: State<'_, AppState>,
    kind: String,
    project_id: Option<String>,
    payload: serde_json::Value,
    on_progress: Channel<ProgressMsg>,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::task_create(&state.pool, &id, &kind, project_id.as_deref()).await?;
    let job = TaskJob {
        id: id.clone(),
        kind,
        project_id,
        payload,
        channel: on_progress,
    };
    state.task_tx.send(job).await.map_err(|e| e.to_string())?;
    Ok(id)
}

#[tauri::command]
pub async fn task_status(state: State<'_, AppState>, id: String) -> Result<db::TaskStatus, String> {
    db::task_get(&state.pool, &id).await
}

// ---------------------------------------------------------------------------
// M2：film_categories 命令
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn film_category_list(state: State<'_, AppState>) -> Result<Vec<db::FilmCategoryRow>, String> {
    db::film_category_list(&state.pool).await
}

#[tauri::command]
pub async fn film_category_create(state: State<'_, AppState>, name: String, order: i64) -> Result<String, String> {
    db::film_category_create(&state.pool, &name, order).await
}

#[tauri::command]
pub async fn film_category_rename(state: State<'_, AppState>, id: String, name: String) -> Result<(), String> {
    db::film_category_rename(&state.pool, &id, &name).await
}

#[tauri::command]
pub async fn film_category_reorder(state: State<'_, AppState>, id: String, order: i64) -> Result<(), String> {
    db::film_category_reorder(&state.pool, &id, order).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilmCategoryDelete {
    pub id: String,
    #[serde(default = "default_strategy")]
    pub strategy: String,
    #[serde(default)]
    pub target_id: Option<String>,
}

fn default_strategy() -> String {
    "cascade".into()
}

#[tauri::command]
pub async fn film_category_delete(state: State<'_, AppState>, req: FilmCategoryDelete) -> Result<(), String> {
    db::film_category_delete(&state.pool, &req.id, &req.strategy, req.target_id.as_deref()).await
}

// ---------------------------------------------------------------------------
// M2：film_projects 命令
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn film_project_list(state: State<'_, AppState>, category_id: String) -> Result<Vec<db::FilmProjectRow>, String> {
    db::film_project_list(&state.pool, &category_id).await
}

#[tauri::command]
pub async fn film_project_create(
    state: State<'_, AppState>,
    category_id: String,
    title: String,
    cover: Option<String>,
) -> Result<String, String> {
    db::film_project_create(&state.pool, &category_id, &title, cover.as_deref()).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilmProjectUpdate {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub tags: Option<String>,
}

#[tauri::command]
pub async fn film_project_update(state: State<'_, AppState>, req: FilmProjectUpdate) -> Result<(), String> {
    db::film_project_update(
        &state.pool,
        &req.id,
        req.title.as_deref(),
        req.cover.as_deref(),
        req.status.as_deref(),
        req.tags.as_deref(),
    )
    .await
}

#[tauri::command]
pub async fn film_project_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    db::film_project_delete(&state.pool, &id).await
}

// ---------------------------------------------------------------------------
// M2：edit_timelines 命令
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn film_timeline_load(state: State<'_, AppState>, project_id: String) -> Result<Option<db::TimelineRow>, String> {
    db::timeline_get(&state.pool, &project_id).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilmTimelineSave {
    pub project_id: String,
    pub tracks: String,
    pub clips: String,
}

#[tauri::command]
pub async fn film_timeline_save(state: State<'_, AppState>, req: FilmTimelineSave) -> Result<String, String> {
    db::timeline_save(&state.pool, &req.project_id, &req.tracks, &req.clips).await
}

// ---------------------------------------------------------------------------
// M2：异步任务命令（内部提交 film_* 任务，返回任务 id）
// ---------------------------------------------------------------------------

#[tauri::command(rename_all = "camelCase")]
pub async fn film_import(
    state: State<'_, AppState>,
    project_id: String,
    video_path: String,
    script: String,
    on_progress: Channel<ProgressMsg>,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::task_create(&state.pool, &id, "film_import", Some(&project_id)).await?;
    let job = TaskJob {
        id: id.clone(),
        kind: "film_import".into(),
        project_id: Some(project_id.clone()),
        payload: serde_json::json!({
            "projectId": project_id,
            "videoPath": video_path,
            "script": script,
        }),
        channel: on_progress,
    };
    state.task_tx.send(job).await.map_err(|e| e.to_string())?;
    Ok(id)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn film_smart_cut(
    state: State<'_, AppState>,
    project_id: String,
    script: String,
    on_progress: Channel<ProgressMsg>,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::task_create(&state.pool, &id, "film_smart_cut", Some(&project_id)).await?;
    let job = TaskJob {
        id: id.clone(),
        kind: "film_smart_cut".into(),
        project_id: Some(project_id.clone()),
        payload: serde_json::json!({
            "projectId": project_id,
            "script": script,
        }),
        channel: on_progress,
    };
    state.task_tx.send(job).await.map_err(|e| e.to_string())?;
    Ok(id)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn film_export(
    state: State<'_, AppState>,
    project_id: String,
    hw: bool,
    resolution: String,
    burn_sub: bool,
    mix_voice: bool,
    voice_mix: f64,
    script: String,
    on_progress: Channel<ProgressMsg>,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::task_create(&state.pool, &id, "film_export", Some(&project_id)).await?;
    let job = TaskJob {
        id: id.clone(),
        kind: "film_export".into(),
        project_id: Some(project_id.clone()),
        payload: serde_json::json!({
            "projectId": project_id,
            "hw": hw,
            "resolution": resolution,
            "burnSub": burn_sub,
            "mixVoice": mix_voice,
            "voiceMix": voice_mix,
            "script": script,
        }),
        channel: on_progress,
    };
    state.task_tx.send(job).await.map_err(|e| e.to_string())?;
    Ok(id)
}

// ---------------------------------------------------------------------------
// M3：口播模块命令
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn spoken_video_list(state: State<'_, AppState>) -> Result<Vec<db::SpokenVideoRow>, String> {
    db::spoken_video_list(&state.pool).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn spoken_video_create(
    state: State<'_, AppState>,
    name: String,
    path: String,
    duration: f64,
) -> Result<String, String> {
    db::spoken_video_create(&state.pool, &name, &path, duration).await
}

#[tauri::command]
pub async fn spoken_video_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    db::spoken_video_delete(&state.pool, &id).await
}

#[tauri::command]
pub async fn spoken_video_get(state: State<'_, AppState>, id: String) -> Result<db::SpokenVideoRow, String> {
    db::spoken_video_get(&state.pool, &id).await
}

/// 同步提取文案：按标点切 transcript 句并去填充词。
#[tauri::command]
pub async fn spoken_extract_script(state: State<'_, AppState>, video_id: String) -> Result<String, String> {
    let v = db::spoken_video_get(&state.pool, &video_id).await?;
    let script = tasks::extract_script_from_transcript(&v.transcript);
    db::spoken_video_set_script(&state.pool, &video_id, &script).await?;
    Ok(script)
}

#[tauri::command]
pub async fn spoken_edits_list(state: State<'_, AppState>, video_id: String) -> Result<Vec<db::SpokenEditRow>, String> {
    db::spoken_edits_list(&state.pool, &video_id).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn spoken_edits_set_accepted(
    state: State<'_, AppState>,
    id: String,
    accepted: i64,
) -> Result<(), String> {
    db::spoken_edits_set_accepted(&state.pool, &id, accepted).await
}

/// 一键应用所有 accepted=1 的 edits → 生成 cleanScript（不破坏原片）。
#[tauri::command]
pub async fn spoken_apply_edits(state: State<'_, AppState>, video_id: String) -> Result<String, String> {
    let v = db::spoken_video_get(&state.pool, &video_id).await?;
    let edits = db::spoken_edits_list(&state.pool, &video_id).await?;
    let clean = tasks::apply_edits_to_transcript(&v.transcript, &edits);
    db::spoken_video_set_clean(&state.pool, &video_id, &clean).await?;
    Ok(clean)
}

#[tauri::command]
pub async fn spoken_assets_list(state: State<'_, AppState>, video_id: String) -> Result<Vec<db::SpokenAssetRow>, String> {
    db::spoken_assets_list(&state.pool, &video_id).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn spoken_asset_create(
    state: State<'_, AppState>,
    video_id: String,
    name: String,
    kind: String,
    path: String,
) -> Result<String, String> {
    db::spoken_asset_create(&state.pool, &video_id, &name, &kind, &path).await
}

#[tauri::command]
pub async fn spoken_asset_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    db::spoken_asset_delete(&state.pool, &id).await
}

#[tauri::command]
pub async fn spoken_keywords_list(state: State<'_, AppState>, video_id: String) -> Result<Vec<db::SpokenKeywordRow>, String> {
    db::spoken_keywords_list(&state.pool, &video_id).await
}

#[tauri::command]
pub async fn spoken_matches_list(state: State<'_, AppState>, video_id: String) -> Result<Vec<db::SpokenMatchRow>, String> {
    db::spoken_matches_list(&state.pool, &video_id).await
}

#[tauri::command]
pub async fn spoken_match_toggle(state: State<'_, AppState>, id: String) -> Result<(), String> {
    db::spoken_match_toggle(&state.pool, &id).await
}

/// 异步任务：抽音轨 → XiaomiMimo ASR → 写 transcript（共用 M2 transcribe_asr）。
#[tauri::command(rename_all = "camelCase")]
pub async fn spoken_asr(
    state: State<'_, AppState>,
    video_id: String,
    on_progress: Channel<ProgressMsg>,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::task_create(&state.pool, &id, "spoken_asr", Some(&video_id)).await?;
    let job = TaskJob {
        id: id.clone(),
        kind: "spoken_asr".into(),
        project_id: Some(video_id.clone()),
        payload: serde_json::json!({ "videoId": video_id }),
        channel: on_progress,
    };
    state.task_tx.send(job).await.map_err(|e| e.to_string())?;
    Ok(id)
}

/// 异步任务：gap(FFmpeg) + repeat(Rust) + mistake(Agnes LLM) → 写 spoken_edits。
#[tauri::command(rename_all = "camelCase")]
pub async fn spoken_detect(
    state: State<'_, AppState>,
    video_id: String,
    on_progress: Channel<ProgressMsg>,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::task_create(&state.pool, &id, "spoken_detect", Some(&video_id)).await?;
    let job = TaskJob {
        id: id.clone(),
        kind: "spoken_detect".into(),
        project_id: Some(video_id.clone()),
        payload: serde_json::json!({ "videoId": video_id }),
        channel: on_progress,
    };
    state.task_tx.send(job).await.map_err(|e| e.to_string())?;
    Ok(id)
}

/// 异步任务：Agnes LLM 抽关键词 → 写 spoken_keywords（降级走 TF-IDF）。
#[tauri::command(rename_all = "camelCase")]
pub async fn spoken_keyword(
    state: State<'_, AppState>,
    video_id: String,
    on_progress: Channel<ProgressMsg>,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::task_create(&state.pool, &id, "spoken_keyword", Some(&video_id)).await?;
    let job = TaskJob {
        id: id.clone(),
        kind: "spoken_keyword".into(),
        project_id: Some(video_id.clone()),
        payload: serde_json::json!({ "videoId": video_id }),
        channel: on_progress,
    };
    state.task_tx.send(job).await.map_err(|e| e.to_string())?;
    Ok(id)
}

/// 同步：根据 spoken_keywords + spoken_assets 做贪心匹配 → 写 spoken_matches。
#[tauri::command]
pub async fn spoken_match_assets(state: State<'_, AppState>, video_id: String) -> Result<Vec<db::SpokenMatchRow>, String> {
    tasks::match_assets_greedy(&state.pool, &video_id).await
}

/// 异步任务：FFmpeg 烧录花字到视频（生成 mp4）。
#[tauri::command(rename_all = "camelCase")]
pub async fn spoken_burn(
    state: State<'_, AppState>,
    video_id: String,
    flower: String,
    on_progress: Channel<ProgressMsg>,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::task_create(&state.pool, &id, "spoken_burn", Some(&video_id)).await?;
    let job = TaskJob {
        id: id.clone(),
        kind: "spoken_burn".into(),
        project_id: Some(video_id.clone()),
        payload: serde_json::json!({ "videoId": video_id, "flower": flower }),
        channel: on_progress,
    };
    state.task_tx.send(job).await.map_err(|e| e.to_string())?;
    Ok(id)
}

/// 异步任务：基于 accepted=1 的 edits 切片段 → concat → 输出干净 mp4。
#[tauri::command(rename_all = "camelCase")]
pub async fn spoken_export(
    state: State<'_, AppState>,
    video_id: String,
    burn_flower: bool,
    flower: String,
    on_progress: Channel<ProgressMsg>,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::task_create(&state.pool, &id, "spoken_export", Some(&video_id)).await?;
    let job = TaskJob {
        id: id.clone(),
        kind: "spoken_export".into(),
        project_id: Some(video_id.clone()),
        payload: serde_json::json!({ "videoId": video_id, "burnFlower": burn_flower, "flower": flower }),
        channel: on_progress,
    };
    state.task_tx.send(job).await.map_err(|e| e.to_string())?;
    Ok(id)
}

// ---------------------------------------------------------------------------
// M4：创作模块命令
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn creation_project_list(state: State<'_, AppState>) -> Result<Vec<db::CreationProjectRow>, String> {
    db::creation_project_list(&state.pool).await
}

#[tauri::command]
pub async fn creation_project_get(state: State<'_, AppState>, id: String) -> Result<db::CreationProjectRow, String> {
    db::creation_project_get(&state.pool, &id).await
}

#[tauri::command]
pub async fn creation_project_create(state: State<'_, AppState>, brief: String) -> Result<String, String> {
    db::creation_project_create(&state.pool, &brief).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn creation_project_update(
    state: State<'_, AppState>,
    id: String,
    brief: Option<String>,
    script: Option<String>,
    humanized_script: Option<String>,
    status: Option<String>,
) -> Result<(), String> {
    db::creation_project_update(
        &state.pool,
        &id,
        brief.as_deref(),
        script.as_deref(),
        humanized_script.as_deref(),
        status.as_deref(),
    ).await
}

#[tauri::command]
pub async fn creation_project_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    db::creation_project_delete(&state.pool, &id).await
}

#[tauri::command]
pub async fn storyboard_get(state: State<'_, AppState>, project_id: String) -> Result<Option<db::StoryboardRow>, String> {
    db::storyboard_get(&state.pool, &project_id).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn storyboard_save(
    state: State<'_, AppState>,
    project_id: String,
    shots: String,
    style_ref: String,
) -> Result<String, String> {
    db::storyboard_save(&state.pool, &project_id, &shots, &style_ref).await
}

#[tauri::command]
pub async fn generated_assets_list(state: State<'_, AppState>, project_id: String) -> Result<Vec<db::GeneratedAssetRow>, String> {
    db::generated_assets_list(&state.pool, &project_id).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn submit_script_write(
    state: State<'_, AppState>,
    project_id: String,
    on_progress: Channel<ProgressMsg>,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::task_create(&state.pool, &id, "script_write", Some(&project_id)).await?;
    let job = TaskJob {
        id: id.clone(),
        kind: "script_write".into(),
        project_id: Some(project_id.clone()),
        payload: serde_json::json!({ "projectId": project_id }),
        channel: on_progress,
    };
    state.task_tx.send(job).await.map_err(|e| e.to_string())?;
    Ok(id)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn submit_script_humanize(
    state: State<'_, AppState>,
    project_id: String,
    on_progress: Channel<ProgressMsg>,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::task_create(&state.pool, &id, "script_humanize", Some(&project_id)).await?;
    let job = TaskJob {
        id: id.clone(),
        kind: "script_humanize".into(),
        project_id: Some(project_id.clone()),
        payload: serde_json::json!({ "projectId": project_id }),
        channel: on_progress,
    };
    state.task_tx.send(job).await.map_err(|e| e.to_string())?;
    Ok(id)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn submit_storyboard_gen(
    state: State<'_, AppState>,
    project_id: String,
    on_progress: Channel<ProgressMsg>,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::task_create(&state.pool, &id, "storyboard_gen", Some(&project_id)).await?;
    let job = TaskJob {
        id: id.clone(),
        kind: "storyboard_gen".into(),
        project_id: Some(project_id.clone()),
        payload: serde_json::json!({ "projectId": project_id }),
        channel: on_progress,
    };
    state.task_tx.send(job).await.map_err(|e| e.to_string())?;
    Ok(id)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn submit_image_gen(
    state: State<'_, AppState>,
    project_id: String,
    shot_index: i64,
    style_ref: String,
    on_progress: Channel<ProgressMsg>,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::task_create(&state.pool, &id, "image_gen", Some(&project_id)).await?;
    let job = TaskJob {
        id: id.clone(),
        kind: "image_gen".into(),
        project_id: Some(project_id.clone()),
        payload: serde_json::json!({ "projectId": project_id, "shotIndex": shot_index, "styleRef": style_ref }),
        channel: on_progress,
    };
    state.task_tx.send(job).await.map_err(|e| e.to_string())?;
    Ok(id)
}

/// 读取创作工程 M5 产物清单（clips / audios / tails / exported），供前端展示生成结果。
#[tauri::command(rename_all = "camelCase")]
pub async fn creation_manifest_get(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<tasks::CreationManifest, String> {
    Ok(tasks::read_creation_manifest(&state.data_dir, &project_id))
}

/// 提交「首尾帧视频」生成任务：逐镜由首帧图生成运镜片段（可选尾帧 crossfade）。
#[tauri::command(rename_all = "camelCase")]
pub async fn submit_creation_frames(
    state: State<'_, AppState>,
    project_id: String,
    tails: String,
    on_progress: Channel<ProgressMsg>,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::task_create(&state.pool, &id, "creation_frames", Some(&project_id)).await?;
    let job = TaskJob {
        id: id.clone(),
        kind: "creation_frames".into(),
        project_id: Some(project_id.clone()),
        payload: serde_json::json!({ "projectId": project_id, "tails": tails }),
        channel: on_progress,
    };
    state.task_tx.send(job).await.map_err(|e| e.to_string())?;
    Ok(id)
}

/// 提交「配音」任务：逐镜台词走 TTS 生成 wav 配音。
#[tauri::command(rename_all = "camelCase")]
pub async fn submit_creation_voice(
    state: State<'_, AppState>,
    project_id: String,
    voice: String,
    on_progress: Channel<ProgressMsg>,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::task_create(&state.pool, &id, "creation_voice", Some(&project_id)).await?;
    let job = TaskJob {
        id: id.clone(),
        kind: "creation_voice".into(),
        project_id: Some(project_id.clone()),
        payload: serde_json::json!({ "projectId": project_id, "voice": voice }),
        channel: on_progress,
    };
    state.task_tx.send(job).await.map_err(|e| e.to_string())?;
    Ok(id)
}

/// 提交「导出成片」任务：拼接镜头片段 + 混入配音 + 烧录字幕 → 最终 MP4。
#[tauri::command(rename_all = "camelCase")]
pub async fn submit_creation_export(
    state: State<'_, AppState>,
    project_id: String,
    subtitle_style: String,
    on_progress: Channel<ProgressMsg>,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::task_create(&state.pool, &id, "creation_export", Some(&project_id)).await?;
    let job = TaskJob {
        id: id.clone(),
        kind: "creation_export".into(),
        project_id: Some(project_id.clone()),
        payload: serde_json::json!({ "projectId": project_id, "subtitleStyle": subtitle_style }),
        channel: on_progress,
    };
    state.task_tx.send(job).await.map_err(|e| e.to_string())?;
    Ok(id)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn submit_film_script_gen(
    state: State<'_, AppState>,
    project_id: String,
    video_path: String,
    title: String,
    style: String,
    style_name: String,
    language: String,
    duration: u32,
    hint: String,
    mode: String,
    view: String,
    model: String,
    analysis_mode: f32,
    voice_id: String,
    subtitle_style: String,
    analysis: Option<String>,
    role_prompt: Option<String>,
    on_progress: Channel<ProgressMsg>,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::task_create(&state.pool, &id, "film_script_gen", Some(&project_id)).await?;
    let job = TaskJob {
        id: id.clone(),
        kind: "film_script_gen".into(),
        project_id: Some(project_id.clone()),
        payload: serde_json::json!({
            "projectId": project_id,
            "videoPath": video_path,
            "title": title,
            "style": style,
            "styleName": style_name,
            "language": language,
            "duration": duration,
            "hint": hint,
            "mode": mode,
            "view": view,
            "model": model,
            "analysisMode": analysis_mode,
            "voiceId": voice_id,
            "subtitleStyle": subtitle_style,
            "analysis": analysis.unwrap_or_default(),
            "role_prompt": role_prompt.unwrap_or_default(),
        }),
        channel: on_progress,
    };
    state.task_tx.send(job).await.map_err(|e| e.to_string())?;
    Ok(id)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_film_analysis(state: State<'_, AppState>, project_id: String) -> Result<Option<String>, String> {
    let r = db::film_project_get_analysis(&state.pool, &project_id).await;
    let len = match &r { Ok(Some(s)) => s.len(), _ => 0 };
    eprintln!("[get_film_analysis] project_id={project_id} report_len={len}");
    r
}

#[tauri::command(rename_all = "camelCase")]
pub async fn submit_film_video_analysis(
    state: State<'_, AppState>,
    project_id: String,
    video_path: String,
    start: f64,
    end: f64,
    title: String,
    style_name: String,
    on_progress: Channel<ProgressMsg>,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::task_create(&state.pool, &id, "film_video_analysis", Some(&project_id)).await?;
    let job = TaskJob {
        id: id.clone(),
        kind: "film_video_analysis".into(),
        project_id: Some(project_id.clone()),
        payload: serde_json::json!({
            "projectId": project_id,
            "videoPath": video_path,
            "start": start,
            "end": end,
            "title": title,
            "styleName": style_name,
        }),
        channel: on_progress,
    };
    state.task_tx.send(job).await.map_err(|e| e.to_string())?;
    Ok(id)
}

// 返回本地视频预览文件服务器的基址（127.0.0.1:随机端口），供 WebView <video> 加载本地文件
#[tauri::command(rename_all = "camelCase")]
pub fn get_video_server_url(state: State<'_, AppState>) -> String {
    format!("http://127.0.0.1:{}", state.video_server_port)
}

// ===========================================================================
// 分镜工作台：音色库 / 批量配音 / 翻译 / 剪映草稿
// ===========================================================================

/// 拉取可用音色列表（XiaomiMimo /audio/voices，OpenAI 兼容）。失败时返回内置 fallback。
#[tauri::command(rename_all = "camelCase")]
pub async fn voice_list(
    state: State<'_, AppState>,
    client: State<'_, Client>,
) -> Result<Vec<serde_json::Value>, String> {
    let row = db::get_by_kind(&state.pool, "tts").await?;
    let key = match cred::get_key(&state.pool, "tts").await {
        Ok(Some(k)) if !k.is_empty() => k,
        _ => {
            // 无 Key 时回退一组内置候选（与前端 VOICE_FALLBACK 一致）
            return Ok(serde_json::json!([
                { "id": "default", "name": "默认（系统音色）" },
                { "id": "male_calm", "name": "磁性男声 · 沉稳" },
                { "id": "male_warm", "name": "温暖男声 · 亲切" },
                { "id": "female_lively", "name": "知性女声 · 活力" },
                { "id": "female_news", "name": "标准女声 · 新闻" },
                { "id": "narrator", "name": "旁白男声 · 纪录片" }
            ]).as_array().cloned().unwrap_or_default());
        }
    };
    let base = row.base_url.trim_end_matches('/');
    let url = format!("{base}/audio/voices");
    let resp = client
        .get(&url)
        .bearer_auth(&key)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("列音色失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("音色列表 HTTP {}", resp.status()));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    // 兼容多种返回结构：直接数组 / {voices:[]} / {data:[]}
    let arr = v.as_array()
        .cloned()
        .or_else(|| v.get("voices").and_then(|x| x.as_array()).cloned())
        .or_else(|| v.get("data").and_then(|x| x.as_array()).cloned())
        .unwrap_or_default();
    // 规整为 [{id, name, gender?, language?}]
    let out: Vec<serde_json::Value> = arr.into_iter().filter_map(|x| {
        let id = x.get("id").or_else(|| x.get("voice")).or_else(|| x.get("name"))?.as_str()?.to_string();
        let name = x.get("name").and_then(|x| x.as_str()).map(|s| s.to_string()).unwrap_or_else(|| id.clone());
        Some(serde_json::json!({
            "id": id,
            "name": name,
            "gender": x.get("gender").and_then(|x| x.as_str()),
            "language": x.get("language").and_then(|x| x.as_str()),
        }))
    }).collect();
    Ok(out)
}

/// 批量配音（按 segments 调用 XiaomiMimo TTS 生成 wav，写入 data/dub/<project>/seg_<i>.wav，
/// 经 Channel 推送每段 status=done|failed + url）。完成后 emit done。
#[tauri::command(rename_all = "camelCase")]
pub async fn batch_dub(
    state: State<'_, AppState>,
    project_id: String,
    segments: String,
    on_progress: Channel<ProgressMsg>,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::task_create(&state.pool, &id, "batch_dub", Some(&project_id)).await?;
    let job = TaskJob {
        id: id.clone(),
        kind: "batch_dub".into(),
        project_id: Some(project_id.clone()),
        payload: serde_json::json!({
            "projectId": project_id,
            "segments": segments,
        }),
        channel: on_progress,
    };
    state.task_tx.send(job).await.map_err(|e| e.to_string())?;
    Ok(id)
}

/// 翻译整段解说文案到目标语言（zh/en/ja），调 LLM。返回 {index, text} 数组。
#[tauri::command(rename_all = "camelCase")]
pub async fn translate_script(
    state: State<'_, AppState>,
    project_id: String,
    language: String,
    segments: String,
) -> Result<String, String> {
    let segs: Vec<serde_json::Value> = serde_json::from_str(&segments).map_err(|e| format!("segments JSON 解析失败: {e}"))?;
    let (base_url, model, key) = match tasks::llm_provider(&state.pool).await {
        Ok(b) => b,
        Err(e) => return Err(format!("未配置文本大模型：{e}")),
    };
    let _ = project_id; // 暂不落库
    let items: Vec<serde_json::Value> = segs.iter().enumerate().map(|(i, s)| serde_json::json!({
        "index": s.get("index").and_then(|x| x.as_u64()).unwrap_or(i as u64),
        "section": s.get("section").and_then(|t| t.as_str()).unwrap_or(""),
        "text": s.get("text").and_then(|t| t.as_str()).unwrap_or(""),
    })).collect();
    let prompt = format!(
        "你是专业影视翻译。请把以下解说逐段翻译为{}{}，严格原样返回 JSON 数组，每项 {{\"index\":<原 index 整数>,\"text\":<译后文本>}}。严禁遗漏、合并、改写或补充：\n{}",
        language,
        match language.as_str() { "zh" => "（简体）", "en" => "（English）", "ja" => "（日本語）", _ => "" },
        serde_json::to_string(&serde_json::json!({ "items": items })).unwrap_or_default(),
    );
    let out = tasks::run_llm_text(&state.client, &base_url, &model, &key, &prompt).await?;
    // 解析：可能包在 markdown 代码块中，先剥离 ```json … ```
    let core = out.trim();
    let core = if core.starts_with("```") {
        if let Some(p) = core.find('\n') { &core[p + 1..] } else { core }
    } else { core };
    let core = core.trim_end_matches("```").trim();
    let parsed: Vec<serde_json::Value> = serde_json::from_str(core).map_err(|e| format!("LLM 返回非 JSON: {e}；raw={}", &core.chars().take(200).collect::<String>()))?;
    let _ = project_id; // 暂不落库
    Ok(serde_json::to_string(&parsed).unwrap_or_else(|_| "[]".into()))
}

/// 导出剪映草稿：生成 JianyingPro/Drafts/<project>_<ts>/draft_content.json + draft_meta_info.json + draft_extra_info.json + 素材副本。
#[tauri::command(rename_all = "camelCase")]
pub async fn film_jianying_draft(
    state: State<'_, AppState>,
    project_id: String,
    script: String,
    video_path: String,
    range_start: f64,
    range_end: f64,
    out_dir: Option<String>,
    on_progress: Channel<ProgressMsg>,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::task_create(&state.pool, &id, "film_jianying_draft", Some(&project_id)).await?;
    let job = TaskJob {
        id: id.clone(),
        kind: "film_jianying_draft".into(),
        project_id: Some(project_id.clone()),
        payload: serde_json::json!({
            "projectId": project_id,
            "script": script,
            "videoPath": video_path,
            "rangeStart": range_start,
            "rangeEnd": range_end,
            "outDir": out_dir,
        }),
        channel: on_progress,
    };
    state.task_tx.send(job).await.map_err(|e| e.to_string())?;
    Ok(id)
}

/// 导入字幕（SRT/VTT/JSON）。同步返回解析结果。
/// SRT 格式：
///   <index>\n<hh:mm:ss,mmm> --> <hh:mm:ss,mmm>\n<text...>\n
/// JSON 格式：[{"start":float,"end":float,"text":"..."}, ...]
#[tauri::command(rename_all = "camelCase")]
pub async fn import_script(file_path: String) -> Result<String, String> {
    let raw = std::fs::read_to_string(&file_path).map_err(|e| format!("读取字幕失败：{e}"))?;
    let lower = file_path.to_lowercase();
    let segments: Vec<serde_json::Value> = if lower.ends_with(".json") {
        serde_json::from_str::<Vec<serde_json::Value>>(&raw)
            .map_err(|e| format!("JSON 解析失败：{e}"))?
    } else {
        // 默认按 SRT 解析（VTT 类似，仅 ignore "WEBVTT" 头）
        let cleaned = raw.replace("WEBVTT", "").replace("\r\n", "\n");
        let mut out: Vec<serde_json::Value> = Vec::new();
        for block in cleaned.split("\n\n") {
            let lines: Vec<&str> = block.lines().filter(|l| !l.trim().is_empty()).collect();
            if lines.is_empty() { continue; }
            // 找带 "-->" 的时间行
            let mut time_line_idx = None;
            for (i, l) in lines.iter().enumerate() {
                if l.contains("-->") { time_line_idx = Some(i); break; }
            }
            let Some(idx) = time_line_idx else { continue; };
            let tl = lines[idx];
            let ts: Vec<&str> = tl.split("-->").collect();
            if ts.len() != 2 { continue; }
            let parse_ts = |s: &str| -> Option<f64> {
                // 支持 hh:mm:ss.mmm 或 hh:mm:ss,mmm 或 mm:ss.mmm
                let s = s.trim();
                let s = s.replace(',', ".");
                let parts: Vec<&str> = s.split(':').collect();
                match parts.len() {
                    3 => {
                        let h: f64 = parts[0].parse().ok()?;
                        let m: f64 = parts[1].parse().ok()?;
                        let ss: f64 = parts[2].parse().ok()?;
                        Some(h * 3600.0 + m * 60.0 + ss)
                    }
                    2 => {
                        let m: f64 = parts[0].parse().ok()?;
                        let ss: f64 = parts[1].parse().ok()?;
                        Some(m * 60.0 + ss)
                    }
                    _ => None,
                }
            };
            let start = match parse_ts(ts[0]) { Some(v) => v, None => continue };
            let end = match parse_ts(ts[1]) { Some(v) => v, None => continue };
            let text = lines.iter().skip(idx + 1).map(|s| *s).collect::<Vec<_>>().join("\n").trim().to_string();
            if text.is_empty() { continue; }
            out.push(serde_json::json!({ "start": start, "end": end, "text": text }));
        }
        out
    };
    Ok(serde_json::to_string(&serde_json::json!({ "segments": segments })).unwrap_or_else(|_| "{\"segments\":[]}".into()))
}

/// 导入配音：调用 transcribe_local 对 wav/mp3 整体转写。
/// 同步阻塞（Python 子进程）。
#[tauri::command(rename_all = "camelCase")]
pub async fn import_audio_dub(
    _state: State<'_, AppState>,
    _project_id: String,
    file_path: String,
) -> Result<String, String> {
    if !std::path::Path::new(&file_path).exists() { return Err(format!("音频不存在：{file_path}")); }
    let (segs, _lang, _dur, _deg, reason) = tasks::transcribe_local(&file_path).await;
    if segs.is_empty() {
        return Err(format!("本地转写失败：{reason}"));
    }
    Ok(serde_json::to_string(&serde_json::json!({ "segments": segs.iter().map(|s| serde_json::json!({ "start": s.start, "end": s.end, "text": s.text })).collect::<Vec<_>>() })).unwrap_or_else(|_| "{\"segments\":[]}".into()))
}

/// 导出 Premiere：生成 data_dir/premier_drafts/<project>_<ts>/{project}.edl + .timeline.json + subtitles.srt。
#[tauri::command(rename_all = "camelCase")]
pub async fn film_premiere_export(
    state: State<'_, AppState>,
    project_id: String,
    script: String,
    video_path: String,
    range_start: f64,
    range_end: f64,
    follow_original: bool,
    flower_text: bool,
    strict_align: bool,
    out_dir: Option<String>,
    on_progress: Channel<ProgressMsg>,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::task_create(&state.pool, &id, "film_premiere_export", Some(&project_id)).await?;
    let job = TaskJob {
        id: id.clone(),
        kind: "film_premiere_export".into(),
        project_id: Some(project_id.clone()),
        payload: serde_json::json!({
            "projectId": project_id,
            "script": script,
            "videoPath": video_path,
            "rangeStart": range_start,
            "rangeEnd": range_end,
            "followOriginal": follow_original,
            "flowerText": flower_text,
            "strictAlign": strict_align,
            "outDir": out_dir,
        }),
        channel: on_progress,
    };
    state.task_tx.send(job).await.map_err(|e| e.to_string())?;
    Ok(id)
}

/// 导出国际剪映（CapCut 兼容）：同 jianying draft 路径，但 media path 用绝对路径 + 国际字段。
#[tauri::command(rename_all = "camelCase")]
pub async fn film_jianying_draft_intl(
    state: State<'_, AppState>,
    project_id: String,
    script: String,
    video_path: String,
    range_start: f64,
    range_end: f64,
    follow_original: bool,
    flower_text: bool,
    strict_align: bool,
    out_dir: Option<String>,
    on_progress: Channel<ProgressMsg>,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::task_create(&state.pool, &id, "film_jianying_draft_intl", Some(&project_id)).await?;
    let job = TaskJob {
        id: id.clone(),
        kind: "film_jianying_draft_intl".into(),
        project_id: Some(project_id.clone()),
        payload: serde_json::json!({
            "projectId": project_id,
            "script": script,
            "videoPath": video_path,
            "rangeStart": range_start,
            "rangeEnd": range_end,
            "followOriginal": follow_original,
            "flowerText": flower_text,
            "strictAlign": strict_align,
            "outDir": out_dir,
        }),
        channel: on_progress,
    };
    state.task_tx.send(job).await.map_err(|e| e.to_string())?;
    Ok(id)
}

/// 渲染预览成片：源视频 + 分段配音 + 烧录字幕 合成到 data_dir/preview/<safe>.mp4。
/// payload 回传 { outPath }。
#[tauri::command(rename_all = "camelCase")]
pub async fn film_render_preview(
    state: State<'_, AppState>,
    project_id: String,
    script: String,
    video_path: String,
    mix_voice: bool,
    subtitle_style: String,
    out_dir: Option<String>,
    on_progress: Channel<ProgressMsg>,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::task_create(&state.pool, &id, "film_render_preview", Some(&project_id)).await?;
    let job = TaskJob {
        id: id.clone(),
        kind: "film_render_preview".into(),
        project_id: Some(project_id.clone()),
        payload: serde_json::json!({
            "projectId": project_id,
            "script": script,
            "videoPath": video_path,
            "mixVoice": mix_voice,
            "subtitleStyle": subtitle_style,
            "outDir": out_dir,
        }),
        channel: on_progress,
    };
    state.task_tx.send(job).await.map_err(|e| e.to_string())?;
    Ok(id)
}

/// 导出成片 MP4：与预览相同管线，但输出到 data_dir/export/<safe>_<ts>.mp4。
/// payload 回传 { outPath }。
#[tauri::command(rename_all = "camelCase")]
pub async fn film_export_final(
    state: State<'_, AppState>,
    project_id: String,
    script: String,
    video_path: String,
    mix_voice: bool,
    subtitle_style: String,
    out_dir: Option<String>,
    on_progress: Channel<ProgressMsg>,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::task_create(&state.pool, &id, "film_export_final", Some(&project_id)).await?;
    let job = TaskJob {
        id: id.clone(),
        kind: "film_export_final".into(),
        project_id: Some(project_id.clone()),
        payload: serde_json::json!({
            "projectId": project_id,
            "script": script,
            "videoPath": video_path,
            "mixVoice": mix_voice,
            "subtitleStyle": subtitle_style,
            "outDir": out_dir,
        }),
        channel: on_progress,
    };
    state.task_tx.send(job).await.map_err(|e| e.to_string())?;
    Ok(id)
}

/// 导出 SRT：把前端生成好的字幕文本写入指定文件夹（outDir）下的 <project>.srt。
/// payload 回传 { outPath }。
#[tauri::command(rename_all = "camelCase")]
pub async fn film_export_srt(
    state: State<'_, AppState>,
    project_id: String,
    content: String,
    out_dir: Option<String>,
    on_progress: Channel<ProgressMsg>,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::task_create(&state.pool, &id, "film_export_srt", Some(&project_id)).await?;
    let job = TaskJob {
        id: id.clone(),
        kind: "film_export_srt".into(),
        project_id: Some(project_id.clone()),
        payload: serde_json::json!({
            "projectId": project_id,
            "content": content,
            "outDir": out_dir,
        }),
        channel: on_progress,
    };
    state.task_tx.send(job).await.map_err(|e| e.to_string())?;
    Ok(id)
}
