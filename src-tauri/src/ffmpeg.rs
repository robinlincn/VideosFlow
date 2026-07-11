// VideosFlow — FFmpeg 封装 + 首启下载器
// M0：定位（PATH / 本地缓存）→ 缺失时按 VF_FFMPEG_URL 首启下载并解包（不随包内置）。
// 媒体命令构造方法供 P1-P5 复用（抽音轨/拼接/混音/烧字幕/导出）。

use std::path::PathBuf;
use std::process::Command;

pub struct FfMpeg {
    pub path: PathBuf,
}

fn bin_name() -> &'static str {
    if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    }
}

impl FfMpeg {
    /// 定位 ffmpeg：先查 PATH，再查本地缓存 data_dir/ffmpeg/bin。
    pub fn locate(data_dir: &std::path::Path) -> Result<FfMpeg, String> {
        if Command::new(bin_name())
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return Ok(FfMpeg {
                path: PathBuf::from(bin_name()),
            });
        }
        let local = data_dir.join("ffmpeg").join("bin").join(bin_name());
        if local.exists() {
            return Ok(FfMpeg { path: local });
        }
        Err("FFmpeg 未找到，请放置到 PATH 或 data_dir/ffmpeg/bin，或配置首启下载".into())
    }

    /// 确保可用：本地没有则触发首启下载，再定位。
    pub async fn ensure(data_dir: &std::path::Path) -> Result<FfMpeg, String> {
        if let Ok(f) = Self::locate(data_dir) {
            return Ok(f);
        }
        download_first_launch(data_dir).await?;
        Self::locate(data_dir)
    }

    pub fn version(&self) -> Result<String, String> {
        let out = Command::new(&self.path)
            .arg("-version")
            .output()
            .map_err(|e| e.to_string())?;
        let s = String::from_utf8_lossy(&out.stdout);
        Ok(s.lines().next().unwrap_or("").to_string())
    }

    // ---- 媒体命令构造（供 P1-P5 复用）----

    pub fn extract_audio_cmd(&self, input: &str, output: &str) -> Command {
        let mut c = Command::new(&self.path);
        c.args(["-i", input, "-vn", "-ac", "1", "-ar", "16000", output]);
        c
    }

    pub fn concat_cmd(&self, list: &str, output: &str) -> Command {
        let mut c = Command::new(&self.path);
        c.args(["-f", "concat", "-safe", "0", "-i", list, "-c", "copy", output]);
        c
    }

    pub fn mux_cmd(&self, video: &str, audio: &str, output: &str) -> Command {
        let mut c = Command::new(&self.path);
        c.args([
            "-i", video, "-i", audio, "-c:v", "copy", "-c:a", "aac", "-shortest", output,
        ]);
        c
    }

    pub fn burn_ass_cmd(&self, input: &str, ass: &str, output: &str) -> Command {
        let mut c = Command::new(&self.path);
        c.args(["-i", input, "-vf", &format!("ass={ass}"), output]);
        c
    }

    pub fn export_cmd(&self, input: &str, output: &str, hw: bool) -> Command {
        let mut c = Command::new(&self.path);
        if hw {
            c.args(["-i", input, "-c:v", "h264_nvenc", "-c:a", "aac", output]);
        } else {
            c.args([
                "-i", input, "-c:v", "libx264", "-crf", "20", "-c:a", "aac", output,
            ]);
        }
        c
    }
}

/// 首启下载器：URL 由环境变量 VF_FFMPEG_URL 提供（建议使用国内可达镜像）。
/// 未配置时返回可读错误，不阻塞应用启动。
async fn download_first_launch(data_dir: &std::path::Path) -> Result<(), String> {
    let url = match std::env::var("VF_FFMPEG_URL") {
        Ok(u) if !u.is_empty() => u,
        _ => {
            return Err(
                "未配置 FFmpeg 下载源（环境变量 VF_FFMPEG_URL）。\n请手动将 ffmpeg 放到 data_dir/ffmpeg/bin，或设置 VF_FFMPEG_URL 后重试首启下载。".into(),
            );
        }
    };
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("下载 FFmpeg 失败: {e}"))?;
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    let tmp = data_dir.join("ffmpeg_download.tmp");
    std::fs::write(&tmp, &bytes).map_err(|e| e.to_string())?;
    let dest = data_dir.join("ffmpeg");
    std::fs::create_dir_all(&dest).ok();
    // 用系统 tar 解包（Windows 10+、Linux、macOS 均自带 tar）
    let status = Command::new("tar")
        .args([
            "-xf",
            tmp.to_str().unwrap_or(""),
            "-C",
            dest.to_str().unwrap_or(""),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit())
        .status();
    let _ = std::fs::remove_file(&tmp);
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("解包 FFmpeg 失败，tar 退出码: {s}")),
        Err(e) => Err(format!("解包 FFmpeg 失败（缺少 tar）: {e}")),
    }
}
