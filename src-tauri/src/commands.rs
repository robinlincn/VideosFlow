// VideosFlow — Tauri 命令实现
// M0：ping / provider 读写 / 连接测试 / 任务提交 / 任务状态。
// M2：film_category_* / film_project_* / film_timeline_save / film_timeline_load /
//     film_import / film_smart_cut / film_export（后三者内部提交异步任务并经 Channel 推进度）。

use serde::Deserialize;
use tauri::ipc::Channel;
use tauri::State;

use crate::{cred, db, python, tasks::ProgressMsg, tasks::TaskJob, AppState};

#[tauri::command]
pub fn ping() -> String {
    "pong".into()
}

#[tauri::command]
pub async fn provider_list(state: State<'_, AppState>) -> Result<Vec<db::ProviderRow>, String> {
    let mut rows = db::list(&state.pool).await?;
    for r in rows.iter_mut() {
        r.has_key = cred::has_key(&r.kind);
    }
    Ok(rows)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUpsert {
    pub kind: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub model: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[tauri::command]
pub async fn provider_upsert(state: State<'_, AppState>, p: ProviderUpsert) -> Result<(), String> {
    db::upsert(
        &state.pool,
        &p.kind,
        &p.name,
        &p.provider,
        &p.base_url,
        &p.model,
        p.enabled,
    )
    .await
}

#[tauri::command]
pub async fn provider_key_set(kind: String, key: String) -> Result<(), String> {
    cred::set_key(&kind, &key)
}

#[tauri::command]
pub async fn provider_key_get(kind: String) -> Result<Option<String>, String> {
    cred::get_key(&kind)
}

#[tauri::command]
pub async fn provider_test(state: State<'_, AppState>, kind: String) -> Result<String, String> {
    let row = db::get_by_kind(&state.pool, &kind).await?;
    let key = cred::get_key(&kind)?;
    let cfg = python::build_cfg(&row, key);
    let env = if kind == "llm" {
        python::call_chat(&state.client, state.sidecar_port, &cfg, "ping", 1).await?
    } else {
        python::call_test(&state.client, state.sidecar_port, &cfg).await?
    };
    if env.ok {
        Ok("ok".into())
    } else {
        Err(env.message)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSubmit {
    pub kind: String,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub payload: serde_json::Value,
    pub on_progress: Channel<ProgressMsg>,
}

#[tauri::command]
pub async fn task_submit(state: State<'_, AppState>, req: TaskSubmit) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::task_create(&state.pool, &id, &req.kind, req.project_id.as_deref()).await?;
    let job = TaskJob {
        id: id.clone(),
        kind: req.kind,
        project_id: req.project_id,
        payload: req.payload,
        channel: req.on_progress,
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
    db::film_category_reorder(&state.pool, &id, &order).await
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilmImportReq {
    pub project_id: String,
    pub video_path: String,
    pub script: String,
    pub on_progress: Channel<ProgressMsg>,
}

#[tauri::command]
pub async fn film_import(state: State<'_, AppState>, req: FilmImportReq) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::task_create(&state.pool, &id, "film_import", Some(&req.project_id)).await?;
    let job = TaskJob {
        id: id.clone(),
        kind: "film_import".into(),
        project_id: Some(req.project_id.clone()),
        payload: serde_json::json!({
            "projectId": req.project_id,
            "videoPath": req.video_path,
            "script": req.script,
        }),
        channel: req.on_progress,
    };
    state.task_tx.send(job).await.map_err(|e| e.to_string())?;
    Ok(id)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilmSmartCutReq {
    pub project_id: String,
    pub script: String,
    pub on_progress: Channel<ProgressMsg>,
}

#[tauri::command]
pub async fn film_smart_cut(state: State<'_, AppState>, req: FilmSmartCutReq) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::task_create(&state.pool, &id, "film_smart_cut", Some(&req.project_id)).await?;
    let job = TaskJob {
        id: id.clone(),
        kind: "film_smart_cut".into(),
        project_id: Some(req.project_id.clone()),
        payload: serde_json::json!({
            "projectId": req.project_id,
            "script": req.script,
        }),
        channel: req.on_progress,
    };
    state.task_tx.send(job).await.map_err(|e| e.to_string())?;
    Ok(id)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilmExportReq {
    pub project_id: String,
    #[serde(default)]
    pub hw: bool,
    #[serde(default)]
    pub resolution: String,
    #[serde(default = "default_true")]
    pub burn_sub: bool,
    #[serde(default)]
    pub mix_voice: bool,
    #[serde(default)]
    pub voice_mix: f64,
    #[serde(default)]
    pub script: String,
    pub on_progress: Channel<ProgressMsg>,
}

#[tauri::command]
pub async fn film_export(state: State<'_, AppState>, req: FilmExportReq) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::task_create(&state.pool, &id, "film_export", Some(&req.project_id)).await?;
    let job = TaskJob {
        id: id.clone(),
        kind: "film_export".into(),
        project_id: Some(req.project_id.clone()),
        payload: serde_json::json!({
            "projectId": req.project_id,
            "hw": req.hw,
            "resolution": req.resolution,
            "burnSub": req.burn_sub,
            "mixVoice": req.mix_voice,
            "voiceMix": req.voice_mix,
            "script": req.script,
        }),
        channel: req.on_progress,
    };
    state.task_tx.send(job).await.map_err(|e| e.to_string())?;
    Ok(id)
}
