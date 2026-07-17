// VideosFlow — Tauri2 应用入口
// M0 基础设施：SQLite 建表 + Python sidecar 守护 + 任务队列 + IPC 命令 + 进度 Channel。
// M2：注册 film_* 命令。

mod commands;
mod cred;
mod db;
mod ffmpeg;
mod fileserver;
mod tasks;

use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use tauri::Manager;

pub struct AppState {
    pub pool: SqlitePool,
    pub client: reqwest::Client,
    pub task_tx: tasks::TaskSender,
    pub sidecar_port: u16,
    pub data_dir: std::path::PathBuf,
    pub models_dir: std::path::PathBuf,
    pub video_server_port: u16,
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
                // 本地模型目录：默认指向「本项目根目录下的 models」（用户要求下载到项目内，便于版本管理与直接查看）；
                // 可用环境变量 VF_MODELS_DIR 覆盖（生产打包后项目根不存在时使用）。
                let models_dir = if let Ok(p) = std::env::var("VF_MODELS_DIR") {
                    std::path::PathBuf::from(p)
                } else {
                    std::env::current_dir()
                        .map(|d| d.join("models"))
                        .unwrap_or_else(|_| data_dir.join("models"))
                };
                std::fs::create_dir_all(&models_dir).ok();

                // SQLite 连接 + 首次建表/种子
                let db_path = data_dir.join("videosflow.db");
                let pool = SqlitePoolOptions::new()
                    .max_connections(5)
                    .connect(&format!("sqlite://{}?mode=rwc", db_path.display()))
                    .await
                    .map_err(|e| format!("SQLite 连接失败: {e}"))?;
                db::init(&pool).await?;

                // HTTP 客户端（直连各云网关：Agnes 对话 / XiaomiMimo ASR·TTS）
                let client = reqwest::Client::new();

                // 影片 ASR/TTS 已改为 Rust reqwest 直连 XiaomiMimo，不再启动 Python sidecar。
                let port = 8731u16; // 仅保留给任务队列签名兼容（不再用于 sidecar）

                // 任务队列 + worker
                let (tx, rx) = tokio::sync::mpsc::channel::<tasks::TaskJob>(32);
                tasks::start(pool.clone(), client.clone(), port, data_dir.clone(), models_dir.clone(), rx);

                // 本地视频预览文件服务器（127.0.0.1，支持 Range，供 WebView <video> 播放）
                let video_port = fileserver::start();

                handle.manage(AppState {
                    pool,
                    client,
                    task_tx: tx,
                    sidecar_port: port,
                    data_dir,
                    models_dir,
                    video_server_port: video_port,
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
            commands::get_models_dir,
            commands::download_model,
            commands::check_local_model,
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
            // ---- M3 spoken 命令 ----
            commands::spoken_video_list,
            commands::spoken_video_create,
            commands::spoken_video_delete,
            commands::spoken_video_get,
            commands::spoken_extract_script,
            commands::spoken_edits_list,
            commands::spoken_edits_set_accepted,
            commands::spoken_apply_edits,
            commands::spoken_assets_list,
            commands::spoken_asset_create,
            commands::spoken_asset_delete,
            commands::spoken_keywords_list,
            commands::spoken_matches_list,
            commands::spoken_match_toggle,
            commands::spoken_asr,
            commands::spoken_detect,
            commands::spoken_keyword,
            commands::spoken_match_assets,
            commands::spoken_burn,
            commands::spoken_export,
            // ---- M4 creation 命令 ----
            commands::creation_project_list,
            commands::creation_project_get,
            commands::creation_project_create,
            commands::creation_project_update,
            commands::creation_project_delete,
            commands::storyboard_get,
            commands::storyboard_save,
            commands::generated_assets_list,
            commands::submit_script_write,
            commands::submit_script_humanize,
            commands::submit_storyboard_gen,
            commands::submit_image_gen,
            commands::submit_film_script_gen,
            commands::submit_film_video_analysis,
            commands::get_film_analysis,
            commands::get_video_server_url,
            commands::voice_list,
            commands::batch_dub,
            commands::translate_script,
            commands::film_jianying_draft,
            commands::import_script,
            commands::import_audio_dub,
            commands::film_premiere_export,
            commands::film_jianying_draft_intl,
            commands::film_render_preview,
            commands::film_export_final,
            commands::film_export_srt,
        ])
        .run(tauri::generate_context!())
        .expect("error while running VideosFlow");
}
