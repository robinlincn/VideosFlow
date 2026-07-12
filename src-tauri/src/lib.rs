// VideosFlow — Tauri2 应用入口
// M0 基础设施：SQLite 建表 + Python sidecar 守护 + 任务队列 + IPC 命令 + 进度 Channel。
// M2：注册 film_* 命令。

mod commands;
mod cred;
mod db;
mod ffmpeg;
mod python;
mod tasks;

use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use tauri::Manager;

pub struct AppState {
    pub pool: SqlitePool,
    pub client: reqwest::Client,
    pub task_tx: tasks::TaskSender,
    pub sidecar_port: u16,
    pub data_dir: std::path::PathBuf,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let result: Result<(), String> = tauri::async_runtime::block_on(async move {
                // 数据目录（工程库 / sidecar 缓存 / FFmpeg 缓存）
                let data_dir = handle.path().app_data_dir().map_err(|e| e.to_string())?;
                std::fs::create_dir_all(&data_dir).ok();

                // SQLite 连接 + 首次建表/种子
                let db_path = data_dir.join("videosflow.db");
                let pool = SqlitePoolOptions::new()
                    .max_connections(5)
                    .connect(&format!("sqlite://{}?mode=rwc", db_path.display()))
                    .await
                    .map_err(|e| format!("SQLite 连接失败: {e}"))?;
                db::init(&pool).await?;

                // HTTP 客户端（调用 sidecar）
                let client = reqwest::Client::new();

                // Python sidecar（best-effort，启动失败不阻塞主程序）
                let port = python::DEFAULT_PORT;
                let _guard = python::spawn_sidecar(
                    handle
                        .path()
                        .resource_dir()
                        .map(|p| p.to_path_buf())
                        .as_deref()
                        .unwrap_or(&data_dir),
                    port,
                );

                // 任务队列 + worker
                let (tx, rx) = tokio::sync::mpsc::channel::<tasks::TaskJob>(32);
                tasks::start(pool.clone(), client.clone(), port, data_dir.clone(), rx);

                handle.manage(AppState {
                    pool,
                    client,
                    task_tx: tx,
                    sidecar_port: port,
                    data_dir,
                });
                Ok(())
            });
            result?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::provider_list,
            commands::provider_upsert,
            commands::provider_key_set,
            commands::provider_key_get,
            commands::provider_test,
            commands::task_submit,
            commands::task_status,
            // ---- M2 film 命令 ----
            commands::film_category_list,
            commands::film_category_create,
            commands::film_category_rename,
            commands::film_category_reorder,
            commands::film_category_delete,
            commands::film_project_list,
            commands::film_project_create,
            commands::film_project_update,
            commands::film_project_delete,
            commands::film_timeline_load,
            commands::film_timeline_save,
            commands::film_import,
            commands::film_smart_cut,
            commands::film_export,
        ])
        .run(tauri::generate_context!())
        .expect("error while running VideosFlow");
}
