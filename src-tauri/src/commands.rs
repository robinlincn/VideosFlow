// VideosFlow — Tauri 命令实现（M0）
// ping / provider 读写 / 连接测试 / 任务提交 / 任务状态。
// 进度通过 on_progress: Channel<ProgressMsg> 实时推回前端（Tauri 2 原生 Channel，替代裸 WS）。

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
    let env = python::call_test(&state.client, state.sidecar_port, &cfg).await?;
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
