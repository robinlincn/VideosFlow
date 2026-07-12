// VideosFlow — Tauri 命令实现
// M0：ping / provider 读写 / 连接测试 / 任务提交 / 任务状态。
// M2：film_category_* / film_project_* / film_timeline_save / film_timeline_load /
//     film_import / film_smart_cut / film_export（后三者内部提交异步任务并经 Channel 推进度）。

use serde::Deserialize;
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::State;

use crate::{cred, db, tasks::ProgressMsg, tasks::TaskJob, AppState};

#[tauri::command]
pub fn ping() -> String {
    "pong".into()
}

#[tauri::command]
pub async fn provider_list(state: State<'_, AppState>) -> Result<Vec<db::ProviderRow>, String> {
    let mut rows = db::list(&state.pool).await?;
    // DB 的 has_key 列为权威标记（密钥写入成功即置位）；仅在 DB 为 false 时回退凭据库探测，
    // 兼容历史已存密钥，且避免 Windows 凭据库回读不稳导致 UI 误报「尚未保存」。
    for r in rows.iter_mut() {
        if !r.has_key {
            r.has_key = cred::has_key(&r.kind);
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
        // 清空密钥：从凭据库删除，并把 DB 标记复位
        let _ = cred::delete_key(&kind);
        db::set_has_key(&state.pool, &kind, false).await?;
        return Ok(());
    }
    // 写入系统凭据库；成功后以 DB 布尔标记记录「已保存」，作为 UI 提示权威来源。
    // 密钥本体始终只存凭据库，绝不落 SQLite 明文（安全红线）。
    cred::set_key(&kind, &key)?;
    db::set_has_key(&state.pool, &kind, true).await?;
    Ok(())
}

#[tauri::command]
pub async fn provider_key_get(kind: String) -> Result<Option<String>, String> {
    cred::get_key(&kind)
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
    let stored = cred::get_key(&kind)?;
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
