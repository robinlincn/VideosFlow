// VideosFlow — Python sidecar 启动/守护/健康检查 + HTTP 信封
// M0 仅做 best-effort 启动与连通性，崩溃不阻塞主程序（独立进程，崩了可重启）。
// M2：新增 call_asr / call_tts 转发 /v1/asr、/v1/tts。

use std::path::Path;
use std::process::{Child, Command};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::db;

pub const DEFAULT_PORT: u16 = 8731;

/// 与前端/sidecar 一致的 Provider 配置（camelCase，便于直接序列化转发给 FastAPI）。
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCfg {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub model: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub test: String,
}

fn default_true() -> bool {
    true
}

/// 与 sidecar 统一信封对齐。
#[derive(Deserialize)]
pub struct Envelope {
    pub ok: bool,
    #[serde(default)]
    pub code: i64,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
}

/// sidecar 进程守护句柄（child 持有所有权，drop 时随主程序退出）。
pub struct SidecarGuard {
    pub child: Mutex<Option<Child>>,
    pub port: u16,
}

fn find_python(resource_dir: &Path) -> Option<String> {
    // 1) 显式覆盖（优先，便于 CI/特殊环境指定解释器）
    if let Ok(p) = std::env::var("VF_PYTHON") {
        if !p.is_empty() {
            return Some(p);
        }
    }
    // 2) sidecar 随附 venv（优先，避免污染系统 Python）
    let venv_rel = if cfg!(windows) {
        ".venv/Scripts/python.exe"
    } else {
        ".venv/bin/python"
    };
    let sc_dirs = [
        resource_dir.join("python-sidecar"),
        resource_dir.join("..").join("python-sidecar"),
        resource_dir.join("..").join("..").join("python-sidecar"),
    ];
    let mut candidates: Vec<String> = Vec::new();
    for d in sc_dirs.iter() {
        candidates.push(d.join(venv_rel).to_string_lossy().to_string());
    }
    // 3) 回退到 PATH 中的解释器
    candidates.extend(["python".to_string(), "python3".to_string(), "py".to_string()]);
    for interp in candidates {
        if interp.is_empty() {
            continue;
        }
        if Command::new(&interp)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return Some(interp);
        }
    }
    None
}

fn find_sidecar_main(resource_dir: &Path) -> Option<std::path::PathBuf> {
    if let Ok(d) = std::env::var("VF_SIDECAR_DIR") {
        let p = Path::new(&d).join("main.py");
        if p.exists() {
            return Some(p);
        }
    }
    let candidates = [
        resource_dir.join("python-sidecar").join("main.py"),
        resource_dir.join("..").join("python-sidecar").join("main.py"),
        resource_dir.join("..").join("..").join("python-sidecar").join("main.py"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

/// 启动 sidecar（best-effort）。找不到 Python 或 main.py 时仅告警并返回空守护。
pub fn spawn_sidecar(resource_dir: &Path, port: u16) -> SidecarGuard {
    let guard = SidecarGuard {
        child: Mutex::new(None),
        port,
    };
    let interp = match find_python(resource_dir) {
        Some(i) => i,
        None => {
            eprintln!("[videosflow] 未找到 Python 解释器，sidecar 未启动（不影响应用启动）");
            return guard;
        }
    };
    let main = match find_sidecar_main(resource_dir) {
        Some(m) => m,
        None => {
            eprintln!("[videosflow] 未找到 python-sidecar/main.py，sidecar 未启动");
            return guard;
        }
    };
    match Command::new(interp)
        .arg(&main)
        .env("SIDECAR_PORT", port.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(child) => {
            *guard.child.lock().unwrap() = Some(child);
            eprintln!("[videosflow] Python sidecar 已启动: {}", main.display());
        }
        Err(e) => eprintln!("[videosflow] 启动 sidecar 失败: {e}"),
    }
    guard
}

/// 健康检查：GET /health。
pub async fn health(client: &reqwest::Client, port: u16) -> bool {
    client
        .get(format!("http://127.0.0.1:{port}/health"))
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

async fn post_envelope(
    client: &reqwest::Client,
    port: u16,
    path: &str,
    body: &serde_json::Value,
) -> Result<Envelope, String> {
    let resp = client
        .post(format!("http://127.0.0.1:{port}{path}"))
        .json(body)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("请求 sidecar 失败: {e}"))?;
    let env: Envelope = resp
        .json()
        .await
        .map_err(|e| format!("解析响应失败: {e}"))?;
    Ok(env)
}

/// 连接测试：POST /v1/test { cfg }。
pub async fn call_test(
    client: &reqwest::Client,
    port: u16,
    cfg: &ProviderCfg,
) -> Result<Envelope, String> {
    let body = serde_json::json!({ "cfg": cfg });
    post_envelope(client, port, "/v1/test", &body).await
}

/// 通用能力调用：POST /v1/{endpoint} { cfg, req }。
pub async fn call_endpoint(
    client: &reqwest::Client,
    port: u16,
    endpoint: &str,
    cfg: &ProviderCfg,
    req: serde_json::Value,
) -> Result<Envelope, String> {
    let body = serde_json::json!({ "cfg": cfg, "req": req });
    post_envelope(client, port, &format!("/v1/{endpoint}"), &body).await
}

/// 真实对话：POST /v1/chat { cfg, req:{prompt, max_tokens} }。
pub async fn call_chat(
    client: &reqwest::Client,
    port: u16,
    cfg: &ProviderCfg,
    prompt: &str,
    max_tokens: u32,
) -> Result<Envelope, String> {
    let req = serde_json::json!({ "prompt": prompt, "max_tokens": max_tokens });
    call_endpoint(client, port, "chat", cfg, req).await
}

/// M2：语音识别：POST /v1/asr { cfg, req:{audioPath, language} }。
pub async fn call_asr(
    client: &reqwest::Client,
    port: u16,
    cfg: &ProviderCfg,
    audio_path: &str,
    language: &str,
) -> Result<Envelope, String> {
    let req = serde_json::json!({ "audioPath": audio_path, "language": language });
    call_endpoint(client, port, "asr", cfg, req).await
}

/// M2：语音合成：POST /v1/tts { cfg, req:{text, voice} }。返回 data.audioPath。
pub async fn call_tts(
    client: &reqwest::Client,
    port: u16,
    cfg: &ProviderCfg,
    text: &str,
    voice: &str,
) -> Result<Envelope, String> {
    let req = serde_json::json!({ "text": text, "voice": voice });
    call_endpoint(client, port, "tts", cfg, req).await
}

/// 由 DB 行 + 凭据库 key 组装完整 ProviderCfg（供连接测试转发）。
pub fn build_cfg(row: &db::ProviderRow, api_key: Option<String>) -> ProviderCfg {
    ProviderCfg {
        name: row.name.clone(),
        provider: row.provider.clone(),
        base_url: row.base_url.clone(),
        api_key: api_key.unwrap_or_default(),
        model: row.model.clone(),
        enabled: row.enabled,
        test: "idle".to_string(),
    }
}
