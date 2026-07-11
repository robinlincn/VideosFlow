// VideosFlow — 任务队列 + 进度广播（tokio mpsc + Tauri Channel）
// M0：worker 消费任务，做 sidecar 健康检查并广播进度，最终持久化状态到 tasks 表。
// 真实能力（ASR/脚本/图/视频/TTS）在 M1-M5 接入 sidecar 各端点。

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tokio::sync::mpsc;

use reqwest::Client;
use sqlx::sqlite::SqlitePool;

use crate::{db, python};

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

    // sidecar 连通性（M0 链路验证）
    let alive = python::health(client, port).await;
    if !alive {
        db::task_update(
            pool,
            &job.id,
            "failed",
            30.0,
            "Python sidecar 未运行（M0 仅验证链路；请先启动 python-sidecar）",
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

    // 真实能力在 M1-M5 接入；M0 完成链路验证
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
