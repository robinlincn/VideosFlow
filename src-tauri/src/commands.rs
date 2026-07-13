// VideosFlow — Tauri 命令实现
// M0：ping / provider 读写 / 连接测试 / 任务提交 / 任务状态。
// M2：film_category_* / film_project_* / film_timeline_save / film_timeline_load /
//     film_import / film_smart_cut / film_export（后三者内部提交异步任务并经 Channel 推进度）。

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
) -> Result<(), String> {
    db::upsert(&state.pool, &kind, &name, &provider, &base_url, &model, enabled).await
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

/// 连接测试：Rust 端直接用 reqwest 探测 Base URL 可达性 + 鉴权有效性，
/// 不再依赖 Python sidecar（sidecar 仅在口播/创作模块运行时启用，设置页测试不应被其阻塞）。
/// `api_key` 为可选：传入时优先用「正在填写、尚未保存」的 Key 直接测，无需先点保存。
#[tauri::command(rename_all = "camelCase")]
pub async fn provider_test(
    state: State<'_, AppState>,
    kind: String,
    api_key: Option<String>,
) -> Result<String, String> {
    let row = db::get_by_kind(&state.pool, &kind).await?;
    let stored = cred::get_key(&state.pool, &kind).await?;
    let key = api_key
        .filter(|k| !k.is_empty())
        .or_else(|| stored.filter(|k| !k.is_empty()));
    let base = row.base_url.trim().to_string();
    if base.is_empty() {
        return Err("请先填写 Base URL 再测试连接".into());
    }
    let client = &state.client;
    let has_key = key.is_some();
    // 优先尝试 OpenAI 兼容的 /models 端点（可同时验证鉴权有效性）
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

#[tauri::command(rename_all = "camelCase")]
pub async fn submit_film_script_gen(
    state: State<'_, AppState>,
    project_id: String,
    on_progress: Channel<ProgressMsg>,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::task_create(&state.pool, &id, "film_script_gen", Some(&project_id)).await?;
    let job = TaskJob {
        id: id.clone(),
        kind: "film_script_gen".into(),
        project_id: Some(project_id.clone()),
        payload: serde_json::json!({ "projectId": project_id }),
        channel: on_progress,
    };
    state.task_tx.send(job).await.map_err(|e| e.to_string())?;
    Ok(id)
}
