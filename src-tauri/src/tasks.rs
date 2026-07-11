// VideosFlow — 任务队列 + 进度广播（tokio mpsc + Tauri Channel）
// M0：worker 消费任务，做 sidecar 健康检查并广播进度，最终持久化状态到 tasks 表。
// 真实能力（ASR/脚本/图/视频/TTS）在 M1-M5 接入 sidecar 各端点。

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tokio::sync::mpsc;

use reqwest::Client;
use sqlx::sqlite::SqlitePool;

use crate::{cred, db, python};

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
pub fn start(pool: SqlitePool, client: Client, port: u16, rx: mpsc::Receiver<TaskJob>) {
    tauri::async_runtime::spawn(async move {
        run_loop(pool, client, port, rx).await;
    });
}

async fn run_loop(pool: SqlitePool, client: Client, port: u16, mut rx: mpsc::Receiver<TaskJob>) {
    while let Some(job) = rx.recv().await {
        run_job(&pool, &client, port, job).await;
    }
}

async fn run_job(pool: &SqlitePool, client: &Client, port: u16, job: TaskJob) {
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

    // M1：真实 Agnes /v1/chat 最小调用（打通 Rust→sidecar→Agnes 全链路）
    if job.kind == "chat" || job.kind == "llm_chat" {
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
async fn run_chat(
    pool: &SqlitePool,
    client: &Client,
    port: u16,
    job: &TaskJob,
) -> Result<String, String> {
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
