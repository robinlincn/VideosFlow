// VideosFlow — SQLite 持久层（sqlx）
// M0：12 张表首次启动建表（幂等）+ 双网关默认 Provider 种子 + Provider/Task CRUD。
// M2：film_categories / film_projects / edit_timelines 全量 CRUD + 时间线领域模型。

use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqlitePool;
use sqlx::Row;
use std::collections::HashMap;

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

// ---------------------------------------------------------------------------
// M2：时间线领域模型（序列化为 edit_timelines.tracks / clips 的 JSON）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AsrSegment {
    pub start: f64,
    pub end: f64,
    pub text: String,
    #[serde(default)]
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScriptSeg {
    pub index: usize,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TimelineClip {
    pub id: String,
    #[serde(default)]
    pub source: String,
    pub timeline_start: f64,
    pub timeline_end: f64,
    pub src_start: f64,
    pub src_end: f64,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub flower: String,
    #[serde(default = "default_transition")]
    pub transition: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TimelineTrack {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_one_f")]
    pub volume: f64,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub clips: Vec<TimelineClip>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TimelineEnvelope {
    #[serde(default)]
    pub asr: Vec<AsrSegment>,
    #[serde(default)]
    pub script_segs: Vec<ScriptSeg>,
    #[serde(default)]
    pub alignment: HashMap<String, (f64, f64)>,
    #[serde(default)]
    pub tracks: Vec<TimelineTrack>,
    #[serde(default)]
    pub video_path: String,
}

fn default_transition() -> String {
    "none".into()
}
fn default_one_f() -> f64 {
    1.0
}

// ---------------------------------------------------------------------------
// M2：film 三表行结构
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilmCategoryRow {
    pub id: String,
    pub name: String,
    pub order: i64,
    pub editable: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilmProjectRow {
    pub id: String,
    pub category_id: String,
    pub title: String,
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub tags: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineRow {
    pub id: String,
    pub project_id: String,
    pub tracks: String,
    pub clips: String,
    pub updated_at: i64,
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
    seed_film_categories(pool).await?;
    Ok(())
}

/// 首次运行填充默认影片分类（与前端 initialFilmCats 对齐：电影/故事/电视剧/动画片/记录片）。
/// 仅当 film_categories 为空时插入，已存在则跳过（用户已自定义的分类不覆盖）。
async fn seed_film_categories(pool: &SqlitePool) -> Result<(), String> {
    let cnt: i64 = sqlx::query("SELECT COUNT(*) AS c FROM film_categories")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?
        .try_get("c")
        .map_err(|e| e.to_string())?;
    if cnt > 0 {
        return Ok(());
    }
    let defaults: &[(&str, &str, i64)] = &[
        ("c1", "电影", 1),
        ("c2", "故事", 2),
        ("c3", "电视剧", 3),
        ("c4", "动画片", 4),
        ("c5", "记录片", 5),
    ];
    for (id, name, order) in defaults {
        sqlx::query("INSERT INTO film_categories(id, name, \"order\", editable) VALUES(?, ?, ?, 1)")
            .bind(id)
            .bind(name)
            .bind(order)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
    }
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
        ("asr", "语音识别", "XiaomiMimo", "https://api.xiaomimimo.com/v1", "mimo-v2.5-asr"),
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

// ---------------------------------------------------------------------------
// M2：film_categories CRUD
// ---------------------------------------------------------------------------

pub async fn film_category_list(pool: &SqlitePool) -> Result<Vec<FilmCategoryRow>, String> {
    let rows = sqlx::query("SELECT id, name, \"order\", editable FROM film_categories ORDER BY \"order\"")
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(FilmCategoryRow {
            id: r.try_get("id").map_err(|e| e.to_string())?,
            name: r.try_get("name").map_err(|e| e.to_string())?,
            order: r.try_get("order").map_err(|e| e.to_string())?,
            editable: r.try_get("editable").map_err(|e| e.to_string())?,
        });
    }
    Ok(out)
}

pub async fn film_category_create(pool: &SqlitePool, name: &str, order: i64) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO film_categories(id, name, \"order\", editable) VALUES(?, ?, ?, 1)")
        .bind(&id)
        .bind(name)
        .bind(order)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(id)
}

pub async fn film_category_rename(pool: &SqlitePool, id: &str, name: &str) -> Result<(), String> {
    sqlx::query("UPDATE film_categories SET name=? WHERE id=?")
        .bind(name)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn film_category_reorder(pool: &SqlitePool, id: &str, order: i64) -> Result<(), String> {
    sqlx::query("UPDATE film_categories SET \"order\"=? WHERE id=?")
        .bind(order)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 删除：strategy = "merge" 归并到 target_id；"cascade" 级联删其工程+timeline。
pub async fn film_category_delete(
    pool: &SqlitePool,
    id: &str,
    strategy: &str,
    target_id: Option<&str>,
) -> Result<(), String> {
    if strategy == "merge" {
        if let Some(target) = target_id {
            sqlx::query("UPDATE film_projects SET category_id=? WHERE category_id=?")
                .bind(target)
                .bind(id)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
        }
        sqlx::query("DELETE FROM film_categories WHERE id=?")
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
    } else {
        // cascade：先删工程下的 timeine，再删工程，再删分类
        let pids = sqlx::query("SELECT id FROM film_projects WHERE category_id=?")
            .bind(id)
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?;
        for p in pids {
            let pid: String = p.try_get("id").map_err(|e| e.to_string())?;
            sqlx::query("DELETE FROM edit_timelines WHERE project_id=?")
                .bind(&pid)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
        }
        sqlx::query("DELETE FROM film_projects WHERE category_id=?")
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        sqlx::query("DELETE FROM film_categories WHERE id=?")
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// M2：film_projects CRUD
// ---------------------------------------------------------------------------

pub async fn film_project_list(pool: &SqlitePool, category_id: &str) -> Result<Vec<FilmProjectRow>, String> {
    let rows = sqlx::query(
        "SELECT id, category_id, title, cover, status, tags, created_at FROM film_projects WHERE category_id=? ORDER BY created_at DESC",
    )
    .bind(category_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(FilmProjectRow {
            id: r.try_get("id").map_err(|e| e.to_string())?,
            category_id: r.try_get("category_id").map_err(|e| e.to_string())?,
            title: r.try_get("title").map_err(|e| e.to_string())?,
            cover: r.try_get("cover").map_err(|e| e.to_string())?,
            status: r.try_get("status").map_err(|e| e.to_string()).unwrap_or_default(),
            tags: r.try_get("tags").map_err(|e| e.to_string()).unwrap_or_default(),
            created_at: r.try_get("created_at").map_err(|e| e.to_string())?,
        });
    }
    Ok(out)
}

pub async fn film_project_create(
    pool: &SqlitePool,
    category_id: &str,
    title: &str,
    cover: Option<&str>,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO film_projects(id, category_id, title, cover, status, tags, created_at) VALUES(?, ?, ?, ?, '草稿', '', ?)",
    )
    .bind(&id)
    .bind(category_id)
    .bind(title)
    .bind(cover)
    .bind(now_secs())
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(id)
}

pub async fn film_project_update(
    pool: &SqlitePool,
    id: &str,
    title: Option<&str>,
    cover: Option<&str>,
    status: Option<&str>,
    tags: Option<&str>,
) -> Result<(), String> {
    let mut sets: Vec<&str> = Vec::new();
    if title.is_some() {
        sets.push("title=?");
    }
    if cover.is_some() {
        sets.push("cover=?");
    }
    if status.is_some() {
        sets.push("status=?");
    }
    if tags.is_some() {
        sets.push("tags=?");
    }
    if sets.is_empty() {
        return Ok(());
    }
    let sql = format!("UPDATE film_projects SET {} WHERE id=?", sets.join(", "));
    let mut q = sqlx::query(&sql);
    if let Some(v) = title {
        q = q.bind(v);
    }
    if let Some(v) = cover {
        q = q.bind(v);
    }
    if let Some(v) = status {
        q = q.bind(v);
    }
    if let Some(v) = tags {
        q = q.bind(v);
    }
    q = q.bind(id);
    q.execute(pool).await.map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn film_project_delete(pool: &SqlitePool, id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM edit_timelines WHERE project_id=?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM film_projects WHERE id=?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// M2：edit_timelines CRUD
// ---------------------------------------------------------------------------

pub async fn timeline_get(pool: &SqlitePool, project_id: &str) -> Result<Option<TimelineRow>, String> {
    let r = sqlx::query("SELECT id, project_id, tracks, clips, updated_at FROM edit_timelines WHERE project_id=? LIMIT 1")
        .bind(project_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(r.map(|row| TimelineRow {
        id: row.try_get("id").map_err(|e| e.to_string()).unwrap_or_default(),
        project_id: row.try_get("project_id").map_err(|e| e.to_string()).unwrap_or_default(),
        tracks: row.try_get("tracks").map_err(|e| e.to_string()).unwrap_or_default(),
        clips: row.try_get("clips").map_err(|e| e.to_string()).unwrap_or_default(),
        updated_at: row.try_get("updated_at").map_err(|e| e.to_string()).unwrap_or_default(),
    }))
}

pub async fn timeline_save(
    pool: &SqlitePool,
    project_id: &str,
    tracks_json: &str,
    clips_json: &str,
) -> Result<String, String> {
    let existing: Option<String> = sqlx::query("SELECT id FROM edit_timelines WHERE project_id=?")
        .bind(project_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .and_then(|r| r.try_get::<String, _>("id").ok());
    match existing {
        Some(id) => {
            sqlx::query("UPDATE edit_timelines SET tracks=?, clips=?, updated_at=? WHERE id=?")
                .bind(tracks_json)
                .bind(clips_json)
                .bind(now_secs())
                .bind(&id)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
            Ok(id)
        }
        None => {
            let id = uuid::Uuid::new_v4().to_string();
            sqlx::query(
                "INSERT INTO edit_timelines(id, project_id, tracks, clips, updated_at) VALUES(?, ?, ?, ?, ?)",
            )
            .bind(&id)
            .bind(project_id)
            .bind(tracks_json)
            .bind(clips_json)
            .bind(now_secs())
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
            Ok(id)
        }
    }
}
