// VideosFlow — 任务队列 + 进度广播（tokio mpsc + Tauri Channel）
// M0：worker 消费任务，做 sidecar 健康检查并广播进度，最终持久化状态到 tasks 表。
// M2：新增 film_import / film_smart_cut / film_export 分支（确定性纯算法 + ffmpeg）。

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tokio::sync::mpsc;

use std::path::{Path, PathBuf};
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

// ===========================================================================
// M5：创作工程「产物清单」持久化（clips / audios / tails / exported）
// 落在 data_dir/creation_manifest/<safe>.json，与 storyboards 解耦，避免覆盖用户分镜编辑。
// ===========================================================================

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CreationManifest {
    /// 逐镜视频片段路径（frames 步产出）：{ "<shotIndex>": "<abs path>" }
    #[serde(default)]
    pub clips: std::collections::HashMap<String, String>,
    /// 逐镜配音 wav 路径（voice 步产出）：{ "<shotIndex>": "<abs path>" }
    #[serde(default)]
    pub audios: std::collections::HashMap<String, String>,
    /// 逐镜尾帧图路径（frames 步可选上传）：{ "<shotIndex>": "<abs path>" }
    #[serde(default)]
    pub tails: std::collections::HashMap<String, String>,
    /// 最终导出成片路径（export 步产出），未导出为 null
    #[serde(default)]
    pub exported: Option<String>,
}

fn creation_manifest_path(data_dir: &Path, project_id: &str) -> PathBuf {
    let safe = sanitize_filename(project_id);
    data_dir.join("creation_manifest").join(format!("{safe}.json"))
}

/// 读取产物清单；文件不存在或损坏时返回空清单（不报错），保证流程可继续。
pub fn read_creation_manifest(data_dir: &Path, project_id: &str) -> CreationManifest {
    let p = creation_manifest_path(data_dir, project_id);
    match std::fs::read_to_string(&p) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => CreationManifest::default(),
    }
}

/// 写回产物清单（覆盖式）。目录不存在时自动创建。
pub fn write_creation_manifest(data_dir: &Path, project_id: &str, m: &CreationManifest) -> Result<(), String> {
    let p = creation_manifest_path(data_dir, project_id);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let s = serde_json::to_string_pretty(m).map_err(|e| e.to_string())?;
    std::fs::write(&p, s).map_err(|e| e.to_string())?;
    Ok(())
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
                    eprintln!("[film-script-gen] done emitted, script_len={}", script.len());
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
        "batch_dub" => {
            match run_batch_dub(pool, client, data_dir, &job, emit.clone()).await {
                Ok(()) => {
                    db::task_update(pool, &job.id, "done", 100.0, "批量配音完成").await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "done".into(),
                        message: Some("批量配音完成".into()),
                        payload: None,
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
        "film_jianying_draft" => {
            match run_film_jianying_draft(pool, client, data_dir, &job, emit.clone()).await {
                Ok(draft_dir) => {
                    db::task_update(pool, &job.id, "done", 100.0, &format!("剪映草稿已生成：{draft_dir}")).await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "done".into(),
                        message: Some(format!("剪映草稿已生成：{draft_dir}")),
                        payload: Some(serde_json::json!({ "draftDir": draft_dir })),
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
        "film_jianying_draft_intl" => {
            match run_film_jianying_draft_intl(pool, client, data_dir, &job, emit.clone()).await {
                Ok(draft_dir) => {
                    db::task_update(pool, &job.id, "done", 100.0, &format!("国际剪映草稿已生成：{draft_dir}")).await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "done".into(),
                        message: Some(format!("国际剪映草稿已生成：{draft_dir}")),
                        payload: Some(serde_json::json!({ "draftDir": draft_dir })),
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
        "film_premiere_export" => {
            match run_film_premiere_export(pool, data_dir, &job, emit.clone()).await {
                Ok(out_dir) => {
                    db::task_update(pool, &job.id, "done", 100.0, &format!("Premiere 导出完成：{out_dir}")).await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "done".into(),
                        message: Some(format!("Premiere 导出完成：{out_dir}")),
                        payload: Some(serde_json::json!({ "outDir": out_dir })),
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
        "film_render_preview" => {
            match run_film_render_preview(pool, client, data_dir, &job, emit.clone()).await {
                Ok(out_path) => {
                    db::task_update(pool, &job.id, "done", 100.0, "预览成片已生成").await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "done".into(),
                        message: Some("预览成片已生成".into()),
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
        "film_export_final" => {
            match run_film_export_final(pool, client, data_dir, &job, emit.clone()).await {
                Ok(out_path) => {
                    db::task_update(pool, &job.id, "done", 100.0, "成片 MP4 已导出").await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "done".into(),
                        message: Some("成片 MP4 已导出".into()),
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
        "film_export_srt" => {
            match run_film_export_srt(pool, data_dir, &job, emit.clone()).await {
                Ok(out_path) => {
                    db::task_update(pool, &job.id, "done", 100.0, &format!("SRT 已导出：{out_path}")).await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 100.0,
                        status: "done".into(),
                        message: Some(format!("SRT 已导出：{out_path}")),
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
        "creation_frames" => {
            match run_creation_frames(pool, data_dir, &job, emit.clone()).await {
                Ok(()) => {
                    db::task_update(pool, &job.id, "done", 100.0, "首尾帧视频生成完成").await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(), progress: 100.0, status: "done".into(),
                        message: Some("首尾帧视频生成完成".into()), payload: None,
                    });
                }
                Err(e) => {
                    db::task_update(pool, &job.id, "failed", 100.0, &e).await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(), progress: 100.0, status: "failed".into(),
                        message: Some(e), payload: None,
                    });
                }
            }
            return;
        }
        "creation_voice" => {
            match run_creation_voice(pool, client, data_dir, &job, emit.clone()).await {
                Ok(()) => {
                    db::task_update(pool, &job.id, "done", 100.0, "配音生成完成").await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(), progress: 100.0, status: "done".into(),
                        message: Some("配音生成完成".into()), payload: None,
                    });
                }
                Err(e) => {
                    db::task_update(pool, &job.id, "failed", 100.0, &e).await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(), progress: 100.0, status: "failed".into(),
                        message: Some(e), payload: None,
                    });
                }
            }
            return;
        }
        "creation_export" => {
            match run_creation_export(pool, data_dir, &job, emit.clone()).await {
                Ok(out_path) => {
                    db::task_update(pool, &job.id, "done", 100.0, "成片导出完成").await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(), progress: 100.0, status: "done".into(),
                        message: Some("成片导出完成".into()),
                        payload: Some(serde_json::json!({ "outPath": out_path })),
                    });
                }
                Err(e) => {
                    db::task_update(pool, &job.id, "failed", 100.0, &e).await.ok();
                    emit(ProgressMsg {
                        task_id: job.id.clone(), progress: 100.0, status: "failed".into(),
                        message: Some(e), payload: None,
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
    let py = if let Ok(p) = std::env::var("VF_PYTHON") {
        p
    } else {
        let candidates: [std::path::PathBuf; 4] = [
            std::path::PathBuf::from("C:/Users/csit/.workbuddy/binaries/python/envs/default/Scripts/python.exe"),
            std::path::PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default())
                .join(".workbuddy/binaries/python/envs/default/Scripts/python.exe"),
            std::path::PathBuf::from("/c/Users/csit/.workbuddy/binaries/python/envs/default/Scripts/python.exe"),
            std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
                .join(".workbuddy/binaries/python/envs/default/bin/python"),
        ];
        candidates.into_iter().find(|p| p.exists())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "python".to_string())
    };
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
                Ok(o) if o.status.success() => {
                    let out = String::from_utf8_lossy(&o.stdout).to_string();
                    // 脚本异常（如缺 opencc）可能输出空串，此时回退原文，避免吞掉解说文案
                    if out.trim().is_empty() { text.to_string() } else { out }
                }
                _ => text.to_string(),
            }
        }
        Err(_) => text.to_string(),
    }
}

/// 调用 python-sidecar/transcribe.py（faster-whisper）做本地转写，返回 segments。
/// 支持环境变量 VF_PYTHON（Python 解释器路径）与 VF_TRANSCRIBE_SCRIPT（脚本路径）。
pub async fn transcribe_local(audio_path: &str) -> (Vec<db::AsrSegment>, String, f64, bool, String) {
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
    // 优先级：VF_PYTHON 环境变量 → workbuddy managed venv → PATH 中的 python
    // 系统 PATH 中的 python 通常不含 faster_whisper，优先 venv 才能跑通本地 ASR
    let py = if let Ok(p) = std::env::var("VF_PYTHON") {
        p
    } else {
        // 探测 workbuddy managed venv（Windows: Scripts\\python.exe；POSIX: bin/python）
        let candidates: [std::path::PathBuf; 4] = [
            std::path::PathBuf::from("C:/Users/csit/.workbuddy/binaries/python/envs/default/Scripts/python.exe"),
            std::path::PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default())
                .join(".workbuddy/binaries/python/envs/default/Scripts/python.exe"),
            std::path::PathBuf::from("/c/Users/csit/.workbuddy/binaries/python/envs/default/Scripts/python.exe"),
            std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
                .join(".workbuddy/binaries/python/envs/default/bin/python"),
        ];
        candidates.into_iter().find(|p| p.exists())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "python".to_string())
    };
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
        Ok(o) => {
            // 优先解析 stdout 的 {"error":"..."}（transcribe.py 异常时把错误 print 到 stdout）
            let out = String::from_utf8_lossy(&o.stdout).to_string();
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            let inner_err = serde_json::from_str::<serde_json::Value>(&out)
                .ok()
                .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(|s| s.to_string()));
            let detail = inner_err
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| if stderr.trim().is_empty() { "<无 stdout/stderr 输出>".into() } else { stderr.clone() });
            (Vec::new(), "zh".to_string(), 0.0, true,
                format!("本地推理进程失败({}): {}", o.status, detail))
        }
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
        // 与 run_llm_text 保持一致：输出 token 上限提到 8192。
        // 注意：推理类模型会把 reasoning 计入 max_tokens 总预算，1024 太小会导致
        // reasoning 吃满预算后 content 返回空字符串，进而触发「EOF while parsing」空内容解析错误。
        // 提至 8192 给足余量，分镜 JSON 4~6 镜头也远用不完。
        "max_tokens": 8192,
        "temperature": 0.3,
    });
    let resp = client
        .post(&url)
        .bearer_auth(api_key)
        .json(&body)
        .timeout(std::time::Duration::from_secs(180))
        .send()
        .await
        .map_err(|e| format!("大模型调用失败: {e}"))?;
    if !resp.status().is_success() {
        let st = resp.status();
        let txt = resp.text().await.unwrap_or_default();
        return Err(format!("大模型返回 {st}: {txt}"));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let msg = v
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"));
    let text = msg
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if text.is_empty() {
        // content 为空：dump 原始响应便于诊断（推理模型预算耗尽 / 字段错位等）
        let dump = serde_json::to_string(&v).unwrap_or_default();
        return Err(format!("大模型返回的 content 为空（原文: {}）", dump.chars().take(400).collect::<String>()));
    }
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
pub async fn llm_provider(pool: &SqlitePool) -> Result<(String, String, String), String> {
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

/// 清理大模型返回的「创作性文案」：去 ``` 代码围栏、去首行说明性前导废话、去首尾空白，只留正文。
fn clean_script(s: &str) -> String {
    let mut t = s.trim().to_string();
    // 去 ``` 围栏
    if t.starts_with("```") {
        if let Some(stripped) = t.strip_prefix("```json").or_else(|| t.strip_prefix("```")) {
            t = stripped.trim().to_string();
        }
        if let Some(end) = t.rfind("```") {
            t = t[..end].trim().to_string();
        }
    }
    // 去掉首行「说明性前导」（模型常会加「根据您的需求…」「以下是…」之类）
    let first = t.lines().next().unwrap_or("").trim().to_string();
    let meta: [&str; 10] = [
        "根据您", "根据需求", "以下是", "为您", "好的，", "好的!", "【需求", "【文案", "注：", "说明：",
    ];
    if meta.iter().any(|m| first.starts_with(m)) {
        t = t.lines().skip(1).collect::<Vec<_>>().join("\n").trim().to_string();
    }
    t.trim().to_string()
}

/// 把 LLM 返回的分镜对象规整为前端需要的 canonical 字段（兼容中英文 key，dur 强制为数字）。
fn canon_shot(s: &serde_json::Value, idx: usize) -> serde_json::Value {
    let o = s.as_object().cloned().unwrap_or_default();
    let pick = |en: &str, zh: &[&str]| -> serde_json::Value {
        if let Some(v) = o.get(en) { return v.clone(); }
        for z in zh { if let Some(v) = o.get(*z) { return v.clone(); } }
        serde_json::Value::Null
    };
    let desc = pick("desc", &["画面描述", "画面", "场景", "scene"]);
    let dialogue = pick("dialogue", &["台词", "旁白", "narration", "line"]);
    let dur_raw = pick("dur", &["时长秒", "时长", "duration"]);
    let dur = dur_raw.as_f64()
        .or_else(|| dur_raw.as_str().and_then(|x| {
            let digits: String = x.chars().filter(|c| c.is_ascii_digit() || *c == '.').collect();
            digits.parse::<f64>().ok()
        }))
        .unwrap_or(5.0);
    let cam = pick("cam", &["运镜", "运镜建议", "camera"]);
    serde_json::json!({
        "index": idx,
        "desc": desc.as_str().map(|x| x.trim().to_string()).unwrap_or_default(),
        "dialogue": dialogue.as_str().map(|x| x.trim().to_string()).unwrap_or_default(),
        "dur": dur as u32,
        "cam": cam.as_str().map(|x| x.trim().to_string()).unwrap_or_default(),
    })
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
        "你是一名资深短视频编导。下面是我给定的【创作需求 / 工程标题】，请严格以此为核心创作一份可直接配音的口播文案。\n\n\
硬性要求：\n\
1. 100% 围绕需求中的主题与具体内容展开，绝不可泛泛而谈，也绝不可偏离主题去讲别的产品、工具或平台。\n\
2. 先判断需求里是否指定了【时长】（如 30 秒 / 60 秒 / 2 分钟）、【风格】（科普 / 种草 / 教程 / 职场 / 旅行等）、【受众】或【平台】；若指定则严格遵守——时长决定文案总长度（约 4 字 / 秒）。\n\
3. 必须包含需求中明确提到的关键事物 / 方法 / 步骤的「具体内容」：例如需求点名了某个工具或技术，就要写出它的真实用途、亮点或操作步骤，而不是空话套话。\n\
4. 口语化、有画面感、有信息量；避免「大家好」「今天给大家分享」之类的套话开头，直接切入主题。\n\
5. 只输出文案正文，不要标题、不要序号、不要任何解释说明。\n\n\
【创作需求 / 工程标题】：{}",
        brief
    );
    // 注意：LLM 调用失败时如实返回错误，绝不静默降级成与标题无关的占位文案。
    let raw_script = run_llm_text(client, &base_url, &model, &key, &prompt).await
        .map_err(|e| format!("文案生成失败（大模型）：{e}"))?;
    let script = clean_script(&raw_script);
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
        "请你把以下文案改写成自然口语，去掉 AI 套话与空泛表达，加具体细节、停顿感与生活化比喻，保持原意与主题不变：\n\n{}",
        script
    );
    // 注意：LLM 调用失败时如实返回错误，绝不静默降级成与主题无关的占位文案。
    let raw_human = run_llm_text(client, &base_url, &model, &key, &prompt).await
        .map_err(|e| format!("去 AI 味失败（大模型）：{e}"))?;
    let human = clean_script(&raw_human);
    db::creation_project_update(pool, &project_id, None, None, Some(&human), Some("humanized")).await?;
    Ok(human)
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
        "请将以下文案拆为 4-6 个镜头，严格按如下 JSON 数组返回，每个对象字段固定为：\
\"desc\"（画面描述，字符串）、\"dialogue\"（台词，字符串）、\"dur\"（时长秒，纯数字如 5）、\"cam\"（运镜建议，字符串）。\
只返回 JSON 数组本身，不要任何解释、不要代码围栏：\n\n{}",
        human
    );
    // 默认风格约束卡：现实（也允许前端后续覆盖）
    let style_ref = "现实";
    let shots_json = match run_llm_json(client, &base_url, &model, &key, &prompt).await {
        Ok(arr) => {
            // 规范：为每个分镜规整为 canonical 字段（兼容中英文 key，dur 强制数字）
            let list: Vec<serde_json::Value> = arr.as_array().cloned().unwrap_or_default();
            let norm: Vec<serde_json::Value> = list.iter().enumerate().map(|(i, s)| canon_shot(s, i)).collect();
            serde_json::to_string(&norm).unwrap_or_else(|_| "[]".into())
        }
        // 注意：LLM 调用失败时如实返回错误，绝不静默降级成与主题无关的占位分镜。
        Err(e) => return Err(format!("分镜生成失败（大模型）：{e}")),
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
        .or_else(|| shots.get(shot_index as usize))
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
    eprintln!("[videosflow] image_gen POST {}/images/generations model={}", base_url, row.model);
    // 图片生成（尤其 1024x1024）可能较慢，给足超时（300s）；对超时/连接错误做 1 次重试。
    let max_attempts = 2u32;
    let mut attempt = 0u32;
    let resp = loop {
        attempt += 1;
        match client
            .post(format!("{base_url}/images/generations"))
            .bearer_auth(key.as_str())
            .json(&body)
            .timeout(std::time::Duration::from_secs(300))
            .send()
            .await
        {
            Ok(r) => break r,
            Err(e) => {
                let retryable = e.is_timeout() || e.is_connect() || e.is_request();
                let msg = format!("Agnes 图片调用失败: {e:?}");
                if attempt < max_attempts && retryable {
                    let wait = attempt as u64 * 3;
                    emit(ProgressMsg {
                        task_id: job.id.clone(),
                        progress: 35.0,
                        status: "running".into(),
                        message: Some(format!("图片接口超时，{wait}s 后第 {attempt} 次重试")),
                        payload: None,
                    });
                    tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                    continue;
                }
                return Err(msg);
            }
        }
    };
    if !resp.status().is_success() {
        let st = resp.status();
        let txt = resp.text().await.unwrap_or_default();
        return Err(format!("Agnes 图片返回 {st}: {txt}"));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    // Agnes 兼容 OpenAI 风格响应，可能返回 data[0].b64_json（base64）或 data[0].url（链接）。
    let bytes = if let Some(b64) = v.get("data")
        .and_then(|d| d.get(0))
        .and_then(|d| d.get("b64_json"))
        .and_then(|b| b.as_str())
    {
        base64::engine::general_purpose::STANDARD.decode(b64).map_err(|e| format!("base64 解码失败: {e}"))?
    } else {
        // 回退：data[0].url —— 下载图片字节写本地
        let url = v.get("data")
            .and_then(|d| d.get(0))
            .and_then(|d| d.get("url"))
            .and_then(|u| u.as_str())
            .ok_or_else(|| "Agnes 响应既缺 data[0].b64_json 也缺 data[0].url".to_string())?;
        emit(ProgressMsg {
            task_id: job.id.clone(),
            progress: 55.0,
            status: "running".into(),
            message: Some("下载生成的图片".into()),
            payload: None,
        });
        client
            .get(url)
            .timeout(std::time::Duration::from_secs(300))
            .send()
            .await
            .map_err(|e| format!("下载图片失败: {e}"))?
            .bytes()
            .await
            .map_err(|e| format!("读取图片字节失败: {e}"))?
            .to_vec()
    };

    emit(ProgressMsg {
        task_id: job.id.clone(),
        progress: 70.0,
        status: "running".into(),
        message: Some("写入本地文件".into()),
        payload: None,
    });
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
pub async fn run_llm_text(
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
        // 输出 token 上限：原为 1024，导致「解说词初稿」等长文本被硬截断。
        // 提升到 8192（约 5000-6000 中文字，20x+ 需求量），彻底消除截断。
        // 注意：单位是输出 token 而非字符，模型 API 对此有硬上限（常见 4K~16K），
        // 设置过高（如百万级）会被 API 直接拒绝并触发上层静默回退，故取通用安全的 8192。
        "max_tokens": 8192,
        "temperature": 0.7,
    });
    let resp = client
        .post(&url)
        .bearer_auth(api_key)
        .json(&body)
        .timeout(std::time::Duration::from_secs(180))
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
    let text = text.trim();
    if text.is_empty() {
        // 内容为空字符串（HTTP 200 但 content 为空）按错误处理，让上层走降级而非把空文案当真
        return Err("大模型返回的 content 为空字符串".to_string());
    }
    Ok(text.to_string())
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
/// 解析多模态视觉返回的「分段 JSON」：[{start,end,desc}]，过滤非法/越界项，至少 2 段才采用。
fn parse_vision_segments(text: &str, dur: f64) -> Option<Vec<(f64, f64, String)>> {
    let s = text.trim();
    let a = s.find('[')?;
    let b = s.rfind(']')?;
    let arr = &s[a..=b];
    let v: serde_json::Value = serde_json::from_str(arr).ok()?;
    let arr = v.as_array()?;
    let mut out: Vec<(f64, f64, String)> = Vec::new();
    for item in arr {
        let st = item.get("start").and_then(|x| x.as_f64())?;
        let en = item.get("end").and_then(|x| x.as_f64())?;
        let desc = item.get("desc").and_then(|x| x.as_str()).unwrap_or("").to_string();
        if en > st && st >= -0.5 && en <= dur + 0.5 {
            out.push((st.max(0.0), en.min(dur), desc));
        }
    }
    if out.len() >= 2 { Some(out) } else { None }
}

/// 基于已抽取帧做视觉分割：让多模态模型给出真实场景切分（相对片段的 0 基时间）+ 每段描述。
/// 返回段列表；任何失败返回空（调用方退化为其它方式）。
async fn vision_segment_frames(
    frames: &[std::path::PathBuf],
    dur_seg: f64,
    pool: &SqlitePool,
    client: &Client,
) -> Vec<(f64, f64, String)> {
    let sample = base64_frames(frames, 12);
    if sample.is_empty() { return Vec::new(); }
    let (base_url, model, key) = match llm_provider(pool).await {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let n_seg = ((dur_seg / 8.0).ceil()).clamp(4.0, 16.0) as usize;
    let prompt = format!(
        "以下是影片片段按时间顺序均匀抽取的若干关键帧（已内联为图片，顺序即时间顺序）。请：\n\
         1) 识别画面中的真实场景切换点；\n\
         2) 将片段划分为若干语义段落，每段对应连续且内容相近的画面；\n\
         3) 对每一段给出详细的中文内容描述（40-60字）：说明「画面里的主体是谁/是什么、在做什么动作、情绪或氛围如何、有哪些关键画面元素」，要具体、可据此撰写解说词，不要笼统；\n\
         4) 输出严格 JSON 数组（不要 markdown 代码块、不要额外文字）：\n\
         [{{\"start\":<该段在片段内的起始秒，浮点>,\"end\":<该段结束秒，浮点>,\"desc\":\"<该段画面详细内容，40-60字>\"}}, ...]\n\
         段落按时间顺序排列，覆盖整个片段时长 {dur} 秒；段落数建议 {n_seg} 个左右，按真实画面切换而定。",
        dur = dur_seg as u32, n_seg = n_seg
    );
    match run_llm_vision(client, &base_url, &model, &key, &prompt, &sample, "大模型").await {
        Ok(text) => parse_vision_segments(&text, dur_seg).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// 针对每个已确定的时间窗口取一张代表帧，让多模态模型写出该窗口的「详细内容理解」，
/// 使影片分析对每个窗口都给出具体、可据此撰写解说词的内容（而非笼统概览）。
/// 返回与 windows 等长的描述数组；任一环节失败或长度不匹配则退化为空（调用方回退到粗略描述）。
async fn vision_describe_windows(
    rep_frames: &[std::path::PathBuf],
    windows: &[(f64, f64)],
    pool: &SqlitePool,
    client: &Client,
) -> Vec<String> {
    if rep_frames.is_empty() || windows.is_empty() {
        return Vec::new();
    }
    let sample: Vec<String> = rep_frames
        .iter()
        .filter_map(|f| std::fs::read(f).ok().map(|b| B64.encode(b)))
        .collect();
    if sample.is_empty() {
        return Vec::new();
    }
    let (base_url, model, key) = match llm_provider(pool).await {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let labels: Vec<String> = windows
        .iter()
        .enumerate()
        .map(|(i, (s, e))| format!("第{}张({}-{})", i + 1, va_fmt_ts(*s), va_fmt_ts(*e)))
        .collect();
    let label_line = labels.join("、");
    let prompt = format!(
        "以下是影片片段中若干代表性画面，按时间顺序排列，每张对应一个标注的时间窗口：{labels}。\n\
         请依次为每张图输出「该时间窗口内真实发生的内容」的详细中文描述（40-60字）：\n\
         说明画面里的主体是谁/是什么、在做什么动作、情绪或氛围如何、有哪些关键画面元素；要具体、可据此撰写解说词，不要笼统。\n\
         按图片顺序返回严格 JSON 数组（不要 markdown 代码块、不要额外文字）：[\"<第1张描述>\", \"<第2张描述>\", ...]，数组长度须与图片数一致。",
        labels = label_line
    );
    match run_llm_vision(client, &base_url, &model, &key, &prompt, &sample, "大模型").await {
        Ok(text) => {
            let s = text.trim();
            if let (Some(a), Some(b)) = (s.find('['), s.rfind(']')) {
                if let Ok(v) = serde_json::from_str::<Vec<String>>(&s[a..=b]) {
                    if v.len() == windows.len() {
                        return v.iter().map(|x| x.trim().to_string()).collect();
                    }
                }
            }
            Vec::new()
        }
        Err(_) => Vec::new(),
    }
}

/// 自动对齐兜底：当未提供影片分析报告（scene_nodes 为空）时，基于「真实视频」抽帧并调用多模态大模型，
/// 让其直接给出真实场景切分（时间点对齐）+ 每段画面描述（内容对齐），使解说词在「时间」与「内容」上都
/// 贴合画面，而不是凭标题瞎猜（ffmpeg scene 滤镜在本机对合成/部分素材不触发，故改用视觉分割）。
/// 返回 (scene_nodes 文本, understanding 文本)；任何一步失败都优雅降级为空串（退回原有逻辑）。
async fn auto_ground_from_video(
    pool: &SqlitePool,
    client: &Client,
    data_dir: &Path,
    video_path: &str,
    start: f64,
    end: f64,
) -> (String, String) {
    let dur_seg = (end - start).max(0.5);
    let ff = match FfMpeg::ensure(data_dir).await {
        Ok(f) => f,
        Err(_) => return (String::new(), String::new()),
    };
    // 1) 均匀抽帧
    let frames_dir = data_dir.join("tmp").join("auto_ground");
    let _ = std::fs::create_dir_all(&frames_dir);
    let n_frames = ((dur_seg / 4.0).clamp(6.0, 16.0)) as usize;
    let fps = n_frames as f64 / dur_seg;
    let pattern = frames_dir.join("frame_%03d.jpg");
    let _ = Command::new(&ff.path)
        .args([
            "-ss", &format!("{start:.3}"),
            "-i", video_path,
            "-t", &format!("{dur_seg:.3}"),
            "-vf", &format!("fps={fps:.4}"),
            "-q:v", "2",
            pattern.to_str().unwrap_or("frame_%03d.jpg"),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();
    let frames = list_frames(&frames_dir);
    let segs = vision_segment_frames(&frames, dur_seg, pool, client).await;
    if segs.is_empty() {
        // 视觉分割失败：尝试一次性视觉理解作为内容兜底（时间点交给 LLM 合理切分）
        if let Ok((base_url, model, key)) = llm_provider(pool).await {
            let sample = base64_frames(&frames, 8);
            if !sample.is_empty() {
                let prompt = "你是资深视频分析专家。以下是影片片段按时间顺序的关键帧（已内联为图片）。请综合描述：1) 场景与环境 2) 人物或主体及其动作 3) 关键情绪与张力 4) 整体基调。用简体中文分点回答，不超过 300 字。";
                if let Ok(t) = run_llm_vision(client, &base_url, &model, &key, prompt, &sample, "大模型").await {
                    return (String::new(), t);
                }
            }
        }
        return (String::new(), String::new());
    }
    let scene_nodes = segs
        .iter()
        .map(|(s, e, _d)| format!("{}-{}", fmt_ts(*s), fmt_ts(*e)))
        .collect::<Vec<_>>()
        .join("\n");
    let numbered = segs
        .iter()
        .enumerate()
        .map(|(i, (s, e, d))| format!("{}. {}-{}：{}", i + 1, fmt_ts(*s), fmt_ts(*e), d))
        .collect::<Vec<_>>()
        .join("\n");
    (scene_nodes, format!("（自动基于真实视频帧的视觉理解）\n{numbered}"))
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
    let total_duration = if total_duration > 0.0 { total_duration } else { (n as f64) * 60.0 };

    // ASR 整段无标点（如本地 ASR 仅返回整段）：按字数均分到 n 段，避免后续段为空
    let mut out: Vec<db::TimelineClip> = Vec::new();
    if raw_sentences.len() <= 1 {
        let full = asr_text.trim();
        if !full.is_empty() {
            let chars: Vec<char> = full.chars().collect();
            let per = (chars.len() + n - 1) / n;
            for i in 0..n {
                let s = i * per;
                let e = ((i + 1) * per).min(chars.len());
                let body: String = chars[s..e].iter().collect();
                let start = (i as f64 / n as f64) * total_duration;
                let end = ((i + 1) as f64 / n as f64) * total_duration;
                let sec_label = section_labels.get(i % section_labels.len()).copied().unwrap_or("");
                let text = format!("[{}] {}-{} {}", sec_label, fmt_ts(start), fmt_ts(end), body);
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
        }
        return out;
    }

    let per_seg = (raw_sentences.len() + n - 1) / n;
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
    /** 设置「角色设定」提示词模板内容，作为解说生成的角色身份/口吻基准（前端从 prompts 取 name='角色设定' 传入） */
    pub role_prompt: &'a str,
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

/// M2.6：从影片分析报告中提取「逐窗口内容理解」（相对片段秒数 + 中文内容描述）。
/// 优先解析机器标记 <!--SCENE_SCRIPT:start-end|desc;;...-->；
/// 回退解析「画面时间节点与内容理解」里的 `start - end — desc` 文本。
pub fn extract_scene_script(report: &str) -> Vec<(f64, f64, String)> {
    if let Some(i) = report.find("<!--SCENE_SCRIPT:") {
        let rest = &report[i + "<!--SCENE_SCRIPT:".len()..];
        if let Some(j) = rest.find("-->") {
            let body = &rest[..j];
            let mut out: Vec<(f64, f64, String)> = Vec::new();
            for pair in body.split(";;") {
                let mut it = pair.splitn(2, '|');
                let st = it.next().unwrap_or("").trim();
                let d = it.next().unwrap_or("").to_string();
                let mut t = st.splitn(2, '-');
                let s = t.next().and_then(|x| x.trim().parse::<f64>().ok());
                let e = t.next().and_then(|x| x.trim().parse::<f64>().ok());
                if let (Some(s), Some(e)) = (s, e) {
                    if e > s {
                        out.push((s, e, d));
                    }
                }
            }
            if !out.is_empty() {
                return out;
            }
        }
    }
    // 回退：扫描 `start - end — desc` 行
    let mut out: Vec<(f64, f64, String)> = Vec::new();
    for line in report.lines() {
        if !line.contains(" - ") {
            continue;
        }
        let toks = scan_ts_tokens(line);
        if toks.len() >= 2 && toks[1] > toks[0] {
            let desc = line.splitn(2, '—').nth(1).unwrap_or("").trim().to_string();
            out.push((toks[0], toks[1], desc));
        }
    }
    out
}

/// 解析「自动对齐」产出的带编号窗口理解：`1. 0:00-0:12：内容` → (start, end, desc)。
fn parse_numbered_understanding(text: &str) -> Vec<(f64, f64, String)> {
    let mut out: Vec<(f64, f64, String)> = Vec::new();
    for line in text.lines() {
        let rest = match line.find('.') {
            Some(i) => line[i + 1..].trim(),
            None => continue,
        };
        if let Some(colon) = rest.find('：') {
            let timepart = rest[..colon].trim();
            let desc = rest[colon + 1..].trim().to_string();
            let toks = scan_ts_tokens(timepart);
            if toks.len() >= 2 && toks[1] > toks[0] {
                out.push((toks[0], toks[1], desc));
            }
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
    // 去 AI 味 + 通俗化：强制口语化、禁用套话，让字幕/配音像真人在聊天
    let deai_block = r#"【语言风格（最高优先级，必须严格执行）】：解说词要彻底去 AI 味、通俗易懂，就像你边看画面边跟朋友随口聊。
- 严禁 AI 套话与书面八股：不得出现「总而言之 / 值得注意的是 / 在这个快节奏的时代 / 令人惊叹的是 / 不难看出 / 首先…其次…最后 / 不仅如此 / 值得一提的是 / 顾名思义 / 由此可见」等模板句；
- 禁用排比三连、对仗过渡句、空洞总结；少用「之 / 其 / 然 / 故而 / 诸如」等书面字；
- 多用短句和口语词（咱们、其实、说白了、你品、讲真的、说穿了），适当用生活化比喻；
- 一句大白话顶十句漂亮话，老百姓一听就懂，不绕弯子、不端着；
- 每条字幕就是一句人话，节奏自然、有停顿感，像真人在说话，而不是机器朗读。"#;
    let analysis_block = if !cfg.analysis.trim().is_empty() {
        format!(
            "影片逐窗口内容理解（来自多模态大模型，是每条字幕内容的事实依据，请据此撰写贴合画面、不脱离实际内容的解说）：\n{}\n",
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
    // 设置「角色设定」：作为解说生成的角色身份/口吻基准，全程遵循
    let role_block = if !cfg.role_prompt.trim().is_empty() {
        format!(
            "角色设定（你在此视频解说中的身份、口吻与表达基准，须全程遵循，不得偏离）：\n{}\n",
            cfg.role_prompt.trim()
        )
    } else {
        String::new()
    };
    // M2.6：真实画面时间节点（来自场景检测/语义块）——字幕据此与画面对齐
    let has_nodes = cfg.scene_nodes.trim().len() > 0 && cfg.scene_nodes.lines().count() >= 2;
    let nodes_block = if has_nodes {
        format!(
            "画面时间节点（来自真实场景检测/语义块划分，是「字幕与画面对齐」的硬性依据，请逐段对齐；其中每条窗口对应的「该窗口真实画面内容」见下方「影片逐窗口内容理解」）：\n{}\n",
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
{role_block}
视频标题：{title}
写作风格：{style_hint}
{lang_hint}
{extra_hints}
{deai_block}
视频时长：{d} 分钟（≈ {target_chars} 字）
{hint_block}

{analysis_block}
{nodes_block}
{asr_block}

要求：
1. 每条字幕用【真实原创】的解说词，体现上述风格与视角，不要照抄原片语音
2. 若提供了「影片逐窗口内容理解」，须以其为每条字幕内容的事实基础，确保第 N 条字幕与第 N 个窗口内容一致，不得脱离实际内容
3. {timeline_req}
4. 整体遵循「开端→铺垫→冲突→高潮→反转→结局」的叙事弧，但字幕切分以画面时间节点为准（可将多条相邻字幕归入同一叙事阶段）
5. 内容硬约束（最高优先级）：第 N 条字幕必须只描述「影片逐窗口内容理解」中第 N 个窗口标明的真实内容——即该时间区间内画面里真实发生的事。紧扣对应区间画面，严禁串到其它窗口、严禁提前剧透后续、严禁编造画面里没有的东西；看得到才说，看不到的不说。若某窗口内容理解为空，也只可基于该窗口时间区间合理推断，不得套用别的窗口内容。
6. 返回严格 JSON 数组（不要任何额外文字、不要 markdown 代码块）：
[{{"start": "0:00", "end": "0:12", "dialogue": "解说词...", "section": "开端"}}, ...]
其中 section 可选，若能判断请标注：开端 / 铺垫 / 冲突 / 高潮 / 反转 / 结局
7. 字数 ≈ {target_chars} 字，口语化、避免空话，每条字幕精炼贴合对应画面
{simp_req}
"#,
        d = cfg.duration_min,
        title = cfg.title,
        style_hint = style_hint,
        lang_hint = lang_hint,
        extra_hints = extra_hints,
        deai_block = deai_block,
        target_chars = target_chars,
        hint_block = hint_block,
        analysis_block = analysis_block,
        nodes_block = nodes_block,
        asr_block = asr_block,
        role_block = role_block,
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
        // 兼容 dialogue / text / content 三种字段名（部分小模型不返回 dialogue）
        let dialogue = item
            .get("dialogue")
            .or_else(|| item.get("text"))
            .or_else(|| item.get("content"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
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

/// 六段式段落标签
const NARRATION_SECTIONS: &[&str] = &["开端", "铺垫", "冲突", "高潮", "反转", "结局"];

/// 解析单行字幕：`[段落] start-end 文案` 或 `start-end 文案`
/// 返回 (section, start_ts, end_ts, dialogue)；无法识别返回 None
fn parse_one_clip_line(line: &str) -> Option<(String, String, String, String)> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let mut section = String::new();
    let mut rest = line;
    if let Some(stripped) = line.strip_prefix('[') {
        if let Some(close) = stripped.find(']') {
            section = stripped[..close].trim().to_string();
            rest = stripped[close + 1..].trim();
        }
    }
    let toks: Vec<&str> = rest.split_whitespace().collect();
    for (i, t) in toks.iter().enumerate() {
        if *t == "-" && i >= 1 && i + 1 < toks.len() {
            let start = toks[i - 1];
            let end = toks[i + 1];
            if start.contains(':') && end.contains(':') {
                let dialogue = toks[i + 2..].join(" ");
                return Some((section, start.to_string(), end.to_string(), dialogue));
            }
        }
    }
    None
}

/// 把纯文本切成多个语义块（用于 LLM 返回非 JSON 的兜底）：
/// 优先按空行，其次按换行，最后按句号每 2 句一组
fn split_into_blocks(text: &str) -> Vec<String> {
    let by_blank: Vec<String> = text
        .split("\n\n")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if by_blank.len() >= 2 {
        return by_blank;
    }
    let by_line: Vec<String> = text
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if by_line.len() >= 2 {
        return by_line;
    }
    let sentences: Vec<&str> = text
        .split(|c: char| "。！？!?.".contains(c))
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if sentences.len() >= 2 {
        let per = 2usize;
        let mut blocks = Vec::new();
        for chunk in sentences.chunks(per) {
            blocks.push(chunk.join("。"));
        }
        return blocks;
    }
    Vec::new()
}

/// M2.5：当 LLM 未返回规范 JSON 时，从其原文恢复多段解说结构。
/// ① 提取带 `[段落] start-end 文案` / `start-end 文案` 的行；
/// ② 否则把纯分段文本（空行/换行/句子）按总时长均分时间。
/// 返回空表示完全无法恢复（上层再用 ASR 切分兜底）。
pub fn recover_narration_segments(llm_text: &str, total_duration: f64) -> Vec<db::TimelineClip> {
    let dur = if total_duration > 0.0 { total_duration } else { 180.0 };

    // 1) 提取带时间标签的行
    let mut tagged: Vec<(String, String, String, String)> = Vec::new();
    for line in llm_text.lines() {
        if let Some(c) = parse_one_clip_line(line) {
            tagged.push(c);
        }
    }
    if !tagged.is_empty() {
        return tagged
            .into_iter()
            .enumerate()
            .map(|(i, (section, start, end, dialogue))| {
                let start_sec = parse_timestamp(&start);
                let end_sec = parse_timestamp(&end);
                let label = if section.is_empty() {
                    NARRATION_SECTIONS[i % NARRATION_SECTIONS.len()].to_string()
                } else {
                    section.clone()
                };
                let text = format!("[{}] {}-{} {}", label, start, end, dialogue.trim());
                db::TimelineClip {
                    id: format!("s{i}"),
                    source: "narration".into(),
                    timeline_start: start_sec,
                    timeline_end: end_sec,
                    src_start: start_sec,
                    src_end: end_sec,
                    label,
                    text,
                    flower: String::new(),
                    transition: "none".into(),
                }
            })
            .collect();
    }

    // 2) 纯分段文本：按块均分时间
    let blocks = split_into_blocks(llm_text);
    if blocks.len() >= 2 {
        let n = blocks.len();
        return blocks
            .into_iter()
            .enumerate()
            .map(|(i, body)| {
                let start = (i as f64 / n as f64) * dur;
                let end = ((i + 1) as f64 / n as f64) * dur;
                let label = NARRATION_SECTIONS[i % NARRATION_SECTIONS.len()].to_string();
                let text = format!("[{}] {}-{} {}", label, fmt_ts(start), fmt_ts(end), body.trim());
                db::TimelineClip {
                    id: format!("s{i}"),
                    source: "narration".into(),
                    timeline_start: start,
                    timeline_end: end,
                    src_start: start,
                    src_end: end,
                    label,
                    text,
                    flower: String::new(),
                    transition: "none".into(),
                }
            })
            .collect();
    }

    Vec::new()
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
    let role_prompt = job.payload.get("role_prompt").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let range_start = job.payload.get("rangeStart").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let range_end_raw = job.payload.get("rangeEnd").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let range_end = if range_end_raw > range_start { range_end_raw } else { duration as f64 };
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
            // 提供足够的上下文让 LLM 能生成 6 段式解说，避免返回空 content
            let mut note = format!(
                "视频标题：{}。分析片段时长：约 {} 秒。请按所选风格「{}」生成一段完整的六段式解说文案（开端/铺垫/冲突/高潮/反转/结局），每段包含起止时间与解说词正文。",
                title, duration, if style_name.is_empty() { "默认" } else { &style_name }
            );
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
                // M2.6：从影片分析报告提取「逐窗口内容理解」（真实画面时间 + 中文内容）。
                // 若分析里携带逐窗口内容，则用它驱动解说，确保每条字幕对应一个窗口的真实画面内容；
                // 若未提供分析或分析过浅（仅一个空窗口），则自动基于真实视频做场景检测 + 多模态视觉理解兜底。
                let script3 = extract_scene_script(&analysis);
                let has_real_script = !script3.is_empty()
                    && !(script3.len() == 1 && script3[0].2.trim().is_empty());
                let (scene_nodes, analysis_for_llm) = if has_real_script {
                    // 有逐窗口内容理解：scene_nodes 仅列时间窗口（表头），analysis 承载带内容的理解
                    let sn = format_scene_nodes(
                        &script3.iter().map(|(s, e, _)| (*s, *e)).collect::<Vec<_>>(),
                    );
                    let al = script3
                        .iter()
                        .enumerate()
                        .map(|(i, (s, e, d))| format!("{}. {}-{}：【{}】", i + 1, fmt_ts(*s), fmt_ts(*e), d))
                        .collect::<Vec<_>>()
                        .join("\n");
                    (sn, al)
                } else {
                    let nodes = extract_scene_nodes(&analysis);
                    let mut sn = format_scene_nodes(&nodes);
                    let mut al = analysis.clone();
                    if nodes.is_empty() && !video_path.is_empty() {
                        emit(ProgressMsg {
                            task_id: job.id.clone(),
                            progress: 45.0,
                            status: "running".into(),
                            message: Some("未检测到画面分析，自动基于真实视频对齐场景".into()),
                            payload: None,
                        });
                        let (auto_nodes, auto_understanding) = auto_ground_from_video(pool, client, data_dir, &video_path, range_start, range_end).await;
                        if !auto_understanding.is_empty() {
                            // 自动对齐产出带编号的逐窗口理解（形如「1. 0:00-0:12：内容」），
                            // 解析为「时间窗口 + 内容」后作为内容依据，scene_nodes 也承载内容以便提示词双重强调
                            let auto_script = parse_numbered_understanding(&auto_understanding);
                            if !auto_script.is_empty() {
                                sn = format_scene_nodes(
                                    &auto_script.iter().map(|(s, e, _)| (*s, *e)).collect::<Vec<_>>(),
                                );
                                al = auto_script
                                    .iter()
                                    .enumerate()
                                    .map(|(i, (s, e, d))| format!("{}. {}-{}：【{}】", i + 1, fmt_ts(*s), fmt_ts(*e), d))
                                    .collect::<Vec<_>>()
                                    .join("\n");
                            } else {
                                if !auto_nodes.is_empty() { sn = auto_nodes; }
                                if !auto_understanding.is_empty() { al = auto_understanding; }
                            }
                        }
                    }
                    (sn, al)
                };
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
                    analysis: &analysis_for_llm,
                    role_prompt: &role_prompt,
                    scene_nodes: &scene_nodes,
                };
                let prompt = build_narration_prompt(&cfg);
                match run_llm_text(client, &base_url, &llm_model, &key, &prompt).await {
                    Ok(text) => {
                        eprintln!("[film-script-gen] LLM ok text_len={}", text.len());
                        match parse_narration_response(&text, duration as f64, &style) {
                            Ok(shots) => {
                                eprintln!("[film-script-gen] parse OK shots={}", shots.len());
                                // 保留 LLM 生成的原创解说（含时间轴），不再用原始 ASR 覆盖
                                let s: String = shots.iter().map(|x| x.text.clone()).collect::<Vec<_>>().join("\n");
                                s
                            }
                            Err(_e) => {
                                // 降级 2a：优先从 LLM 原文恢复多段结构（本地小模型常返回带标签文本而非 JSON）
                                let recovered = recover_narration_segments(&text, duration as f64);
                                eprintln!("[film-script-gen] parse ERR recovered={} pre_sections={}", recovered.len(), pre_sections.len());
                                if !recovered.is_empty() {
                                    recovered.iter().map(|x| x.text.clone()).collect::<Vec<_>>().join("\n")
                                } else {
                                    // 降级 2b：用 ASR 切分兜底（注意：asr_failed 时 pre_sections 是空数组，会得到空文案）
                                    pre_sections.iter().map(|x| x.text.clone()).collect::<Vec<_>>().join("\n")
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[film-script-gen] LLM ERR {e}, fallback to pre_sections len={}", pre_sections.len());
                        // 降级 2：LLM 失败，用 ASR 切分
                        pre_sections.iter().map(|x| x.text.clone()).collect::<Vec<_>>().join("\n")
                    }
                }
            }
            Err(e) => {
                eprintln!("[film-script-gen] llm_provider ERR {e}, fallback to pre_sections len={}", pre_sections.len());
                // 降级 1：缺 Key
                pre_sections.iter().map(|x| x.text.clone()).collect::<Vec<_>>().join("\n")
            }
        }
    };
    eprintln!("[film-script-gen] raw_script_len={} asr_failed={asr_failed} asr_text_len={}", raw_script.len(), asr_text.len());

    // 最终兜底：即便 LLM 全部降级链路都失败（返回空 content 等），也用 title/style/duration 生成 6 段模板文案
    // 避免 UI 出现「script 为空 → 无结果区」的情况
    let raw_script = if raw_script.trim().is_empty() {
        eprintln!("[film-script-gen] raw_script empty, using final fallback template");
        let secs = ["开端", "铺垫", "冲突", "高潮", "反转", "结局"];
        let seg = (duration as f64) / secs.len() as f64;
        secs.iter().enumerate().map(|(i, s)| {
            let start = va_fmt_ts(seg * i as f64);
            let end = va_fmt_ts(seg * (i + 1) as f64);
            format!(
                "[{}] {}-{} 【{}】这是《{}》的第 {} 段「{}」占位文案（因大模型返回空 content 自动生成）。完整分析：风格 {}；时长约 {} 秒；请在解说工作台基于实际理解精修。",
                s, start, end, style_name, title, i + 1, s,
                if style.is_empty() { "默认".to_string() } else { style.to_string() },
                duration
            )
        }).collect::<Vec<_>>().join("\n")
    } else {
        raw_script
    };

    // 繁体 -> 简体兜底（即使 LLM 偶发繁体或 ASR 上下文带入，也统一为简体）
    let script = to_simplified(&raw_script);
    // 落库失败（如 SQLite 锁/磁盘）不阻断整个文案生成
    db::film_project_set_script(pool, &project_id, &script).await.ok();
    let _ = language; // 抑制 unused 警告
    Ok((script, asr_failed, asr_reason))
}

// ===========================================================================
// 批量配音（每段一段 XiaomiMimo TTS）
// ===========================================================================

#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct DubItemIn {
    index: usize,
    text: String,
    #[serde(default)]
    voice: Option<String>,
}

async fn run_batch_dub(
    pool: &SqlitePool,
    client: &Client,
    data_dir: &Path,
    job: &TaskJob,
    emit: Arc<dyn Fn(ProgressMsg) + Send + Sync>,
) -> Result<(), String> {
    let project_id = job.project_id.clone().unwrap_or_else(|| "local".into());
    let items_raw = job.payload.get("segments").and_then(|v| v.as_str()).unwrap_or("[]");
    let items: Vec<DubItemIn> = serde_json::from_str(items_raw).map_err(|e| format!("segments 解析失败: {e}"))?;
    if items.is_empty() { return Err("segments 为空".into()); }
    // 输出目录：data_dir/dub/<project>/
    let base_dir = data_dir.join("dub").join(sanitize_filename(&project_id));
    std::fs::create_dir_all(&base_dir).ok();

    let total = items.len();
    let mut ok_count = 0usize;
    for (i, item) in items.iter().enumerate() {
        let prog = ((i as f64) / (total as f64) * 90.0).clamp(0.0, 90.0);
        emit(ProgressMsg {
            task_id: job.id.clone(),
            progress: prog,
            status: "running".into(),
            message: Some(format!("配音 {}/{} ({}…)", i + 1, total, &item.text.chars().take(12).collect::<String>())),
            payload: Some(serde_json::json!({ "index": i, "status": "dubbing" })),
        });
        let voice = item.voice.clone().unwrap_or_else(|| "default".into());
        let wav_path = base_dir.join(format!("seg_{:03}.wav", i));
        match tts_one_segment(pool, client, &item.text, &voice, &wav_path).await {
            Ok(url) => {
                ok_count += 1;
                emit(ProgressMsg {
                    task_id: job.id.clone(),
                    progress: prog + (90.0 / total as f64),
                    status: "running".into(),
                    message: Some(format!("{}/{} 已合成", i + 1, total)),
                    payload: Some(serde_json::json!({ "index": i, "status": "ok", "url": url })),
                });
            }
            Err(e) => {
                emit(ProgressMsg {
                    task_id: job.id.clone(),
                    progress: prog + (90.0 / total as f64),
                    status: "running".into(),
                    message: Some(format!("{}/{} 失败：{}", i + 1, total, e)),
                    payload: Some(serde_json::json!({ "index": i, "status": "failed", "reason": e })),
                });
            }
        }
    }
    emit(ProgressMsg {
        task_id: job.id.clone(),
        progress: 95.0,
        status: "running".into(),
        message: Some(format!("完成 {}/{} 段配音 → {}", ok_count, total, base_dir.display())),
        payload: Some(serde_json::json!({ "dir": base_dir.display().to_string() })),
    });
    Ok(())
}

/// 单段 TTS：直接复刻 synthesize_tts 的实现（避免依赖旧签名），失败回退到 fileserver 直连。
async fn tts_one_segment(
    pool: &SqlitePool,
    client: &Client,
    text: &str,
    voice: &str,
    out_path: &std::path::Path,
) -> Result<String, String> {
    if text.trim().is_empty() { return Err("空文本".into()); }
    let row = db::get_by_kind(pool, "tts").await.map_err(|e| format!("无 TTS 配置: {e}"))?;
    let key = match cred::get_key(pool, "tts").await {
        Ok(Some(k)) if !k.is_empty() => k,
        _ => return Err("未配置 TTS Key（设置 → 接口 → 语音合成）".into()),
    };
    let base_url = row.base_url.trim_end_matches('/');
    // XiaomiMiMo TTS 走 Chat Completions 协议（非 OpenAI 的 /audio/speech，后者返回 404）：
    // 文本放进 messages 里 role=assistant 的 content，audio 对象声明格式/音色，
    // 响应在 choices[0].message.audio.data（base64 音频）。
    let v = if voice.is_empty() || voice.eq_ignore_ascii_case("default") { "mimo_default" } else { voice };
    let body = serde_json::json!({
        "model": row.model,
        "messages": [ { "role": "assistant", "content": text } ],
        "audio": { "format": "wav", "voice": v },
        "stream": false,
    });
    let resp = client
        .post(format!("{base_url}/chat/completions"))
        .header("api-key", &key)
        .bearer_auth(&key)
        .json(&body)
        .timeout(Duration::from_secs(90))
        .send()
        .await
        .map_err(|e| format!("TTS 请求失败: {e}"))?;
    if !resp.status().is_success() {
        let st = resp.status();
        let txt = resp.text().await.unwrap_or_default();
        return Err(format!("TTS {}: {}", st, txt.chars().take(200).collect::<String>()));
    }
    let j: serde_json::Value = resp.json().await.map_err(|e| format!("TTS 响应解析失败: {e}"))?;
    let b64 = j
        .get("choices").and_then(|c| c.get(0))
        .and_then(|m| m.get("message"))
        .and_then(|m| m.get("audio"))
        .and_then(|a| a.get("data"))
        .and_then(|d| d.as_str())
        .ok_or("TTS 响应缺少 audio.data（未返回音频）")?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).map_err(|e| format!("TTS 音频解码失败: {e}"))?;
    if let Some(parent) = out_path.parent() { std::fs::create_dir_all(parent).ok(); }
    std::fs::write(out_path, &bytes).map_err(|e| format!("写 wav 失败: {e}"))?;
    Ok(out_path.to_string_lossy().to_string())
}

// ===========================================================================
// 剪映草稿导出（JianyingPro/Drafts/<project>_<ts>/）
// ===========================================================================

async fn run_film_jianying_draft(
    _pool: &SqlitePool,
    _client: &Client,
    data_dir: &Path,
    job: &TaskJob,
    emit: Arc<dyn Fn(ProgressMsg) + Send + Sync>,
) -> Result<String, String> {
    let project_id = job.project_id.clone().ok_or("缺少 projectId")?;
    let script = job.payload.get("script").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let video_path = job.payload.get("videoPath").and_then(|v| v.as_str()).unwrap_or("").to_string();

    emit(ProgressMsg { task_id: job.id.clone(), progress: 5.0, status: "running".into(),
        message: Some("解析分镜".into()), payload: None });

    // 1) 解析分镜（沿用 Step6 同款正则）
    let segs = parse_script_to_draft_segments(&script);
    if segs.is_empty() { return Err("无可导出的分镜".into()); }

    // 2) 准备输出根目录：若指定 outDir 则写入该文件夹下的 <project>_<ts>/ 子目录，否则回退 data_dir/jianying_drafts/
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let safe = sanitize_filename(&project_id);
    let base = match job.payload.get("outDir").and_then(|v| v.as_str()) {
        Some(d) if !d.trim().is_empty() => PathBuf::from(d.trim()),
        _ => { let mut b = data_dir.to_path_buf(); b.push("jianying_drafts"); b }
    };
    let draft_root = base.join(format!("{safe}_{ts}"));
    std::fs::create_dir_all(&draft_root).map_err(|e| format!("建目录失败: {e}"))?;
    std::fs::create_dir_all(draft_root.join("video")).ok();
    std::fs::create_dir_all(draft_root.join("audio")).ok();
    std::fs::create_dir_all(draft_root.join("subtitle")).ok();

    // 3) 真实素材：逐段从源片切出 video/seg_<i>.mp4，并复制分段配音 audio/seg_<i>.wav（若已批量配音）
    let ff = FfMpeg::ensure(data_dir).await?;
    let dub_dir = data_dir.join("dub").join(&safe);
    let has_audio = cut_real_materials(&ff, &video_path, &segs, &dub_dir, &draft_root, emit.clone(), &job.id, 10.0, 70.0).await;

    // 4) 写出 draft_content.json（真实素材引用：视频轨 + 字幕轨 + 配音轨）
    let content = build_jianying_content_real(&segs, &has_audio);
    std::fs::write(draft_root.join("draft_content.json"),
        serde_json::to_string_pretty(&content).map_err(|e| format!("序列化失败: {e}"))?)
        .map_err(|e| format!("写 draft_content.json 失败: {e}"))?;

    // 5) 写两个必备元信息文件
    let ts_ms = ts * 1000;
    let meta = serde_json::json!({
        "tm_draft_create": ts_ms, "tm_draft_modified": ts_ms,
        "draft_id": project_id, "draft_name": project_id,
        "draft_fold_path": draft_root.file_name().unwrap().to_string_lossy().to_string(),
        "tm_duration": (segs.last().map(|s| s.end).unwrap_or(0.0) * 1_000_000.0) as i64,
        "cover": "", "fps": 30.0, "platform": {"app_id": 3704, "app_source": "lv", "app_version": "9.0.0"},
    });
    std::fs::write(draft_root.join("draft_meta_info.json"), serde_json::to_string_pretty(&meta).unwrap_or_default()).ok();
    std::fs::write(draft_root.join("draft_extra_info.json"), serde_json::to_string_pretty(&serde_json::json!({"extra_info": null})).unwrap_or_default()).ok();

    let audio_cnt = has_audio.iter().filter(|x| **x).count();
    emit(ProgressMsg { task_id: job.id.clone(), progress: 95.0, status: "running".into(),
        message: Some(format!("草稿生成：{} 段视频 / {} 段配音", segs.len(), audio_cnt)), payload: None });

    Ok(draft_root.display().to_string())
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
struct DraftSegment {
    section: String,
    start: f64,
    end: f64,
    text: String,
    voice: Option<String>,
}

fn parse_script_to_draft_segments(script: &str) -> Vec<DraftSegment> {
    let mut out: Vec<DraftSegment> = Vec::new();
    for line in script.lines().map(|l| l.trim()).filter(|l| !l.is_empty()) {
        if let Some(caps) = regex_lite(line) {
            out.push(caps);
        }
    }
    out
}

fn regex_lite(line: &str) -> Option<DraftSegment> {
    // 格式：[section] m:ss-m:ss 文案  （分钟 1~2 位，无前导零均可）
    let parts: Vec<&str> = line.splitn(2, ']').collect();
    if parts.len() != 2 { return None; }
    let section = parts[0].trim_start_matches('[').trim().to_string();
    let rest = parts[1].trim();
    // 时间戳在前，其后空白分隔文案；支持 1 位或 2 位分钟
    let sp: Vec<&str> = rest.splitn(2, char::is_whitespace).collect();
    if sp.len() != 2 { return None; }
    let ts_part = sp[0].trim();
    let text = sp[1].trim().to_string();
    let ts: Vec<&str> = ts_part.splitn(2, '-').collect();
    if ts.len() != 2 { return None; }
    let parse_ts = |s: &str| -> Option<f64> {
        let v: Vec<&str> = s.split(':').collect();
        if v.len() == 2 {
            let m: f64 = v[0].parse().ok()?;
            let ss: f64 = v[1].parse().ok()?;
            Some(m * 60.0 + ss)
        } else if v.len() == 3 {
            let h: f64 = v[0].parse().ok()?;
            let m: f64 = v[1].parse().ok()?;
            let ss: f64 = v[2].parse().ok()?;
            Some(h * 3600.0 + m * 60.0 + ss)
        } else { None }
    };
    let start = parse_ts(ts[0].trim())?;
    let end = parse_ts(ts[1].trim())?;
    if end <= start { return None; }
    Some(DraftSegment { section, start, end, text, voice: None })
}

fn sanitize_filename(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect::<String>().chars().take(48).collect()
}

/// 生成真实可用的剪映 draft_content.json：视频轨（每段从源片真实切出的 video/seg_<i>.mp4）
/// + 字幕轨（每段解说词文本）+ 配音轨（若批量配音已生成 seg_<i>.wav 则引用 audio/seg_<i>.wav）。
fn build_jianying_content_real(segs: &[DraftSegment], has_audio: &[bool]) -> serde_json::Value {
    let mut videos: Vec<serde_json::Value> = Vec::new();
    for (i, s) in segs.iter().enumerate() {
        let dur_us = ((s.end - s.start) * 1_000_000.0).max(0.0) as i64;
        let start_us = (s.start * 1_000_000.0) as i64;
        videos.push(serde_json::json!({
            "id": format!("seg_v_{i}"), "type": "video", "material_id": format!("mat_v_{i}"),
            "source_timerange": { "start": 0, "duration": dur_us },
            "target_timerange": { "start": start_us, "duration": dur_us },
            "speed": 1.0, "volume": 1.0, "visible": true, "extra_material_refs": []
        }));
    }
    let mut subs: Vec<serde_json::Value> = Vec::new();
    for (i, s) in segs.iter().enumerate() {
        let dur_us = ((s.end - s.start) * 1_000_000.0).max(0.0) as i64;
        let start_us = (s.start * 1_000_000.0) as i64;
        subs.push(serde_json::json!({
            "id": format!("seg_s_{i}"), "type": "text",
            "target_timerange": { "start": start_us, "duration": dur_us },
            "speed": 1.0, "visible": true,
            "material_id": format!("mat_s_{i}"),
            "extra_material_refs": []
        }));
    }
    let materials_video: Vec<serde_json::Value> = segs.iter().enumerate().map(|(i, s)| {
        serde_json::json!({
            "id": format!("mat_v_{i}"),
            "type": "video",
            "path": format!("video/seg_{:03}.mp4", i),
            "duration": ((s.end - s.start) * 1_000_000.0) as i64,
            "width": 1280, "height": 720,
            "has_audio": false,
            "has_sound_separated": false
        })
    }).collect();
    let materials_text: Vec<serde_json::Value> = segs.iter().enumerate().map(|(i, s)| {
        serde_json::json!({
            "id": format!("mat_s_{i}"),
            "type": "text",
            "content": { "text": s.text, "font": { "id": "default" } }
        })
    }).collect();

    let mut tracks: Vec<serde_json::Value> = vec![
        serde_json::json!({
            "id": "main_video", "type": "video", "attribute": 0, "flag": 0, "segments": videos
        }),
        serde_json::json!({
            "id": "main_subtitle", "type": "text", "attribute": 0, "flag": 0, "segments": subs
        }),
    ];

    let mut materials_audio: Vec<serde_json::Value> = Vec::new();
    if has_audio.iter().any(|x| *x) {
        let mut audios: Vec<serde_json::Value> = Vec::new();
        for (i, s) in segs.iter().enumerate() {
            if !has_audio.get(i).copied().unwrap_or(false) { continue; }
            let dur_us = ((s.end - s.start) * 1_000_000.0).max(0.0) as i64;
            let start_us = (s.start * 1_000_000.0) as i64;
            audios.push(serde_json::json!({
                "id": format!("seg_a_{i}"), "type": "audio", "material_id": format!("mat_a_{i}"),
                "source_timerange": { "start": 0, "duration": dur_us },
                "target_timerange": { "start": start_us, "duration": dur_us },
                "speed": 1.0, "volume": 1.0, "visible": true, "extra_material_refs": []
            }));
            materials_audio.push(serde_json::json!({
                "id": format!("mat_a_{i}"), "type": "audio",
                "path": format!("audio/seg_{:03}.wav", i),
                "duration": dur_us
            }));
        }
        tracks.push(serde_json::json!({
            "id": "main_audio", "type": "audio", "attribute": 0, "flag": 0, "segments": audios
        }));
    }

    serde_json::json!({
        "id": format!("VF_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)),
        "canvas_config": { "width": 1280, "height": 720, "ratio": "16:9" },
        "duration": (segs.last().map(|s| s.end).unwrap_or(0.0) * 1_000_000.0) as i64,
        "fps": { "denominator": 1, "numerator": 30 },
        "tracks": tracks,
        "materials": {
            "videos": materials_video,
            "texts": materials_text,
            "audios": materials_audio,
            "audio_effects": [],
            "audio_fades": [],
            "audio_indexes": [],
            "video_effects": [],
            "transitions": [],
            "effects": [],
            "filters": [],
            "animations": [],
            "stickers": [],
            "speech_to_songs": [],
            "placeholders": []
        },
        "relations": [],
        "version": 3
    })
}

// ===========================================================================
// 导出 Premiere（CMX3600 EDL + 时间线 JSON + 字幕 SRT 三件套）
// ===========================================================================

async fn run_film_premiere_export(
    _pool: &SqlitePool,
    data_dir: &Path,
    job: &TaskJob,
    emit: Arc<dyn Fn(ProgressMsg) + Send + Sync>,
) -> Result<String, String> {
    let project_id = job.project_id.clone().ok_or("缺少 projectId")?;
    let script = job.payload.get("script").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let _ = job.payload.get("followOriginal").and_then(|v| v.as_bool()).unwrap_or(false);

    emit(ProgressMsg { task_id: job.id.clone(), progress: 10.0, status: "running".into(),
        message: Some("解析分镜".into()), payload: None });
    let segs = parse_script_to_draft_segments(&script);
    if segs.is_empty() { return Err("无可导出的分镜".into()); }

    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let safe = sanitize_filename(&project_id);
    let out_dir_base = match job.payload.get("outDir").and_then(|v| v.as_str()) {
        Some(d) if !d.trim().is_empty() => PathBuf::from(d.trim()),
        _ => { let mut b = data_dir.to_path_buf(); b.push("premier_drafts"); b }
    };
    let draft_root = out_dir_base.join(format!("{safe}_{ts}"));
    std::fs::create_dir_all(&draft_root).map_err(|e| format!("建目录失败: {e}"))?;

    // (1) .edl（CMX3600，仅视频轨；时间线从零开始）
    let mut edl = String::new();
    edl.push_str("TITLE: VIDEOSFLOW_PREMIERE_TIMELINE\n");
    edl.push_str("FCM: NON-DROP FRAME\n\n");
    for (i, s) in segs.iter().enumerate() {
        edl.push_str(&format!("{:03} V C {} {} {} {}-{}\n", i + 1, fmt_edl(s.start), fmt_edl(s.end), fmt_edl(s.end - s.start), "*", "FROM CLIPS"));
    }
    std::fs::write(draft_root.join("timeline.edl"), edl).map_err(|e| format!("写 edl 失败: {e}"))?;

    emit(ProgressMsg { task_id: job.id.clone(), progress: 40.0, status: "running".into(),
        message: Some("写时间线 JSON".into()), payload: None });

    // (2) timeline.json（适用于任意 NLE 的通用时间线描述）
    let clips: Vec<serde_json::Value> = segs.iter().enumerate().map(|(i, s)| serde_json::json!({
        "id": format!("clip_{i}"),
        "mediaType": "video",
        "srcStart": s.start,
        "srcEnd": s.end,
        "timelineStart": s.start,
        "timelineEnd": s.end,
        "label": s.section,
        "text": s.text,
    })).collect();
    let timeline = serde_json::json!({
        "version": 1,
        "canvasConfig": { "width": 1280, "height": 720, "ratio": "16:9" },
        "fps": 30.0,
        "duration": segs.last().map(|s| s.end).unwrap_or(0.0),
        "clips": clips,
        "subtitles": segs.iter().enumerate().map(|(i, s)| serde_json::json!({
            "id": format!("sub_{i}"), "start": s.start, "end": s.end, "text": s.text,
        })).collect::<Vec<_>>(),
    });
    std::fs::write(draft_root.join("timeline.json"), serde_json::to_string_pretty(&timeline).unwrap_or_default())
        .map_err(|e| format!("写 timeline.json 失败: {e}"))?;

    emit(ProgressMsg { task_id: job.id.clone(), progress: 70.0, status: "running".into(),
        message: Some("生成 SRT 字幕".into()), payload: None });

    // (3) subtitles.srt
    let mut srt = String::new();
    for (i, s) in segs.iter().enumerate() {
        srt.push_str(&format!("{}\n{} --> {}\n{}\n\n", i + 1, fmt_srt(s.start), fmt_srt(s.end), s.text));
    }
    std::fs::write(draft_root.join("subtitles.srt"), srt).map_err(|e| format!("写 srt 失败: {e}"))?;

    emit(ProgressMsg { task_id: job.id.clone(), progress: 90.0, status: "running".into(),
        message: Some("写 README".into()), payload: None });

    // (4) README：给 PR / FCPX / VEGAS 用户提示如何导入
    let readme = format!(
        "# Premiere / NLE 三件套\n\n导出目录：{}\n\n文件：\n- `timeline.edl` — CMX3600 EDL（视频轨）。Premiere 选择 File > Import > 导入到序列。\n- `timeline.json` — 通用时间线 JSON（自描述）。\n- `subtitles.srt` — 标准 SRT 字幕。\n\n字段说明：\n- `clip_<i>.srcStart/End` 与 `timelineStart/End` 同步（不做重映射）。\n- 如需重映射到序列起点 0，可在 Premiere 内全选片段 → Nest Sequence → Trim 入口/出口。\n",
        draft_root.display()
    );
    std::fs::write(draft_root.join("README.md"), readme).ok();

    Ok(draft_root.display().to_string())
}

fn fmt_edl(sec: f64) -> String {
    // CMX3600: HH:MM:SS:FF (假设 30fps → 30 frames/sec)
    let s = sec.max(0.0);
    let total = (s * 30.0).round() as i64;
    let h = total / (3600 * 30);
    let rem = total % (3600 * 30);
    let m = rem / (60 * 30);
    let rem = rem % (60 * 30);
    let ss = rem / 30;
    let ff = rem % 30;
    format!("{:02}:{:02}:{:02}:{:02}", h, m, ss, ff)
}

fn fmt_srt(sec: f64) -> String {
    let s = sec.max(0.0);
    let h = (s / 3600.0).floor() as i64;
    let m = ((s % 3600.0) / 60.0).floor() as i64;
    let ss = (s % 60.0).floor() as i64;
    let ms = ((s - s.floor()) * 1000.0).round() as i64;
    if h > 0 { format!("{:02}:{:02}:{:02},{:03}", h, m, ss, ms) }
    else { format!("{:02}:{:02},{:03}", m, ss, ms) }
}

// ===========================================================================
// 导出国际剪映（CapCut / Jianying International）：与国内剪映结构等价，
// 但 media path 用绝对路径 + 部分元数据走英文。
// ===========================================================================

async fn run_film_jianying_draft_intl(
    _pool: &SqlitePool,
    _client: &Client,
    data_dir: &Path,
    job: &TaskJob,
    emit: Arc<dyn Fn(ProgressMsg) + Send + Sync>,
) -> Result<String, String> {
    let project_id = job.project_id.clone().ok_or("缺少 projectId")?;
    let script = job.payload.get("script").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let _ = job.payload.get("followOriginal").and_then(|v| v.as_bool()).unwrap_or(false);

    emit(ProgressMsg { task_id: job.id.clone(), progress: 10.0, status: "running".into(),
        message: Some("Parsing segments".into()), payload: None });
    let segs = parse_script_to_draft_segments(&script);
    if segs.is_empty() { return Err("no segments to export".into()); }

    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let safe = sanitize_filename(&project_id);
    let base = match job.payload.get("outDir").and_then(|v| v.as_str()) {
        Some(d) if !d.trim().is_empty() => PathBuf::from(d.trim()),
        _ => { let mut b = data_dir.to_path_buf(); b.push("jianying_drafts_intl"); b }
    };
    let draft_root = base.join(format!("{safe}_{ts}"));
    std::fs::create_dir_all(&draft_root).map_err(|e| format!("create dir failed: {e}"))?;
    std::fs::create_dir_all(draft_root.join("video")).ok();
    std::fs::create_dir_all(draft_root.join("audio")).ok();
    std::fs::create_dir_all(draft_root.join("subtitle")).ok();

    // 真实素材：切视频 + 复制分段配音（与国内版共用逻辑）
    let ff = FfMpeg::ensure(data_dir).await?;
    let video_path = job.payload.get("videoPath").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let dub_dir = data_dir.join("dub").join(&safe);
    let has_audio = cut_real_materials(&ff, &video_path, &segs, &dub_dir, &draft_root, emit.clone(), &job.id, 15.0, 65.0).await;

    // 国际版：material path 用绝对路径
    let content = build_jianying_content_intl_real(&segs, &has_audio, &draft_root);
    std::fs::write(draft_root.join("draft_content.json"),
        serde_json::to_string_pretty(&content).map_err(|e| format!("serialize failed: {e}"))?)
        .map_err(|e| format!("write draft_content.json failed: {e}"))?;

    let ts_ms = ts * 1000;
    let meta = serde_json::json!({
        "tm_draft_create": ts_ms, "tm_draft_modified": ts_ms,
        "draft_id": project_id, "draft_name": format!("VF_{}", project_id),
        "draft_fold_path": draft_root.file_name().unwrap().to_string_lossy().to_string(),
        "tm_duration": (segs.last().map(|s| s.end).unwrap_or(0.0) * 1_000_000.0) as i64,
        "cover": "", "fps": 30.0,
        "platform": { "app_id": 3704, "app_source": "lv", "app_version": "9.0.0", "device_id": "vf-capcut" },
        "draft_cover": "", "draft_root_path": draft_root.display().to_string(),
        "draft_removable_storage_device": "",
        "group_id": "",
    });
    std::fs::write(draft_root.join("draft_meta_info.json"), serde_json::to_string_pretty(&meta).unwrap_or_default()).ok();
    std::fs::write(draft_root.join("draft_extra_info.json"), serde_json::to_string_pretty(&serde_json::json!({"extra_info": null})).unwrap_or_default()).ok();

    let audio_cnt = has_audio.iter().filter(|x| **x).count();
    emit(ProgressMsg { task_id: job.id.clone(), progress: 95.0, status: "running".into(),
        message: Some(format!("CapCut draft: {} clips / {} audio", segs.len(), audio_cnt)), payload: None });
    Ok(draft_root.display().to_string())
}

fn build_jianying_content_intl_real(segs: &[DraftSegment], has_audio: &[bool], draft_root: &std::path::Path) -> serde_json::Value {
    let mut videos: Vec<serde_json::Value> = Vec::new();
    for (i, s) in segs.iter().enumerate() {
        let dur_us = ((s.end - s.start) * 1_000_000.0).max(0.0) as i64;
        let start_us = (s.start * 1_000_000.0) as i64;
        videos.push(serde_json::json!({
            "id": format!("seg_v_{i}"), "type": "video", "material_id": format!("mat_v_{i}"),
            "source_timerange": { "start": 0, "duration": dur_us },
            "target_timerange": { "start": start_us, "duration": dur_us },
            "speed": 1.0, "volume": 1.0, "visible": true, "extra_material_refs": []
        }));
    }
    let mut subs: Vec<serde_json::Value> = Vec::new();
    for (i, s) in segs.iter().enumerate() {
        let dur_us = ((s.end - s.start) * 1_000_000.0).max(0.0) as i64;
        let start_us = (s.start * 1_000_000.0) as i64;
        subs.push(serde_json::json!({
            "id": format!("seg_s_{i}"), "type": "text",
            "target_timerange": { "start": start_us, "duration": dur_us },
            "speed": 1.0, "visible": true,
            "material_id": format!("mat_s_{i}"),
            "extra_material_refs": []
        }));
    }
    let materials_video: Vec<serde_json::Value> = segs.iter().enumerate().map(|(i, s)| {
        let p = draft_root.join(format!("video/seg_{:03}.mp4", i));
        serde_json::json!({
            "id": format!("mat_v_{i}"),
            "type": "video",
            "path": p.display().to_string(),
            "material_name": format!("seg_{}", i),
            "duration": ((s.end - s.start) * 1_000_000.0) as i64,
            "width": 1280, "height": 720,
            "has_audio": false,
            "has_sound_separated": false
        })
    }).collect();
    let materials_text: Vec<serde_json::Value> = segs.iter().enumerate().map(|(i, s)| {
        serde_json::json!({
            "id": format!("mat_s_{i}"),
            "type": "text",
            "content": { "text": s.text, "font": { "id": "default" }, "styles": [] }
        })
    }).collect();

    let mut tracks: Vec<serde_json::Value> = vec![
        serde_json::json!({
            "id": "main_video", "type": "video", "attribute": 0, "flag": 0, "segments": videos
        }),
        serde_json::json!({
            "id": "main_subtitle", "type": "text", "attribute": 0, "flag": 0, "segments": subs
        }),
    ];
    let mut materials_audio: Vec<serde_json::Value> = Vec::new();
    if has_audio.iter().any(|x| *x) {
        let mut audios: Vec<serde_json::Value> = Vec::new();
        for (i, s) in segs.iter().enumerate() {
            if !has_audio.get(i).copied().unwrap_or(false) { continue; }
            let dur_us = ((s.end - s.start) * 1_000_000.0).max(0.0) as i64;
            let start_us = (s.start * 1_000_000.0) as i64;
            audios.push(serde_json::json!({
                "id": format!("seg_a_{i}"), "type": "audio", "material_id": format!("mat_a_{i}"),
                "source_timerange": { "start": 0, "duration": dur_us },
                "target_timerange": { "start": start_us, "duration": dur_us },
                "speed": 1.0, "volume": 1.0, "visible": true, "extra_material_refs": []
            }));
            let p = draft_root.join(format!("audio/seg_{:03}.wav", i));
            materials_audio.push(serde_json::json!({
                "id": format!("mat_a_{i}"), "type": "audio",
                "path": p.display().to_string(),
                "duration": dur_us
            }));
        }
        tracks.push(serde_json::json!({
            "id": "main_audio", "type": "audio", "attribute": 0, "flag": 0, "segments": audios
        }));
    }

    serde_json::json!({
        "id": format!("VF_INTL_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)),
        "canvas_config": { "width": 1280, "height": 720, "ratio": "16:9" },
        "duration": (segs.last().map(|s| s.end).unwrap_or(0.0) * 1_000_000.0) as i64,
        "fps": { "denominator": 1, "numerator": 30 },
        "tracks": tracks,
        "materials": {
            "videos": materials_video,
            "texts": materials_text,
            "audios": materials_audio,
            "audio_effects": [], "audio_fades": [], "audio_indexes": [],
            "video_effects": [], "transitions": [], "effects": [],
            "filters": [], "animations": [], "stickers": [], "speech_to_songs": [],
            "placeholders": []
        },
        "relations": [],
        "version": 3,
        "platform": { "app_id": 3704, "app_source": "lv" }
    })
}

// ===========================================================================
// 分镜合成辅助：单段 ASS 字幕 / 真实素材切割 / 成片合成（预览 & 导出成片共用）
// ===========================================================================

/// 由单段解说词生成 ASS 字幕文本（保留函数以备兼容，当前 render_composite 已切换到 drawtext 方案）。
#[allow(dead_code)]
fn build_segment_ass(text: &str, style_choice: &str, dur: f64) -> String {
    let (primary, outline, shadow) = if style_choice.contains("无边框") {
        ("&H00FFFFFF", 0, 0)
    } else if style_choice.contains("阴影") {
        ("&H00000000", 0, 2)
    } else {
        // 经典：白字厚黑边+阴影，确保在任何视频背景上清晰可读
        ("&H00FFFFFF", 3, 2)
    };
    // ASS 是 CSV 逗号分隔格式，Fontname 字段内含逗号会导致整行字段错位（字号/颜色/对齐全偏移）→
    // 字幕渲染不可见。必须使用单一字体名，libass 缺失时会自动回退系统默认。
    let fontname = "Microsoft YaHei";
    let style = format!(
        "Style: Default,{fontname},48,{primary},&H00000000,&H00000000,-1,0,0,0,100,100,0,0,1,{outline},{shadow},2,20,35,1"
    );
    let safe = text.replace('\\', "\\\\").replace(',', "\\,").replace('\n', "\\N");
    format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n{style}\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,{},Default,,0,0,0,,{}\n",
        ffmpeg::ass_time(dur), safe
    )
}

/// 把分镜逐段合成（切片 → 烧字幕 → 可选混入分段配音），返回每段最终 mp4 路径（已烧字幕、必要时已混入配音）。
async fn render_composite(
    ff: &FfMpeg,
    video_path: &str,
    segs: &[DraftSegment],
    dub_dir: &Path,
    subtitle_style: &str,
    mix_voice: bool,
    out_dir: &Path,
    emit: Arc<dyn Fn(ProgressMsg) + Send + Sync>,
    task_id: &str,
    progress_base: f64,
    progress_span: f64,
) -> Result<Vec<std::path::PathBuf>, String> {
    let n = segs.len();
    let mut parts: Vec<std::path::PathBuf> = Vec::with_capacity(n);
    for (i, s) in segs.iter().enumerate() {
        let prog = progress_base + (i as f64 / n.max(1) as f64) * progress_span;
        emit(ProgressMsg {
            task_id: task_id.to_string(),
            progress: prog,
            status: "running".into(),
            message: Some(format!("合成片段 {}/{}", i + 1, n)),
            payload: None,
        });
        if s.end <= s.start {
            continue;
        }
        let seg = out_dir.join(format!("seg_{:03}.mp4", i));
        if !ff.segment_cmd(video_path, s.start, s.end, seg.to_str().unwrap()).output().map_err(|e| e.to_string())?.status.success() {
            emit(ProgressMsg {
                task_id: task_id.to_string(),
                progress: prog,
                status: "running".into(),
                message: Some(format!("片段 {}/{} 切片失败，跳过", i + 1, n)),
                payload: None,
            });
            continue;
        }
        // 烧字幕（用 drawtext 滤镜：比 ASS 更可靠，不依赖 libass，任何分辨率都稳定）
        let _ = subtitle_style; // 保留参数兼容（drawtext 内置样式）
        if s.text.trim().is_empty() {
            let _ = std::fs::copy(&seg, &out_dir.join(format!("burn_{:03}.mp4", i)));
            continue;
        }
        let text_path = out_dir.join(format!("seg_{:03}.txt", i));
        if let Err(e) = std::fs::write(&text_path, &s.text) {
            emit(ProgressMsg {
                task_id: task_id.to_string(),
                progress: prog,
                status: "running".into(),
                message: Some(format!("片段 {}/{} 字幕文件写入失败: {}（将保留无字幕片段）", i + 1, n, e)),
                payload: None,
            });
            let _ = std::fs::copy(&seg, &out_dir.join(format!("burn_{:03}.mp4", i)));
            continue;
        }
        let burned = out_dir.join(format!("burn_{:03}.mp4", i));
        let burn_out = ff
            .burn_subtitle_cmd(seg.to_str().unwrap(), text_path.to_str().unwrap(), burned.to_str().unwrap())
            .output();
        match burn_out {
            Ok(o) if o.status.success() => {
                let preview: String = s.text.chars().take(24).collect();
                emit(ProgressMsg {
                    task_id: task_id.to_string(),
                    progress: prog,
                    status: "running".into(),
                    message: Some(format!("片段 {}/{} 字幕已烧录: 「{}…」", i + 1, n, preview)),
                    payload: None,
                });
            }
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr);
                let tail: String = err.lines().rev().take(3).collect::<Vec<_>>().join(" | ");
                emit(ProgressMsg {
                    task_id: task_id.to_string(),
                    progress: prog,
                    status: "running".into(),
                    message: Some(format!("片段 {}/{} 字幕烧录失败: {}（回退无字幕片段）", i + 1, n, tail)),
                    payload: None,
                });
                let _ = std::fs::copy(&seg, &burned);
            }
            Err(e) => {
                emit(ProgressMsg {
                    task_id: task_id.to_string(),
                    progress: prog,
                    status: "running".into(),
                    message: Some(format!("片段 {}/{} 字幕烧录命令启动失败: {}（回退无字幕片段）", i + 1, n, e)),
                    payload: None,
                });
                let _ = std::fs::copy(&seg, &burned);
            }
        }
        // 混音：若批量配音已生成对应 wav，则替换为配音音轨；否则保留原声
        let final_seg = out_dir.join(format!("vo_{:03}.mp4", i));
        let dub_file = dub_dir.join(format!("seg_{:03}.wav", i));
        if mix_voice && dub_file.exists() {
            // 关键：把 TTS 音频精确归一化到切片时长 dur，避免配音与画面因时长不一致而累积漂移
            let dur = s.end - s.start;
            let tts_dur = ff.probe_duration(dub_file.to_str().unwrap()).unwrap_or(dur);
            let use_dub: String = if (tts_dur - dur).abs() > 0.05 {
                let norm = out_dir.join(format!("norm_{:03}.wav", i));
                if ff.normalize_audio_to(dub_file.to_str().unwrap(), norm.to_str().unwrap(), tts_dur, dur)
                    .output().map(|o| o.status.success()).unwrap_or(false)
                {
                    norm.to_string_lossy().to_string()
                } else {
                    dub_file.to_string_lossy().to_string()
                }
            } else {
                dub_file.to_string_lossy().to_string()
            };
            if !ff.mux_cmd(burned.to_str().unwrap(), &use_dub, final_seg.to_str().unwrap()).output().map_err(|e| e.to_string())?.status.success() {
                let _ = std::fs::copy(&burned, &final_seg);
            }
        } else {
            let _ = std::fs::copy(&burned, &final_seg);
        }
        parts.push(final_seg);
    }
    if parts.is_empty() {
        return Err("没有可合成的片段（请检查分镜时间轴或源视频）".into());
    }
    Ok(parts)
}

/// 导出剪映草稿时，逐段从源片切出真实视频素材并复制分段配音到草稿目录。
/// 返回 has_audio：每段是否存在对应配音文件。
async fn cut_real_materials(
    ff: &FfMpeg,
    video_path: &str,
    segs: &[DraftSegment],
    dub_dir: &Path,
    draft_root: &Path,
    emit: Arc<dyn Fn(ProgressMsg) + Send + Sync>,
    task_id: &str,
    progress_base: f64,
    progress_span: f64,
) -> Vec<bool> {
    let n = segs.len();
    let mut has_audio: Vec<bool> = Vec::with_capacity(n);
    for (i, s) in segs.iter().enumerate() {
        let prog = progress_base + (i as f64 / n.max(1) as f64) * progress_span;
        emit(ProgressMsg {
            task_id: task_id.to_string(),
            progress: prog,
            status: "running".into(),
            message: Some(format!("切素材 {}/{}", i + 1, n)),
            payload: None,
        });
        let mut has = false;
        if s.end > s.start {
            let vout = draft_root.join("video").join(format!("seg_{:03}.mp4", i));
            if !ff.segment_cmd(video_path, s.start, s.end, vout.to_str().unwrap()).output().map_err(|e| e.to_string()).map(|o| o.status.success()).unwrap_or(false) {
                emit(ProgressMsg {
                    task_id: task_id.to_string(),
                    progress: prog,
                    status: "running".into(),
                    message: Some(format!("片段 {}/{} 切片失败（源片可能缺失）", i + 1, n)),
                    payload: None,
                });
            }
            let dub_src = dub_dir.join(format!("seg_{:03}.wav", i));
            if dub_src.exists() {
                let aout = draft_root.join("audio").join(format!("seg_{:03}.wav", i));
                if std::fs::copy(&dub_src, &aout).is_ok() {
                    has = true;
                }
            }
        }
        has_audio.push(has);
    }
    has_audio
}

/// 导出成片 / 预览前，确保每段配音 wav 已存在；缺失则自动调用 TTS 合成（需配置 TTS Key）。
/// 失败不影响整体（后续回退原声），但会 emit 警告让用户知道。
async fn ensure_dub_for_segs(
    pool: &SqlitePool,
    client: &Client,
    segs: &[DraftSegment],
    dub_dir: &Path,
    emit: Arc<dyn Fn(ProgressMsg) + Send + Sync>,
    task_id: &str,
    progress_base: f64,
    progress_span: f64,
) {
    let n = segs.len();
    if n == 0 { return; }
    std::fs::create_dir_all(dub_dir).ok();
    let mut ok = 0usize;
    let mut fail = 0usize;
    for (i, s) in segs.iter().enumerate() {
        let wav = dub_dir.join(format!("seg_{:03}.wav", i));
        if wav.exists() { ok += 1; continue; }
        if s.text.trim().is_empty() { continue; }
        let prog = progress_base + (i as f64 / n.max(1) as f64) * progress_span;
        emit(ProgressMsg {
            task_id: task_id.to_string(),
            progress: prog,
            status: "running".into(),
            message: Some(format!("自动合成配音 {}/{}", i + 1, n)),
            payload: None,
        });
        let voice = s.voice.clone().unwrap_or_else(|| "default".into());
        match tts_one_segment(pool, client, &s.text, &voice, &wav).await {
            Ok(_) => { ok += 1; }
            Err(e) => {
                fail += 1;
                emit(ProgressMsg {
                    task_id: task_id.to_string(),
                    progress: prog,
                    status: "running".into(),
                    message: Some(format!("配音 {}/{} 失败：{}（将保留原声）", i + 1, n, e)),
                    payload: None,
                });
            }
        }
    }
    if fail > 0 {
        emit(ProgressMsg {
            task_id: task_id.to_string(),
            progress: progress_base + progress_span,
            status: "running".into(),
            message: Some(format!("配音就绪 {}/{}，{} 段失败（缺失 TTS Key 或未配置语音合成）", ok, n, fail)),
            payload: None,
        });
    }
}

/// 预览成片：合成「原片 + 分段配音 + 烧录字幕」到一个 MP4，写到 data_dir/preview/<safe>.mp4。
async fn run_film_render_preview(
    pool: &SqlitePool,
    client: &Client,
    data_dir: &Path,
    job: &TaskJob,
    emit: Arc<dyn Fn(ProgressMsg) + Send + Sync>,
) -> Result<String, String> {
    let project_id = job.project_id.clone().ok_or("缺少 projectId")?;
    let script = job.payload.get("script").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let video_path = job.payload.get("videoPath").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let mix_voice = job.payload.get("mixVoice").and_then(|v| v.as_bool()).unwrap_or(true);
    let subtitle_style = job.payload.get("subtitleStyle").and_then(|v| v.as_str()).unwrap_or("经典-白字黑边").to_string();

    emit(ProgressMsg { task_id: job.id.clone(), progress: 5.0, status: "running".into(),
        message: Some("解析分镜".into()), payload: None });
    let mut segs = parse_script_to_draft_segments(&script);
    // 解说词时间点相对「确认范围」起点（rangeStart），而源视频是整片；需整体偏移回绝对时间，
    // 否则一旦 rangeStart>0，字幕/配音会与画面整体错位 rangeStart 秒。
    let range_start = job.payload.get("rangeStart").and_then(|v| v.as_f64()).unwrap_or(0.0);
    if range_start > 0.0 {
        for s in &mut segs { s.start += range_start; s.end += range_start; }
    }
    if segs.is_empty() { return Err("无可导出的分镜".into()); }
    if video_path.is_empty() { return Err("缺少源视频路径".into()); }

    let ff = FfMpeg::ensure(data_dir).await?;
    let safe = sanitize_filename(&project_id);
    let tmp = data_dir.join("tmp").join("preview").join(&safe);
    std::fs::create_dir_all(&tmp).ok();
    let dub_dir = data_dir.join("dub").join(&safe);

    // 导出/预览前自动补齐缺失的配音（需 TTS Key），确保成片是真·带新配音的版本
    if mix_voice {
        ensure_dub_for_segs(pool, client, &segs, &dub_dir, emit.clone(), &job.id, 8.0, 12.0).await;
    }

    let parts = render_composite(&ff, &video_path, &segs, &dub_dir, &subtitle_style, mix_voice, &tmp, emit.clone(), &job.id, 20.0, 60.0).await?;

    emit(ProgressMsg { task_id: job.id.clone(), progress: 85.0, status: "running".into(),
        message: Some("拼接成片".into()), payload: None });
    let list = tmp.join("concat.txt");
    let content: String = parts.iter().map(|p| format!("file '{}'", p.to_string_lossy().replace('\\', "/"))).collect::<Vec<_>>().join("\n");
    std::fs::write(&list, content).ok();
    let rough = tmp.join("rough.mp4");
    ff.concat_cmd(list.to_str().unwrap(), rough.to_str().unwrap()).output().map_err(|e| e.to_string())?;
    let out_dir = match job.payload.get("outDir").and_then(|v| v.as_str()) {
        Some(d) if !d.trim().is_empty() => PathBuf::from(d.trim()),
        _ => { let mut b = data_dir.to_path_buf(); b.push("preview"); b }
    };
    std::fs::create_dir_all(&out_dir).ok();
    let out = out_dir.join(format!("{safe}.mp4"));
    ff.export_cmd(rough.to_str().unwrap(), out.to_str().unwrap(), false, "").output().map_err(|e| e.to_string())?;
    Ok(out.to_string_lossy().to_string())
}

/// 导出成片 MP4：与预览同一套合成逻辑，输出到 data_dir/export/<safe>_<ts>.mp4。
async fn run_film_export_final(
    pool: &SqlitePool,
    client: &Client,
    data_dir: &Path,
    job: &TaskJob,
    emit: Arc<dyn Fn(ProgressMsg) + Send + Sync>,
) -> Result<String, String> {
    let project_id = job.project_id.clone().ok_or("缺少 projectId")?;
    let script = job.payload.get("script").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let video_path = job.payload.get("videoPath").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let mix_voice = job.payload.get("mixVoice").and_then(|v| v.as_bool()).unwrap_or(true);
    let subtitle_style = job.payload.get("subtitleStyle").and_then(|v| v.as_str()).unwrap_or("经典-白字黑边").to_string();

    emit(ProgressMsg { task_id: job.id.clone(), progress: 5.0, status: "running".into(),
        message: Some("解析分镜".into()), payload: None });
    let mut segs = parse_script_to_draft_segments(&script);
    // 解说词时间点相对「确认范围」起点（rangeStart），而源视频是整片；需整体偏移回绝对时间，
    // 否则一旦 rangeStart>0，字幕/配音会与画面整体错位 rangeStart 秒。
    let range_start = job.payload.get("rangeStart").and_then(|v| v.as_f64()).unwrap_or(0.0);
    if range_start > 0.0 {
        for s in &mut segs { s.start += range_start; s.end += range_start; }
    }
    if segs.is_empty() { return Err("无可导出的分镜".into()); }
    if video_path.is_empty() { return Err("缺少源视频路径".into()); }

    let ff = FfMpeg::ensure(data_dir).await?;
    let safe = sanitize_filename(&project_id);
    let tmp = data_dir.join("tmp").join("export_final").join(&safe);
    std::fs::create_dir_all(&tmp).ok();
    let dub_dir = data_dir.join("dub").join(&safe);

    // 导出/预览前自动补齐缺失的配音（需 TTS Key），确保成片是真·带新配音的版本
    if mix_voice {
        ensure_dub_for_segs(pool, client, &segs, &dub_dir, emit.clone(), &job.id, 8.0, 12.0).await;
    }

    let parts = render_composite(&ff, &video_path, &segs, &dub_dir, &subtitle_style, mix_voice, &tmp, emit.clone(), &job.id, 20.0, 60.0).await?;

    emit(ProgressMsg { task_id: job.id.clone(), progress: 85.0, status: "running".into(),
        message: Some("拼接成片".into()), payload: None });
    let list = tmp.join("concat.txt");
    let content: String = parts.iter().map(|p| format!("file '{}'", p.to_string_lossy().replace('\\', "/"))).collect::<Vec<_>>().join("\n");
    std::fs::write(&list, content).ok();
    let rough = tmp.join("rough.mp4");
    ff.concat_cmd(list.to_str().unwrap(), rough.to_str().unwrap()).output().map_err(|e| e.to_string())?;
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let out_dir = match job.payload.get("outDir").and_then(|v| v.as_str()) {
        Some(d) if !d.trim().is_empty() => PathBuf::from(d.trim()),
        _ => { let mut b = data_dir.to_path_buf(); b.push("export"); b }
    };
    std::fs::create_dir_all(&out_dir).ok();
    let out = out_dir.join(format!("{safe}_{ts}.mp4"));
    ff.export_cmd(rough.to_str().unwrap(), out.to_str().unwrap(), false, "").output().map_err(|e| e.to_string())?;
    Ok(out.to_string_lossy().to_string())
}

/// 导出 SRT：把前端生成好的字幕内容写入指定文件夹（outDir）下的 <project>.srt；
/// 未指定 outDir 时回退到 data_dir/export/。
async fn run_film_export_srt(
    _pool: &SqlitePool,
    data_dir: &Path,
    job: &TaskJob,
    _emit: Arc<dyn Fn(ProgressMsg) + Send + Sync>,
) -> Result<String, String> {
    let project_id = job.project_id.clone().ok_or("缺少 projectId")?;
    let content = job.payload.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if content.trim().is_empty() { return Err("SRT 内容为空".into()); }
    let safe = sanitize_filename(&project_id);
    let out_dir = match job.payload.get("outDir").and_then(|v| v.as_str()) {
        Some(d) if !d.trim().is_empty() => PathBuf::from(d.trim()),
        _ => { let mut b = data_dir.to_path_buf(); b.push("export"); b }
    };
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("建目录失败: {e}"))?;
    let out = out_dir.join(format!("{safe}.srt"));
    std::fs::write(&out, content).map_err(|e| format!("写 SRT 失败: {e}"))?;
    Ok(out.to_string_lossy().to_string())
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
    segs: &[(f64, f64, String)],
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
    b.push_str("\n\n## 二、画面时间节点与内容理解（供字幕与画面对齐，每条字幕对应一个窗口的真实内容）\n");
    for (i, (s, e, d)) in segs.iter().enumerate() {
        b.push_str(&format!("{}. `{} - {}` — {}\n", i + 1, va_fmt_ts(*s), va_fmt_ts(*e), d));
    }
    // 机器可解析标记：解说生成阶段据此精确提取「逐窗口内容理解」（相对片段秒数 + 中文内容）
    let script_raw: String = segs
        .iter()
        .map(|(s, e, d)| {
            let d = d.replace('|', "｜").replace(';', "；");
            format!("{:.2}-{:.2}|{}", s, e, d)
        })
        .collect::<Vec<_>>()
        .join(";;");
    b.push_str(&format!("\n<!--SCENE_SCRIPT:{}-->\n", script_raw));
    // 兼容旧版解析：保留时间节点标记
    let nodes_raw: String = segs
        .iter()
        .map(|(s, e, _)| format!("{:.2}-{:.2}", s, e))
        .collect::<Vec<_>>()
        .join(",");
    b.push_str(&format!("<!--SCENE_NODES:{}-->\n", nodes_raw));
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
        eprintln!("[film-analysis] step {s}: {msg}");
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

    // ② 检测场景切换点（基于多模态视觉分割；ffmpeg scene 滤镜在本机对合成/部分素材不触发）
    step(2, 20.0, "检测场景切换点".into());
    let frames = list_frames(&frames_dir);
    let seg3 = vision_segment_frames(&frames, dur_seg, pool, client).await;
    let scenes: Vec<f64> = seg3.iter().map(|(s, _, _)| *s).collect();
    step(2, 32.0, format!("检测场景切换点完成（{} 个）", scenes.len()));

    // ③ 多维度特征编码中
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

    // ⑤ 语义块解析中（直接采用视觉分割得到的真实段落边界，相对片段 0 基；并取每段代表帧做「逐窗口详细内容理解」）
    let segs: Vec<(f64, f64, String)> = if seg3.is_empty() {
        vec![(0.0, dur_seg, String::new())]
    } else {
        // 为每段取一张中点代表帧，调用多模态模型写出该窗口的详细内容理解
        let n_windows = seg3.len();
        let rep_frames: Vec<std::path::PathBuf> = if frames.is_empty() {
            Vec::new()
        } else {
            (0..n_windows)
                .map(|i| {
                    let pos = ((i as f64 + 0.5) / n_windows as f64 * frames.len() as f64) as usize;
                    let pos = pos.min(frames.len() - 1);
                    frames[pos].clone()
                })
                .collect()
        };
        let win_bounds: Vec<(f64, f64)> = seg3.iter().map(|(s, e, _)| (*s, *e)).collect();
        let detailed = vision_describe_windows(&rep_frames, &win_bounds, pool, client).await;
        seg3
            .iter()
            .enumerate()
            .map(|(i, (s, e, d))| {
                let d2 = detailed
                    .get(i)
                    .cloned()
                    .filter(|x| !x.trim().is_empty())
                    .unwrap_or_else(|| d.clone());
                (*s, *e, d2)
            })
            .collect()
    };
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
    // DB 落库失败不应阻断报告回传：即便写入失败，仍按 step=10 把完整报告经 Channel 发回前端
    let set_res = db::film_project_set_analysis(pool, &project_id, &report).await;
    eprintln!("[film-analysis] db set_analysis project_id={project_id} report_len={} ok={}", report.len(), set_res.is_ok());
    step(9, 99.0, "输出流水线生成中".into());

    // ⑩ 最终影片分析内容总结报告
    step(10, 100.0, "最终影片分析内容总结报告".into());
    // 报告已在 step=9 落库；done 仅发完成信号（不带大 report），前端收到后从库读取完整报告
    eprintln!("[film-analysis] done emitted, report_len={}", report.len());
    emit(ProgressMsg {
        task_id: job_id.clone(),
        progress: 100.0,
        status: "done".into(),
        message: Some("影片分析完成".into()),
        payload: Some(serde_json::json!({ "step": 10 })),
    });

    Ok(())
}

// ===========================================================================
// M5：创作模块「首尾帧视频 / 配音 / 导出」任务
// ===========================================================================

/// M5-① 首尾帧视频：逐镜由「首帧图」生成运镜视频片段（Ken Burns），可选尾帧则 crossfade。
async fn run_creation_frames(
    pool: &SqlitePool,
    data_dir: &Path,
    job: &TaskJob,
    emit: Arc<dyn Fn(ProgressMsg) + Send + Sync>,
) -> Result<(), String> {
    let project_id = job.project_id.clone().ok_or("creation_frames 缺少 projectId")?;
    let sb = db::storyboard_get(pool, &project_id).await?
        .ok_or_else(|| "请先生成分镜".to_string())?;
    let shots: Vec<serde_json::Value> = serde_json::from_str(&sb.shots).map_err(|e| e.to_string())?;
    if shots.is_empty() { return Err("分镜为空，请先生成分镜".into()); }

    // 可选尾帧（前端上传，{"i": "absPath"}）
    let tails_raw = job.payload.get("tails").and_then(|v| v.as_str()).unwrap_or("{}");
    let tails: std::collections::HashMap<String, String> = serde_json::from_str(tails_raw).unwrap_or_default();

    // 收集该工程所有已生成图片（首帧）按 shot_id
    let assets = db::generated_assets_list(pool, &project_id).await.unwrap_or_default();
    let mut img_by_shot: std::collections::HashMap<i64, String> = std::collections::HashMap::new();
    for a in assets {
        if a.kind == "image" {
            img_by_shot.insert(a.shot_id, a.path.clone()); // 后写覆盖，取最新一张
        }
    }

    let ff = FfMpeg::ensure(data_dir).await?;
    let clip_dir = data_dir.join("creation_clips").join(sanitize_filename(&project_id));
    std::fs::create_dir_all(&clip_dir).map_err(|e| e.to_string())?;

    let mut manifest = read_creation_manifest(data_dir, &project_id);
    let total = shots.len();
    let mut ok = 0usize;
    for (i, shot) in shots.iter().enumerate() {
        let idx = shot.get("index").and_then(|x| x.as_i64()).unwrap_or(i as i64);
        let dur = shot.get("dur").and_then(|x| x.as_f64()).unwrap_or(4.0).max(1.0);
        let prog = ((i as f64) / (total as f64) * 90.0).clamp(0.0, 90.0);
        emit(ProgressMsg {
            task_id: job.id.clone(), progress: prog, status: "running".into(),
            message: Some(format!("生成片段 {}/{}", i + 1, total)), payload: None,
        });
        let img = match img_by_shot.get(&idx) {
            Some(p) => p.clone(),
            None => {
                emit(ProgressMsg {
                    task_id: job.id.clone(), progress: prog, status: "running".into(),
                    message: Some(format!("镜头 {} 缺少生成图，跳过（请先在「图片」步生成）", idx + 1)), payload: None,
                });
                continue;
            }
        };
        let out = clip_dir.join(format!("seg_{:03}.mp4", i));
        let mut cmd = if let Some(tail) = tails.get(&idx.to_string()) {
            if std::path::Path::new(tail).exists() {
                ff.gen_clip_xfade_cmd(&img, tail, dur, out.to_str().unwrap())
            } else {
                ff.gen_clip_single_cmd(&img, dur, out.to_str().unwrap())
            }
        } else {
            ff.gen_clip_single_cmd(&img, dur, out.to_str().unwrap())
        };
        match cmd.output() {
            Ok(o) if o.status.success() => {
                ok += 1;
                manifest.clips.insert(idx.to_string(), out.to_string_lossy().to_string());
            }
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr).chars().take(300).collect::<String>();
                emit(ProgressMsg {
                    task_id: job.id.clone(), progress: prog, status: "running".into(),
                    message: Some(format!("镜头 {} 片段生成失败: {}", idx + 1, err)), payload: None,
                });
            }
            Err(e) => {
                emit(ProgressMsg {
                    task_id: job.id.clone(), progress: prog, status: "running".into(),
                    message: Some(format!("镜头 {} 片段执行失败: {}", idx + 1, e)), payload: None,
                });
            }
        }
    }
    for (k, v) in &tails { manifest.tails.insert(k.clone(), v.clone()); }
    write_creation_manifest(data_dir, &project_id, &manifest)?;
    if ok == 0 { return Err("未生成任何片段（请先在「图片」步为镜头生成图片）".into()); }
    db::creation_project_update(pool, &project_id, None, None, None, Some("frames")).await.ok();
    emit(ProgressMsg {
        task_id: job.id.clone(), progress: 95.0, status: "running".into(),
        message: Some(format!("已生成 {}/{} 个片段", ok, total)), payload: None,
    });
    Ok(())
}

/// M5-② 配音+字幕素材：逐镜台词走 TTS 生成 wav（复用 XiaomiMiMo 协议）。
async fn run_creation_voice(
    pool: &SqlitePool,
    client: &Client,
    data_dir: &Path,
    job: &TaskJob,
    emit: Arc<dyn Fn(ProgressMsg) + Send + Sync>,
) -> Result<(), String> {
    let project_id = job.project_id.clone().ok_or("creation_voice 缺少 projectId")?;
    let voice = job.payload.get("voice").and_then(|v| v.as_str()).unwrap_or("default").to_string();
    let sb = db::storyboard_get(pool, &project_id).await?
        .ok_or_else(|| "请先生成分镜".to_string())?;
    let shots: Vec<serde_json::Value> = serde_json::from_str(&sb.shots).map_err(|e| e.to_string())?;
    if shots.is_empty() { return Err("分镜为空".into()); }

    let dub_dir = data_dir.join("creation_dub").join(sanitize_filename(&project_id));
    std::fs::create_dir_all(&dub_dir).ok();

    let mut manifest = read_creation_manifest(data_dir, &project_id);
    let total = shots.len();
    let mut ok = 0usize;
    for (i, shot) in shots.iter().enumerate() {
        let idx = shot.get("index").and_then(|x| x.as_i64()).unwrap_or(i as i64);
        let text = shot.get("dialogue").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
        let prog = ((i as f64) / (total as f64) * 90.0).clamp(0.0, 90.0);
        if text.is_empty() {
            emit(ProgressMsg {
                task_id: job.id.clone(), progress: prog, status: "running".into(),
                message: Some(format!("镜头 {} 无台词，跳过", idx + 1)), payload: None,
            });
            continue;
        }
        emit(ProgressMsg {
            task_id: job.id.clone(), progress: prog, status: "running".into(),
            message: Some(format!("配音 {}/{} ({}…)", i + 1, total, &text.chars().take(12).collect::<String>())),
            payload: None,
        });
        let wav = dub_dir.join(format!("seg_{:03}.wav", i));
        match tts_one_segment(pool, client, &text, &voice, &wav).await {
            Ok(_) => {
                ok += 1;
                manifest.audios.insert(idx.to_string(), wav.to_string_lossy().to_string());
            }
            Err(e) => {
                emit(ProgressMsg {
                    task_id: job.id.clone(), progress: prog, status: "running".into(),
                    message: Some(format!("镜头 {} 配音失败: {}", idx + 1, e)), payload: None,
                });
            }
        }
    }
    write_creation_manifest(data_dir, &project_id, &manifest)?;
    if ok == 0 { return Err("未生成任何配音（请确认镜头有台词且已在「设置→接口」配置 TTS Key）".into()); }
    db::creation_project_update(pool, &project_id, None, None, None, Some("voice")).await.ok();
    emit(ProgressMsg {
        task_id: job.id.clone(), progress: 95.0, status: "running".into(),
        message: Some(format!("已生成 {}/{} 段配音", ok, total)), payload: None,
    });
    Ok(())
}

/// M5-③ 导出成片：拼接镜头片段 + 混入配音 + 烧录字幕 → 最终 MP4。
async fn run_creation_export(
    pool: &SqlitePool,
    data_dir: &Path,
    job: &TaskJob,
    emit: Arc<dyn Fn(ProgressMsg) + Send + Sync>,
) -> Result<String, String> {
    let project_id = job.project_id.clone().ok_or("creation_export 缺少 projectId")?;
    let sb = db::storyboard_get(pool, &project_id).await?
        .ok_or_else(|| "请先生成分镜".to_string())?;
    let shots: Vec<serde_json::Value> = serde_json::from_str(&sb.shots).map_err(|e| e.to_string())?;
    if shots.is_empty() { return Err("分镜为空".into()); }

    let manifest = read_creation_manifest(data_dir, &project_id);
    if manifest.clips.is_empty() { return Err("请先在「首尾帧视频」步生成镜头片段".into()); }
    let sub_style = job.payload.get("subtitle_style").and_then(|v| v.as_str()).unwrap_or("standard");

    let ff = FfMpeg::ensure(data_dir).await?;
    let safe = sanitize_filename(&project_id);
    let out_dir = data_dir.join("creation_export").join(&safe);
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let subs_dir = out_dir.join("subs");
    std::fs::create_dir_all(&subs_dir).ok();

    let total = shots.len();
    let mut seg_paths: Vec<PathBuf> = Vec::with_capacity(total);
    for (i, shot) in shots.iter().enumerate() {
        let idx = shot.get("index").and_then(|x| x.as_i64()).unwrap_or(i as i64);
        let prog = ((i as f64) / (total as f64) * 80.0).clamp(0.0, 80.0);
        emit(ProgressMsg {
            task_id: job.id.clone(), progress: prog, status: "running".into(),
            message: Some(format!("合成镜头 {}/{}", i + 1, total)), payload: None,
        });
        let clip = match manifest.clips.get(&idx.to_string()) {
            Some(p) => p.clone(),
            None => return Err(format!("镜头 {} 缺少视频片段，请先生成首尾帧视频", idx + 1)),
        };
        if !std::path::Path::new(&clip).exists() {
            return Err(format!("镜头 {idx} 的视频片段文件不存在：{clip}", idx = idx + 1));
        }
        let dialogue = shot.get("dialogue").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let txt = subs_dir.join(format!("seg_{:03}.txt", i));
        std::fs::write(&txt, dialogue.as_bytes()).map_err(|e| e.to_string())?;

        let clip_dur = ff.probe_duration(&clip).unwrap_or(shot.get("dur").and_then(|x| x.as_f64()).unwrap_or(4.0));
        let audio = manifest.audios.get(&idx.to_string());
        let seg_out = out_dir.join(format!("seg_{:03}.mp4", i));
        let mut cmd = if let Some(a) = audio {
            if std::path::Path::new(a).exists() {
                let src_dur = ff.probe_duration(a).unwrap_or(clip_dur);
                let norm = out_dir.join(format!("seg_{:03}_audio.wav", i));
                let _ = ff.normalize_audio_to(a, norm.to_str().unwrap(), src_dur, clip_dur).status();
                ff.compose_seg_cmd(&clip, Some(norm.to_str().unwrap()), txt.to_str().unwrap(), seg_out.to_str().unwrap(), sub_style)
            } else {
                ff.compose_seg_cmd(&clip, None, txt.to_str().unwrap(), seg_out.to_str().unwrap(), sub_style)
            }
        } else {
            ff.compose_seg_cmd(&clip, None, txt.to_str().unwrap(), seg_out.to_str().unwrap(), sub_style)
        };
        let st = cmd.status().map_err(|e| format!("合成镜头 {} 失败: {e}", idx + 1))?;
        if !st.success() { return Err(format!("合成镜头 {} 失败（ffmpeg 退出码 {st}）", idx + 1)); }
        seg_paths.push(seg_out);
    }

    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let final_path = out_dir.join(format!("final_{ts}.mp4"));
    let list_path = out_dir.join("list.txt");
    let list_content: String = seg_paths.iter()
        .map(|p| format!("file '{}'\n", p.to_string_lossy().replace('\\', "/")))
        .collect();
    std::fs::write(&list_path, list_content).map_err(|e| e.to_string())?;
    emit(ProgressMsg {
        task_id: job.id.clone(), progress: 90.0, status: "running".into(),
        message: Some("拼接成片…".into()), payload: None,
    });
    let st = ff.concat_cmd(list_path.to_str().unwrap(), final_path.to_str().unwrap()).status()
        .map_err(|e| format!("拼接失败: {e}"))?;
    if !st.success() { return Err("拼接成片失败（ffmpeg 退出码）".into()); }

    let mut manifest = read_creation_manifest(data_dir, &project_id);
    manifest.exported = Some(final_path.to_string_lossy().to_string());
    write_creation_manifest(data_dir, &project_id, &manifest)?;
    db::creation_project_update(pool, &project_id, None, None, None, Some("done")).await.ok();
    Ok(final_path.to_string_lossy().to_string())
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
            role_prompt: "",
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
            role_prompt: "",
            scene_nodes: "1. 0:00-0:12\n2. 0:12-0:25\n3. 0:25-0:40",
        };
        let p = build_narration_prompt(&cfg);
        assert!(p.contains("画面时间节点"), "有节点时提示词应含画面时间节点块");
        assert!(p.contains("0:12-0:25"), "提示词应内联真实节点");
        assert!(p.contains("严格按上方"), "有节点时应要求严格对齐画面切换点");
        assert!(!p.contains("均匀分布"), "有节点时不应再要求均匀分布");
    }

    #[test]
    fn narration_prompt_injects_role_setting_when_provided() {
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
            asr_text: "",
            asr_failed: false,
            analysis: "",
            role_prompt: "你是一位毒舌但专业的电影解说人，说话犀利、爱用反问",
            scene_nodes: "",
        };
        let p = build_narration_prompt(&cfg);
        assert!(p.contains("角色设定"), "有角色设定时提示词应含角色设定块");
        assert!(p.contains("毒舌但专业的电影解说人"), "提示词应内联角色设定内容");

        // 空角色设定时不注入
        let cfg2 = NarrationConfig {
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
            asr_text: "",
            asr_failed: false,
            analysis: "",
            role_prompt: "",
            scene_nodes: "",
        };
        let p2 = build_narration_prompt(&cfg2);
        assert!(!p2.contains("角色设定"), "空角色设定时不应注入角色设定块");
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
    fn extract_scene_script_parses_marker_with_content() {
        let report = "# 报告\n<!--SCENE_SCRIPT:0.00-12.30|主角在厨房切菜，神情专注｜暖光;;12.30-25.10|妻子推门进来，两人对话，气氛温馨-->\n## 其它\n";
        let script = extract_scene_script(report);
        assert_eq!(script.len(), 2, "应解析出 2 个窗口");
        assert_eq!(script[0].0, 0.0);
        assert_eq!(script[0].1, 12.30);
        assert!(script[0].2.contains("主角在厨房切菜"), "窗口内容应被保留");
        assert!(script[1].2.contains("妻子推门进来"), "窗口内容应被保留");
    }

    #[test]
    fn extract_scene_script_fallback_parses_dash_content() {
        let report = "## 二、画面时间节点与内容理解\n1. `0:00 - 0:12` — 主角在雨中奔跑\n2. `0:12 - 0:30` — 反派现身\n";
        let script = extract_scene_script(report);
        assert_eq!(script.len(), 2, "无标记时应回退解析 `start - end — desc`");
        assert_eq!(script[0].1, 12.0);
        assert!(script[0].2.contains("雨中奔跑"));
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

    #[test]
    fn recover_narration_segments_from_tagged_text() {
        // 本地小模型常返回带 [段落] 标签的自然语言而非规范 JSON
        let text = "[开端] 0:00-0:20 这是第一段解说内容。\n[铺垫] 0:20-0:40 这是第二段。\n[冲突] 0:40-1:00 这是第三段。";
        let clips = recover_narration_segments(text, 60.0);
        assert_eq!(clips.len(), 3, "带标签文本应恢复为多段");
        assert_eq!(clips[0].label, "开端");
        assert_eq!(clips[1].label, "铺垫");
        assert_eq!(clips[2].label, "冲突");
        assert!(!clips[0].text.is_empty());
        assert!(!clips[1].text.is_empty(), "后续段不应为空");
        assert!(!clips[2].text.is_empty(), "后续段不应为空");
        assert!(clips[1].text.contains("第二段"));
        assert!(clips[2].text.contains("第三段"));
    }

    #[test]
    fn recover_narration_segments_from_plain_blocks() {
        // 纯分段文本（无时间标签）也应按块恢复多段
        let text = "第一段解说内容在这里。\n\n第二段解说内容在这里。\n\n第三段解说内容在这里。";
        let clips = recover_narration_segments(text, 60.0);
        assert_eq!(clips.len(), 3, "纯分段文本应恢复为多段");
        assert!(clips[1].text.contains("第二段"));
    }

    #[test]
    fn split_script_to_sections_distributes_unpunctuated_asr() {
        // 本地 ASR 仅返回整段无标点文本时，n 段都应分到内容，而非只第一段
        let asr = "这是一段没有任何断句标点的整段转写内容需要被均分到多个段落";
        let clips = split_script_to_sections(asr, 60.0, 3);
        assert_eq!(clips.len(), 3);
        assert!(!clips[0].text.is_empty());
        assert!(!clips[1].text.is_empty(), "无标点整段不应只填第一段");
        assert!(!clips[2].text.is_empty(), "无标点整段不应只填第一段");
    }
}
