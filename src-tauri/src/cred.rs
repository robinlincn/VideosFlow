// VideosFlow — API Key 加密存储（AES-256-GCM）
//
// 安全策略：
// - 主密钥 32 字节从环境变量 `VF_MASTER_KEY` 读（生产必须用户设置；开发默认走固定 dev key）
// - Key 用 AES-256-GCM 加密后存 SQLite `provider_secrets`（key 永不落明文）
// - 启动时 `provider_config.has_key` 已落 SQLite 作为 UI 标记；
//   `provider_secrets` 是加密本体，删/改 `provider_secrets` 行 = 删/改 Key
//
// 注：M0-M2 设计初稿用 keyring（系统凭据库），但在本机 keyring 3.6 静默成功
//      但不写入 Windows Credential Manager（cmdkey /list 看不到），M3 切换到 AES-GCM 方案。

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};
use base64::Engine;
use rand::RngCore;
use sqlx::SqlitePool;

fn epoch_secs() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// 主密钥：32 字节。生产必须设 `VF_MASTER_KEY` 为 base64(32 字节)；
/// 开发兜底用固定 dev key（仅 debug 模式允许）。
fn master_key() -> [u8; 32] {
    let raw = std::env::var("VF_MASTER_KEY").unwrap_or_default();
    if raw.is_empty() {
        // 开发默认：固定 32 字节字符串（注意：不要在生产用）
        let mut k = [0u8; 32];
        let dev = b"videosflow-dev-master-key-do-not-use";
        let len = dev.len().min(32);
        k[..len].copy_from_slice(&dev[..len]);
        eprintln!("[videosflow] VF_MASTER_KEY 未设置，使用 dev 默认主密钥（生产请设置）");
        return k;
    }
    if raw.len() != 32 {
        eprintln!("[videosflow] VF_MASTER_KEY 长度不是 32 字节（{}），fallback dev key", raw.len());
        let mut k = [0u8; 32];
        let dev = b"videosflow-dev-master-key-do-not-use";
        let len = dev.len().min(32);
        k[..len].copy_from_slice(&dev[..len]);
        return k;
    }
    let mut k = [0u8; 32];
    k.copy_from_slice(raw.as_bytes());
    k
}

/// 加密：随机 12 字节 nonce + AES-GCM，输出格式 nonce || ciphertext (base64)
fn encrypt(key: &[u8; 32], plain: &str) -> Result<String, String> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(nonce, plain.as_bytes())
        .map_err(|e| format!("加密失败: {e}"))?;
    let mut out = Vec::with_capacity(12 + ct.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ct);
    Ok(base64::engine::general_purpose::STANDARD.encode(&out))
}

/// 解密：输入 base64(nonce || ciphertext)
fn decrypt(key: &[u8; 32], blob: &str) -> Result<String, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(blob)
        .map_err(|e| format!("base64 解码失败: {e}"))?;
    if bytes.len() < 12 {
        return Err("密文太短".into());
    }
    let (nonce_bytes, ct) = bytes.split_at(12);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let plain = cipher
        .decrypt(Nonce::from_slice(nonce_bytes), ct)
        .map_err(|e| format!("解密失败: {e}"))?;
    String::from_utf8(plain).map_err(|e| format!("UTF-8 解码失败: {e}"))
}

/// 写入/覆盖某能力网关的 API Key（加密存 SQLite）。
pub async fn set_key(pool: &SqlitePool, kind: &str, key: &str) -> Result<(), String> {
    let mk = master_key();
    let blob = encrypt(&mk, key)?;
    sqlx::query(
        "INSERT INTO provider_secrets(kind, ciphertext, updated_at) VALUES(?, ?, ?)
         ON CONFLICT(kind) DO UPDATE SET ciphertext=excluded.ciphertext, updated_at=excluded.updated_at",
    )
    .bind(kind)
    .bind(&blob)
    .bind(epoch_secs())
    .execute(pool)
    .await
    .map_err(|e| {
        eprintln!("[videosflow] cred::set_key({kind}) SQL FAILED: {e}");
        e.to_string()
    })?;
    eprintln!("[videosflow] cred::set_key({kind}) OK len={}", key.len());
    Ok(())
}

/// 读取某能力网关的 API Key（不存在返回 None）。
pub async fn get_key(pool: &SqlitePool, kind: &str) -> Result<Option<String>, String> {
    let mk = master_key();
    let r: Option<(String,)> = sqlx::query_as("SELECT ciphertext FROM provider_secrets WHERE kind=?")
        .bind(kind)
        .fetch_optional(pool)
        .await
        .map_err(|e| {
            eprintln!("[videosflow] cred::get_key({kind}) SQL FAILED: {e}");
            e.to_string()
        })?;
    match r {
        None => {
            eprintln!("[videosflow] cred::get_key({kind}) NoEntry");
            Ok(None)
        }
        Some((blob,)) => match decrypt(&mk, &blob) {
            Ok(s) => {
                eprintln!("[videosflow] cred::get_key({kind}) OK len={}", s.len());
                Ok(Some(s))
            }
            Err(e) => {
                eprintln!("[videosflow] cred::get_key({kind}) DECRYPT FAILED: {e}");
                Err(e)
            }
        },
    }
}

/// 删除某能力网关的 API Key。
pub async fn delete_key(pool: &SqlitePool, kind: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM provider_secrets WHERE kind=?")
        .bind(kind)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 凭据库是否存在该 key（仅供运行时探测；UI 提示以 SQLite `has_key` 列为权威）。
pub async fn has_key(pool: &SqlitePool, kind: &str) -> bool {
    get_key(pool, kind).await.ok().flatten().is_some()
}