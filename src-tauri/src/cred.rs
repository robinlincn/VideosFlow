// VideosFlow — API Key 凭据库封装（keyring）
// 红线：API Key 只存系统凭据库（Windows Credential Manager / macOS Keychain / Linux libsecret），
// 绝不落 SQLite 明文、绝不进 git。

use keyring::Entry;

const SERVICE: &str = "videosflow";

fn entry(kind: &str) -> Result<Entry, String> {
    Entry::new(SERVICE, &format!("provider:{kind}")).map_err(|e| e.to_string())
}

/// 写入/覆盖某能力网关的 API Key。
pub fn set_key(kind: &str, key: &str) -> Result<(), String> {
    entry(kind)?.set_password(key).map_err(|e| e.to_string())
}

/// 读取某能力网关的 API Key（不存在返回 None）。
pub fn get_key(kind: &str) -> Result<Option<String>, String> {
    match entry(kind)?.get_password() {
        Ok(k) => Ok(Some(k)),
        Err(e) if matches!(e, keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// 删除某能力网关的 API Key。
pub fn delete_key(kind: &str) -> Result<(), String> {
    let _ = entry(kind)?.delete_credential();
    Ok(())
}

/// 凭据库是否存在该 key。
pub fn has_key(kind: &str) -> bool {
    get_key(kind).ok().flatten().is_some()
}
