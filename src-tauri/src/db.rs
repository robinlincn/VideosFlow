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
    /// 运行模式：'cloud'（云端 API，需 Base URL/Model/Key）或 'local'（本地推理，如 faster-whisper/Whisper）
    pub mode: String,
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

// ---------------------------------------------------------------------------
// M3：spoken_videos / spoken_edits 行结构
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpokenVideoRow {
    pub id: String,
    pub name: String,
    pub path: String,
    pub duration: f64,
    /// JSON 序列化的 AsrSegment[]（与影片共享 AsrSegment 结构）
    #[serde(default)]
    pub transcript: String,
    /// 提取的纯文案（按标点切 + 去填充词）
    #[serde(default)]
    pub script: String,
    /// 干净文案（采纳所有 accepted=1 edits 后生成）
    #[serde(default)]
    pub clean_script: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpokenEditRow {
    pub id: String,
    pub video_id: String,
    /// gap | mistake | repeat
    pub issue_type: String,
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub suggestion: String,
    /// 0 待定 / 1 采纳 / -1 忽略
    pub accepted: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpokenAssetRow {
    pub id: String,
    pub video_id: String,
    pub name: String,
    /// image | bgm | sfx | clip
    pub kind: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpokenKeywordRow {
    pub id: String,
    pub video_id: String,
    pub text: String,
    pub weight: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpokenMatchRow {
    pub id: String,
    pub video_id: String,
    pub seg_start: f64,
    pub seg_end: f64,
    pub seg_text: String,
    pub keyword: String,
    pub asset_id: String,
    pub applied: i64,
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
        "CREATE TABLE IF NOT EXISTS film_projects(id TEXT PRIMARY KEY, category_id TEXT, title TEXT, cover TEXT, status TEXT, tags TEXT, script TEXT, analysis TEXT, created_at INTEGER)",
        "CREATE TABLE IF NOT EXISTS edit_timelines(id TEXT PRIMARY KEY, project_id TEXT, tracks TEXT, clips TEXT, updated_at INTEGER)",
        "CREATE TABLE IF NOT EXISTS spoken_videos(id TEXT PRIMARY KEY, path TEXT, duration REAL, transcript TEXT, script TEXT, clean_script TEXT, created_at INTEGER)",
        "CREATE TABLE IF NOT EXISTS spoken_edits(id TEXT PRIMARY KEY, video_id TEXT, issue_type TEXT, start REAL, end REAL, text TEXT, suggestion TEXT, accepted INT DEFAULT 0)",
        "CREATE TABLE IF NOT EXISTS spoken_assets(id TEXT PRIMARY KEY, video_id TEXT, name TEXT, kind TEXT, path TEXT)",
        "CREATE TABLE IF NOT EXISTS spoken_keywords(id TEXT PRIMARY KEY, video_id TEXT, text TEXT, weight REAL DEFAULT 1.0)",
        "CREATE TABLE IF NOT EXISTS spoken_matches(id TEXT PRIMARY KEY, video_id TEXT, seg_start REAL, seg_end REAL, seg_text TEXT, keyword TEXT, asset_id TEXT, applied INT DEFAULT 0)",
        "CREATE TABLE IF NOT EXISTS creation_projects(id TEXT PRIMARY KEY, brief TEXT, script TEXT, humanized_script TEXT, status TEXT, created_at INTEGER)",
        "CREATE TABLE IF NOT EXISTS storyboards(id TEXT PRIMARY KEY, project_id TEXT, shots TEXT, style_ref TEXT, updated_at INTEGER)",
        "CREATE TABLE IF NOT EXISTS generated_assets(id TEXT PRIMARY KEY, project_id TEXT, shot_id TEXT, kind TEXT, path TEXT, created_at INTEGER)",
        "CREATE TABLE IF NOT EXISTS voiceovers(id TEXT PRIMARY KEY, project_id TEXT, shot_id TEXT, voice_id TEXT, path TEXT)",
        "CREATE TABLE IF NOT EXISTS subtitles(id TEXT PRIMARY KEY, project_id TEXT, start REAL, end REAL, text TEXT, style_id TEXT)",
        "CREATE TABLE IF NOT EXISTS provider_config(id TEXT PRIMARY KEY, kind TEXT UNIQUE, name TEXT, provider TEXT, base_url TEXT, api_key TEXT, model TEXT, extra TEXT, enabled INT DEFAULT 1, has_key INTEGER DEFAULT 0, mode TEXT NOT NULL DEFAULT 'cloud')",
        "CREATE TABLE IF NOT EXISTS provider_secrets(kind TEXT PRIMARY KEY, ciphertext TEXT NOT NULL, updated_at INTEGER)",
        "CREATE TABLE IF NOT EXISTS tasks(id TEXT PRIMARY KEY, project_id TEXT, \"type\" TEXT, status TEXT, progress REAL, log TEXT, created_at INTEGER)",
    ];
    for s in stmts {
        sqlx::query(s)
            .execute(pool)
            .await
            .map_err(|e| format!("建表失败: {e}"))?;
    }
    // 兼容旧库：film_projects 早期无 script / analysis 列，按需补列（全新库建表已含，PRAGMA 探测到会跳过）
    let _ = sqlx::query("ALTER TABLE film_projects ADD COLUMN script TEXT")
        .execute(pool)
        .await;
    let _ = sqlx::query("ALTER TABLE film_projects ADD COLUMN analysis TEXT")
        .execute(pool)
        .await;
    // 兼容旧库：storyboards 早期无 updated_at 列（storyboard_save 会写该列），按需补列。
    let _ = sqlx::query("ALTER TABLE storyboards ADD COLUMN updated_at INTEGER")
        .execute(pool)
        .await;
    // 兼容旧库：generated_assets 早期无 project_id 列（generated_assets_list / insert / delete 均写该列），按需补列。
    let _ = sqlx::query("ALTER TABLE generated_assets ADD COLUMN project_id TEXT")
        .execute(pool)
        .await;
    seed_defaults(pool).await?;
    seed_film_categories(pool).await?;
    ensure_provider_has_key_col(pool).await?;
    ensure_provider_mode_col(pool).await?;
    Ok(())
}

/// 兼容已存在的旧 DB：provider_config 早期没有 has_key 列，这里按需 ALTER 补列。
/// 全新 DB 因建表语句已含该列，PRAGMA 探测到后会跳过。
async fn ensure_provider_has_key_col(pool: &SqlitePool) -> Result<(), String> {
    let cols = sqlx::query("PRAGMA table_info(provider_config)")
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
    let has = cols.iter().any(|r| {
        r.try_get::<String, _>("name")
            .map(|n| n == "has_key")
            .unwrap_or(false)
    });
    if !has {
        sqlx::query("ALTER TABLE provider_config ADD COLUMN has_key INTEGER DEFAULT 0")
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 兼容已存在的旧 DB：provider_config 早期没有 mode 列，这里按需 ALTER 补列（默认 'cloud'）。
async fn ensure_provider_mode_col(pool: &SqlitePool) -> Result<(), String> {
    let cols = sqlx::query("PRAGMA table_info(provider_config)")
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
    let has = cols.iter().any(|r| {
        r.try_get::<String, _>("name")
            .map(|n| n == "mode")
            .unwrap_or(false)
    });
    if !has {
        sqlx::query("ALTER TABLE provider_config ADD COLUMN mode TEXT NOT NULL DEFAULT 'cloud'")
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
    }
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
        // has_key 以 DB 布尔列为权威标记（写入密钥成功后由 provider_key_set 置位），
        // 仅在 DB 为 false 时由调用方回退凭据库探测，避免被凭据库回读不稳拖累 UI。
        has_key: r.try_get::<i64, _>("has_key").map(|v| v != 0).unwrap_or(false),
        // mode 缺省回退 'cloud'（兼容旧库无该列时的读取）
        mode: r.try_get::<String, _>("mode")
            .map(|m| if m.is_empty() { "cloud".into() } else { m })
            .unwrap_or_else(|_| "cloud".into()),
    })
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<ProviderRow>, String> {
    let rows = sqlx::query(
        "SELECT id,kind,name,provider,base_url,model,enabled,has_key,mode FROM provider_config ORDER BY kind",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    rows.iter().map(row_to_provider).collect()
}

pub async fn get_by_kind(pool: &SqlitePool, kind: &str) -> Result<ProviderRow, String> {
    let r = sqlx::query(
        "SELECT id,kind,name,provider,base_url,model,enabled,has_key,mode FROM provider_config WHERE kind=?",
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
    mode: &str,
) -> Result<(), String> {
    let id = uuid::Uuid::new_v4().to_string();
    let mode = if mode.is_empty() { "cloud" } else { mode };
    sqlx::query(
        "INSERT INTO provider_config(id,kind,name,provider,base_url,model,enabled,mode) VALUES(?,?,?,?,?,?,?,?) \
         ON CONFLICT(kind) DO UPDATE SET name=excluded.name, provider=excluded.provider, base_url=excluded.base_url, model=excluded.model, enabled=excluded.enabled, mode=excluded.mode",
    )
    .bind(&id)
    .bind(kind)
    .bind(name)
    .bind(provider)
    .bind(base_url)
    .bind(model)
        .bind(if enabled { 1i64 } else { 0i64 })
        .bind(mode)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// 写入某网关「是否已存密钥」的布尔标记（注意：仅存标记，密钥本体只在系统凭据库）。
/// 这是 UI 显示「已保存 KEY」提示的权威来源，写入成功后由 provider_key_set 置 true。
pub async fn set_has_key(pool: &SqlitePool, kind: &str, v: bool) -> Result<(), String> {
    sqlx::query("UPDATE provider_config SET has_key=? WHERE kind=?")
        .bind(if v { 1i64 } else { 0i64 })
        .bind(kind)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
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

/// 写入影片解说文案（film_script_gen 工作流的结果落库点）。
pub async fn film_project_set_script(pool: &SqlitePool, id: &str, script: &str) -> Result<(), String> {
    sqlx::query("UPDATE film_projects SET script=? WHERE id=?")
        .bind(script)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 写入影片视频分析结果（film_video_analysis 工作流的结果落库点，存 Markdown 报告）。
pub async fn film_project_set_analysis(pool: &SqlitePool, id: &str, analysis: &str) -> Result<(), String> {
    sqlx::query("UPDATE film_projects SET analysis=? WHERE id=?")
        .bind(analysis)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 读取影片视频分析结果（Markdown 报告），未分析返回 None。
pub async fn film_project_get_analysis(pool: &SqlitePool, id: &str) -> Result<Option<String>, String> {
    let row: Option<(Option<String>,)> = sqlx::query_as("SELECT analysis FROM film_projects WHERE id=?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(row.and_then(|r| r.0).filter(|s| !s.trim().is_empty()))
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

// ---------------------------------------------------------------------------
// M3：spoken_videos / spoken_edits / spoken_assets / spoken_keywords / spoken_matches
// ---------------------------------------------------------------------------

fn spoken_video_row(r: &sqlx::sqlite::SqliteRow) -> Result<SpokenVideoRow, String> {
    Ok(SpokenVideoRow {
        id: r.try_get("id").map_err(|e| e.to_string())?,
        name: r.try_get("name").map_err(|e| e.to_string())?,
        path: r.try_get("path").map_err(|e| e.to_string())?,
        duration: r.try_get::<f64, _>("duration").map_err(|e| e.to_string()).unwrap_or(0.0),
        transcript: r.try_get("transcript").map_err(|e| e.to_string()).unwrap_or_default(),
        script: r.try_get("script").map_err(|e| e.to_string()).unwrap_or_default(),
        clean_script: r.try_get("clean_script").map_err(|e| e.to_string()).unwrap_or_default(),
        created_at: r.try_get("created_at").map_err(|e| e.to_string()).unwrap_or(0),
    })
}

pub async fn spoken_video_list(pool: &SqlitePool) -> Result<Vec<SpokenVideoRow>, String> {
    let rows = sqlx::query(
        "SELECT id, name, path, duration, transcript, script, clean_script, created_at FROM spoken_videos ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    rows.iter().map(spoken_video_row).collect()
}

pub async fn spoken_video_get(pool: &SqlitePool, id: &str) -> Result<SpokenVideoRow, String> {
    let r = sqlx::query(
        "SELECT id, name, path, duration, transcript, script, clean_script, created_at FROM spoken_videos WHERE id=?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?
    .ok_or_else(|| format!("未找到口播视频: {id}"))?;
    spoken_video_row(&r)
}

pub async fn spoken_video_create(
    pool: &SqlitePool,
    name: &str,
    path: &str,
    duration: f64,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO spoken_videos(id, name, path, duration, transcript, script, clean_script, created_at) VALUES(?, ?, ?, ?, '', '', '', ?)",
    )
    .bind(&id)
    .bind(name)
    .bind(path)
    .bind(duration)
    .bind(now_secs())
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(id)
}

pub async fn spoken_video_delete(pool: &SqlitePool, id: &str) -> Result<(), String> {
    // 级联：edits / assets / keywords / matches
    sqlx::query("DELETE FROM spoken_edits WHERE video_id=?").bind(id).execute(pool).await.map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM spoken_assets WHERE video_id=?").bind(id).execute(pool).await.map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM spoken_keywords WHERE video_id=?").bind(id).execute(pool).await.map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM spoken_matches WHERE video_id=?").bind(id).execute(pool).await.map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM spoken_videos WHERE id=?").bind(id).execute(pool).await.map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn spoken_video_set_transcript(pool: &SqlitePool, id: &str, transcript_json: &str) -> Result<(), String> {
    sqlx::query("UPDATE spoken_videos SET transcript=? WHERE id=?")
        .bind(transcript_json)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn spoken_video_set_script(pool: &SqlitePool, id: &str, script: &str) -> Result<(), String> {
    sqlx::query("UPDATE spoken_videos SET script=? WHERE id=?")
        .bind(script)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn spoken_video_set_clean(pool: &SqlitePool, id: &str, clean_script: &str) -> Result<(), String> {
    sqlx::query("UPDATE spoken_videos SET clean_script=? WHERE id=?")
        .bind(clean_script)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ---- spoken_edits ----

fn spoken_edit_row(r: &sqlx::sqlite::SqliteRow) -> Result<SpokenEditRow, String> {
    Ok(SpokenEditRow {
        id: r.try_get("id").map_err(|e| e.to_string())?,
        video_id: r.try_get("video_id").map_err(|e| e.to_string())?,
        issue_type: r.try_get("issue_type").map_err(|e| e.to_string())?,
        start: r.try_get::<f64, _>("start").map_err(|e| e.to_string()).unwrap_or(0.0),
        end: r.try_get::<f64, _>("end").map_err(|e| e.to_string()).unwrap_or(0.0),
        text: r.try_get("text").map_err(|e| e.to_string()).unwrap_or_default(),
        suggestion: r.try_get("suggestion").map_err(|e| e.to_string()).unwrap_or_default(),
        accepted: r.try_get::<i64, _>("accepted").map_err(|e| e.to_string()).unwrap_or(0),
    })
}

pub async fn spoken_edits_list(pool: &SqlitePool, video_id: &str) -> Result<Vec<SpokenEditRow>, String> {
    let rows = sqlx::query(
        "SELECT id, video_id, issue_type, start, end, text, suggestion, accepted FROM spoken_edits WHERE video_id=? ORDER BY start, id",
    )
    .bind(video_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    rows.iter().map(spoken_edit_row).collect()
}

/// 整体替换某视频的所有 edits（用于检测任务结果写入）。
/// edits_json = JSON.stringify(Partial<SpokenEditRow>[])，不带 id；服务端生成 id。
pub async fn spoken_edits_replace(pool: &SqlitePool, video_id: &str, edits_json: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM spoken_edits WHERE video_id=?").bind(video_id).execute(pool).await.map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(edits_json).map_err(|e| e.to_string())?;
    let arr = v.as_array().cloned().unwrap_or_default();
    for item in arr {
        let issue_type = item.get("issueType").and_then(|x| x.as_str()).unwrap_or("").to_string();
        if issue_type.is_empty() { continue; }
        let start = item.get("start").and_then(|x| x.as_f64()).unwrap_or(0.0);
        let end = item.get("end").and_then(|x| x.as_f64()).unwrap_or(0.0);
        let text = item.get("text").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let suggestion = item.get("suggestion").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let accepted = item.get("accepted").and_then(|x| x.as_i64()).unwrap_or(0);
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO spoken_edits(id, video_id, issue_type, start, end, text, suggestion, accepted) VALUES(?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(&id).bind(video_id).bind(&issue_type).bind(start).bind(end).bind(&text).bind(&suggestion).bind(accepted)
            .execute(pool).await.map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub async fn spoken_edits_set_accepted(pool: &SqlitePool, id: &str, accepted: i64) -> Result<(), String> {
    sqlx::query("UPDATE spoken_edits SET accepted=? WHERE id=?")
        .bind(accepted)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ---- spoken_assets ----

fn spoken_asset_row(r: &sqlx::sqlite::SqliteRow) -> Result<SpokenAssetRow, String> {
    Ok(SpokenAssetRow {
        id: r.try_get("id").map_err(|e| e.to_string())?,
        video_id: r.try_get("video_id").map_err(|e| e.to_string())?,
        name: r.try_get("name").map_err(|e| e.to_string())?,
        kind: r.try_get("kind").map_err(|e| e.to_string())?,
        path: r.try_get("path").map_err(|e| e.to_string())?,
    })
}

pub async fn spoken_assets_list(pool: &SqlitePool, video_id: &str) -> Result<Vec<SpokenAssetRow>, String> {
    let rows = sqlx::query("SELECT id, video_id, name, kind, path FROM spoken_assets WHERE video_id=? ORDER BY id")
        .bind(video_id)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
    rows.iter().map(spoken_asset_row).collect()
}

pub async fn spoken_asset_create(pool: &SqlitePool, video_id: &str, name: &str, kind: &str, path: &str) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO spoken_assets(id, video_id, name, kind, path) VALUES(?, ?, ?, ?, ?)")
        .bind(&id).bind(video_id).bind(name).bind(kind).bind(path)
        .execute(pool).await.map_err(|e| e.to_string())?;
    Ok(id)
}

pub async fn spoken_asset_delete(pool: &SqlitePool, id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM spoken_assets WHERE id=?").bind(id).execute(pool).await.map_err(|e| e.to_string())?;
    Ok(())
}

// ---- spoken_keywords ----

fn spoken_keyword_row(r: &sqlx::sqlite::SqliteRow) -> Result<SpokenKeywordRow, String> {
    Ok(SpokenKeywordRow {
        id: r.try_get("id").map_err(|e| e.to_string())?,
        video_id: r.try_get("video_id").map_err(|e| e.to_string())?,
        text: r.try_get("text").map_err(|e| e.to_string())?,
        weight: r.try_get::<f64, _>("weight").map_err(|e| e.to_string()).unwrap_or(0.0),
    })
}

pub async fn spoken_keywords_list(pool: &SqlitePool, video_id: &str) -> Result<Vec<SpokenKeywordRow>, String> {
    let rows = sqlx::query("SELECT id, video_id, text, weight FROM spoken_keywords WHERE video_id=? ORDER BY weight DESC")
        .bind(video_id)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
    rows.iter().map(spoken_keyword_row).collect()
}

pub async fn spoken_keywords_replace(pool: &SqlitePool, video_id: &str, kws_json: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM spoken_keywords WHERE video_id=?").bind(video_id).execute(pool).await.map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(kws_json).map_err(|e| e.to_string())?;
    let arr = v.as_array().cloned().unwrap_or_default();
    for item in arr {
        let text = item.get("text").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
        if text.is_empty() { continue; }
        let weight = item.get("weight").and_then(|x| x.as_f64()).unwrap_or(1.0);
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO spoken_keywords(id, video_id, text, weight) VALUES(?, ?, ?, ?)")
            .bind(&id).bind(video_id).bind(&text).bind(weight)
            .execute(pool).await.map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ---- spoken_matches ----

fn spoken_match_row(r: &sqlx::sqlite::SqliteRow) -> Result<SpokenMatchRow, String> {
    Ok(SpokenMatchRow {
        id: r.try_get("id").map_err(|e| e.to_string())?,
        video_id: r.try_get("video_id").map_err(|e| e.to_string())?,
        seg_start: r.try_get::<f64, _>("seg_start").map_err(|e| e.to_string()).unwrap_or(0.0),
        seg_end: r.try_get::<f64, _>("seg_end").map_err(|e| e.to_string()).unwrap_or(0.0),
        seg_text: r.try_get("seg_text").map_err(|e| e.to_string()).unwrap_or_default(),
        keyword: r.try_get("keyword").map_err(|e| e.to_string()).unwrap_or_default(),
        asset_id: r.try_get("asset_id").map_err(|e| e.to_string()).unwrap_or_default(),
        applied: r.try_get::<i64, _>("applied").map_err(|e| e.to_string()).unwrap_or(0),
    })
}

pub async fn spoken_matches_list(pool: &SqlitePool, video_id: &str) -> Result<Vec<SpokenMatchRow>, String> {
    let rows = sqlx::query("SELECT id, video_id, seg_start, seg_end, seg_text, keyword, asset_id, applied FROM spoken_matches WHERE video_id=? ORDER BY seg_start, id")
        .bind(video_id)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
    rows.iter().map(spoken_match_row).collect()
}

pub async fn spoken_match_replace(pool: &SqlitePool, video_id: &str, matches_json: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM spoken_matches WHERE video_id=?").bind(video_id).execute(pool).await.map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(matches_json).map_err(|e| e.to_string())?;
    let arr = v.as_array().cloned().unwrap_or_default();
    for item in arr {
        let seg_text = item.get("segText").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let keyword = item.get("keyword").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let asset_id = item.get("assetId").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let seg_start = item.get("segStart").and_then(|x| x.as_f64()).unwrap_or(0.0);
        let seg_end = item.get("segEnd").and_then(|x| x.as_f64()).unwrap_or(0.0);
        let applied = item.get("applied").and_then(|x| x.as_i64()).unwrap_or(0);
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO spoken_matches(id, video_id, seg_start, seg_end, seg_text, keyword, asset_id, applied) VALUES(?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(&id).bind(video_id).bind(seg_start).bind(seg_end).bind(&seg_text).bind(&keyword).bind(&asset_id).bind(applied)
            .execute(pool).await.map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub async fn spoken_match_toggle(pool: &SqlitePool, id: &str) -> Result<(), String> {
    // 在 Rust 端翻 applied（0<->1）；SQLite 单语句
    sqlx::query("UPDATE spoken_matches SET applied = 1 - applied WHERE id=?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// M4：creation_projects / storyboards / generated_assets 行结构
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreationProjectRow {
    pub id: String,
    #[serde(default)]
    pub brief: String,
    #[serde(default)]
    pub script: String,
    #[serde(default)]
    pub humanized_script: String,
    #[serde(default)]
    pub status: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoryboardRow {
    pub id: String,
    pub project_id: String,
    /// JSON.stringify(Shot[])
    pub shots: String,
    #[serde(default)]
    pub style_ref: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedAssetRow {
    pub id: String,
    pub project_id: String,
    pub shot_id: i64,
    /// image
    pub kind: String,
    pub path: String,
    pub created_at: i64,
}

// ---- creation_projects CRUD ----

fn creation_project_row(r: &sqlx::sqlite::SqliteRow) -> Result<CreationProjectRow, String> {
    Ok(CreationProjectRow {
        id: r.try_get("id").map_err(|e| e.to_string())?,
        brief: r.try_get("brief").map_err(|e| e.to_string()).unwrap_or_default(),
        script: r.try_get("script").map_err(|e| e.to_string()).unwrap_or_default(),
        humanized_script: r.try_get("humanized_script").map_err(|e| e.to_string()).unwrap_or_default(),
        status: r.try_get("status").map_err(|e| e.to_string()).unwrap_or_default(),
        created_at: r.try_get("created_at").map_err(|e| e.to_string()).unwrap_or(0),
    })
}

pub async fn creation_project_list(pool: &SqlitePool) -> Result<Vec<CreationProjectRow>, String> {
    let rows = sqlx::query("SELECT id, brief, script, humanized_script, status, created_at FROM creation_projects ORDER BY created_at DESC")
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
    rows.iter().map(creation_project_row).collect()
}

pub async fn creation_project_get(pool: &SqlitePool, id: &str) -> Result<CreationProjectRow, String> {
    let r = sqlx::query("SELECT id, brief, script, humanized_script, status, created_at FROM creation_projects WHERE id=?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("未找到创作工程: {id}"))?;
    creation_project_row(&r)
}

pub async fn creation_project_create(pool: &SqlitePool, brief: &str) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO creation_projects(id, brief, script, humanized_script, status, created_at) VALUES(?, ?, '', '', 'draft', ?)")
        .bind(&id).bind(brief).bind(now_secs())
        .execute(pool).await.map_err(|e| e.to_string())?;
    Ok(id)
}

pub async fn creation_project_update(pool: &SqlitePool, id: &str, brief: Option<&str>, script: Option<&str>, humanized_script: Option<&str>, status: Option<&str>) -> Result<(), String> {
    let mut sets: Vec<&str> = Vec::new();
    if brief.is_some() { sets.push("brief=?"); }
    if script.is_some() { sets.push("script=?"); }
    if humanized_script.is_some() { sets.push("humanized_script=?"); }
    if status.is_some() { sets.push("status=?"); }
    if sets.is_empty() { return Ok(()); }
    let sql = format!("UPDATE creation_projects SET {} WHERE id=?", sets.join(", "));
    let mut q = sqlx::query(&sql);
    if let Some(v) = brief { q = q.bind(v); }
    if let Some(v) = script { q = q.bind(v); }
    if let Some(v) = humanized_script { q = q.bind(v); }
    if let Some(v) = status { q = q.bind(v); }
    q = q.bind(id);
    q.execute(pool).await.map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn creation_project_delete(pool: &SqlitePool, id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM storyboards WHERE project_id=?").bind(id).execute(pool).await.map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM generated_assets WHERE project_id=?").bind(id).execute(pool).await.map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM creation_projects WHERE id=?").bind(id).execute(pool).await.map_err(|e| e.to_string())?;
    Ok(())
}

// ---- storyboards CRUD ----

pub async fn storyboard_get(pool: &SqlitePool, project_id: &str) -> Result<Option<StoryboardRow>, String> {
    let r = sqlx::query("SELECT id, project_id, shots, style_ref, updated_at FROM storyboards WHERE project_id=?")
        .bind(project_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(r.map(|row| StoryboardRow {
        id: row.try_get("id").map_err(|e| e.to_string()).unwrap_or_default(),
        project_id: row.try_get("project_id").map_err(|e| e.to_string()).unwrap_or_default(),
        shots: row.try_get("shots").map_err(|e| e.to_string()).unwrap_or_default(),
        style_ref: row.try_get("style_ref").map_err(|e| e.to_string()).unwrap_or_default(),
        updated_at: row.try_get("updated_at").map_err(|e| e.to_string()).unwrap_or(0),
    }))
}

pub async fn storyboard_save(pool: &SqlitePool, project_id: &str, shots_json: &str, style_ref: &str) -> Result<String, String> {
    let existing: Option<String> = sqlx::query("SELECT id FROM storyboards WHERE project_id=?")
        .bind(project_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .and_then(|r| r.try_get::<String, _>("id").ok());
    match existing {
        Some(id) => {
            sqlx::query("UPDATE storyboards SET shots=?, style_ref=?, updated_at=? WHERE id=?")
                .bind(shots_json).bind(style_ref).bind(now_secs()).bind(&id)
                .execute(pool).await.map_err(|e| e.to_string())?;
            Ok(id)
        }
        None => {
            let id = uuid::Uuid::new_v4().to_string();
            sqlx::query("INSERT INTO storyboards(id, project_id, shots, style_ref, updated_at) VALUES(?, ?, ?, ?, ?)")
                .bind(&id).bind(project_id).bind(shots_json).bind(style_ref).bind(now_secs())
                .execute(pool).await.map_err(|e| e.to_string())?;
            Ok(id)
        }
    }
}

// ---- generated_assets CRUD ----

fn generated_asset_row(r: &sqlx::sqlite::SqliteRow) -> Result<GeneratedAssetRow, String> {
    Ok(GeneratedAssetRow {
        id: r.try_get("id").map_err(|e| e.to_string())?,
        project_id: r.try_get("project_id").map_err(|e| e.to_string())?,
        shot_id: r.try_get::<i64, _>("shot_id").map_err(|e| e.to_string()).unwrap_or(0),
        kind: r.try_get("kind").map_err(|e| e.to_string()).unwrap_or_default(),
        path: r.try_get("path").map_err(|e| e.to_string()).unwrap_or_default(),
        created_at: r.try_get("created_at").map_err(|e| e.to_string()).unwrap_or(0),
    })
}

pub async fn generated_assets_list(pool: &SqlitePool, project_id: &str) -> Result<Vec<GeneratedAssetRow>, String> {
    let rows = sqlx::query("SELECT id, project_id, shot_id, kind, path, created_at FROM generated_assets WHERE project_id=? ORDER BY shot_id, id")
        .bind(project_id)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
    rows.iter().map(generated_asset_row).collect()
}

pub async fn generated_asset_insert(pool: &SqlitePool, project_id: &str, shot_id: i64, kind: &str, path: &str) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO generated_assets(id, project_id, shot_id, kind, path, created_at) VALUES(?, ?, ?, ?, ?, ?)")
        .bind(&id).bind(project_id).bind(shot_id).bind(kind).bind(path).bind(now_secs())
        .execute(pool).await.map_err(|e| e.to_string())?;
    Ok(id)
}
