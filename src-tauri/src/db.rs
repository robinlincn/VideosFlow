// VideosFlow — SQLite 持久层（sqlx）
// M0：12 张表首次启动建表（幂等）+ 双网关默认 Provider 种子 + Provider/Task CRUD。

use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use sqlx::Row;

/// 单个能力网关配置（对应前端 ProviderCfg，但密钥不在此表，存系统凭据库）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRow {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub provider: String,
    pub base_url: String,
    pub model: String,
    pub enabled: bool,
    /// 凭据库是否存在 key（运行时按凭据库补充，不落库）
    pub has_key: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStatus {
    pub id: String,
    pub status: String,
    pub progress: f64,
    pub log: String,
}

fn now_secs() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 首次启动：建 12 张表（幂等）+ 种子默认 Provider。
pub async fn init(pool: &SqlitePool) -> Result<(), String> {
    let stmts = [
        "CREATE TABLE IF NOT EXISTS film_categories(id TEXT PRIMARY KEY, name TEXT, \"order\" INT, editable INT DEFAULT 1)",
        "CREATE TABLE IF NOT EXISTS film_projects(id TEXT PRIMARY KEY, category_id TEXT, title TEXT, cover TEXT, status TEXT, tags TEXT, created_at INTEGER)",
        "CREATE TABLE IF NOT EXISTS edit_timelines(id TEXT PRIMARY KEY, project_id TEXT, tracks TEXT, clips TEXT, updated_at INTEGER)",
        "CREATE TABLE IF NOT EXISTS spoken_videos(id TEXT PRIMARY KEY, path TEXT, duration REAL, transcript TEXT, script TEXT, clean_script TEXT, created_at INTEGER)",
        "CREATE TABLE IF NOT EXISTS spoken_edits(id TEXT PRIMARY KEY, video_id TEXT, issue_type TEXT, start REAL, end REAL, text TEXT, suggestion TEXT, accepted INT DEFAULT 0)",
        "CREATE TABLE IF NOT EXISTS creation_projects(id TEXT PRIMARY KEY, brief TEXT, script TEXT, humanized_script TEXT, status TEXT, created_at INTEGER)",
        "CREATE TABLE IF NOT EXISTS storyboards(id TEXT PRIMARY KEY, project_id TEXT, shots TEXT, style_ref TEXT)",
        "CREATE TABLE IF NOT EXISTS generated_assets(id TEXT PRIMARY KEY, shot_id TEXT, kind TEXT, path TEXT, created_at INTEGER)",
        "CREATE TABLE IF NOT EXISTS voiceovers(id TEXT PRIMARY KEY, project_id TEXT, shot_id TEXT, voice_id TEXT, path TEXT)",
        "CREATE TABLE IF NOT EXISTS subtitles(id TEXT PRIMARY KEY, project_id TEXT, start REAL, end REAL, text TEXT, style_id TEXT)",
        "CREATE TABLE IF NOT EXISTS provider_config(id TEXT PRIMARY KEY, kind TEXT UNIQUE, name TEXT, provider TEXT, base_url TEXT, api_key TEXT, model TEXT, extra TEXT, enabled INT DEFAULT 1)",
        "CREATE TABLE IF NOT EXISTS tasks(id TEXT PRIMARY KEY, project_id TEXT, \"type\" TEXT, status TEXT, progress REAL, log TEXT, created_at INTEGER)",
    ];
    for s in stmts {
        sqlx::query(s)
            .execute(pool)
            .await
            .map_err(|e| format!("建表失败: {e}"))?;
    }
    seed_defaults(pool).await?;
    Ok(())
}

/// 首次运行填充双网关默认 Provider（Agnes: LLM/图像/视频/ASR；Mimo: TTS）。
async fn seed_defaults(pool: &SqlitePool) -> Result<(), String> {
    let cnt: i64 = sqlx::query("SELECT COUNT(*) AS c FROM provider_config")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?
        .try_get("c")
        .map_err(|e| e.to_string())?;
    if cnt > 0 {
        return Ok(());
    }
    // (kind, name, provider, base_url, model)
    let defaults: &[(&str, &str, &str, &str, &str)] = &[
        ("llm", "文字大模型", "agnes", "https://apihub.agnes-ai.com/v1", "agnes-2.0-flash"),
        ("img", "图片大模型", "agnes", "https://apihub.agnes-ai.com/v1", "agnes-image-2.1-flash"),
        ("video", "视频大模型", "agnes", "https://apihub.agnes-ai.com/v1", "agnes-video-v2.0"),
        ("asr", "语音识别", "agnes", "https://apihub.agnes-ai.com/v1", "agnes-asr-1.0"),
        ("tts", "语音合成", "mimo", "https://api.xiaomimimo.com/v1", "mimo-v2.5-tts"),
    ];
    for (kind, name, provider, base, model) in defaults {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO provider_config(id,kind,name,provider,base_url,api_key,model,enabled) VALUES(?,?,?,?,?,NULL,?,1)",
        )
        .bind(id)
        .bind(kind)
        .bind(name)
        .bind(provider)
        .bind(base)
        .bind(model)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn row_to_provider(r: &sqlx::sqlite::SqliteRow) -> Result<ProviderRow, String> {
    Ok(ProviderRow {
        id: r.try_get("id").map_err(|e| e.to_string())?,
        kind: r.try_get("kind").map_err(|e| e.to_string())?,
        name: r.try_get("name").map_err(|e| e.to_string())?,
        provider: r.try_get("provider").map_err(|e| e.to_string())?,
        base_url: r.try_get("base_url").map_err(|e| e.to_string())?,
        model: r.try_get("model").map_err(|e| e.to_string())?,
        enabled: r.try_get::<i64, _>("enabled").map_err(|e| e.to_string())? != 0,
        has_key: false, // 由调用方按凭据库补充
    })
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<ProviderRow>, String> {
    let rows = sqlx::query(
        "SELECT id,kind,name,provider,base_url,model,enabled FROM provider_config ORDER BY kind",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    rows.iter().map(row_to_provider).collect()
}

pub async fn get_by_kind(pool: &SqlitePool, kind: &str) -> Result<ProviderRow, String> {
    let r = sqlx::query(
        "SELECT id,kind,name,provider,base_url,model,enabled FROM provider_config WHERE kind=?",
    )
    .bind(kind)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?
    .ok_or_else(|| format!("未找到 Provider: {kind}"))?;
    row_to_provider(&r)
}

pub async fn upsert(
    pool: &SqlitePool,
    kind: &str,
    name: &str,
    provider: &str,
    base_url: &str,
    model: &str,
    enabled: bool,
) -> Result<(), String> {
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO provider_config(id,kind,name,provider,base_url,model,enabled) VALUES(?,?,?,?,?,?,?) \
         ON CONFLICT(kind) DO UPDATE SET name=excluded.name, provider=excluded.provider, base_url=excluded.base_url, model=excluded.model, enabled=excluded.enabled",
    )
    .bind(&id)
    .bind(kind)
    .bind(name)
    .bind(provider)
    .bind(base_url)
    .bind(model)
    .bind(if enabled { 1i64 } else { 0i64 })
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn task_create(
    pool: &SqlitePool,
    id: &str,
    kind: &str,
    project_id: Option<&str>,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO tasks(id,project_id,\"type\",status,progress,log,created_at) VALUES(?,?,?,'queued',0,'',?)",
    )
    .bind(id)
    .bind(project_id)
    .bind(kind)
    .bind(now_secs())
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn task_update(
    pool: &SqlitePool,
    id: &str,
    status: &str,
    progress: f64,
    log: &str,
) -> Result<(), String> {
    sqlx::query("UPDATE tasks SET status=?, progress=?, log=? WHERE id=?")
        .bind(status)
        .bind(progress)
        .bind(log)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn task_get(pool: &SqlitePool, id: &str) -> Result<TaskStatus, String> {
    let r = sqlx::query("SELECT id,status,progress,log FROM tasks WHERE id=?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("未找到任务: {id}"))?;
    Ok(TaskStatus {
        id: r.try_get("id").map_err(|e| e.to_string())?,
        status: r.try_get("status").map_err(|e| e.to_string())?,
        progress: r.try_get("progress").map_err(|e| e.to_string())?,
        log: r.try_get("log").map_err(|e| e.to_string())?,
    })
}
