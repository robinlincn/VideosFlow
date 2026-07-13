// VideosFlow — FFmpeg 封装 + 首启下载器
// M0：定位（PATH / 本地缓存）→ 缺失时按 VF_FFMPEG_URL 首启下载并解包（不随包内置）。
// M2：新增 silence_cmd / segment_cmd / build_ass，export_cmd 补分辨率参数。

use std::path::PathBuf;
use std::process::Command;

use crate::db;

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

    #[allow(dead_code)] // 公共 API 备查：探测 ffmpeg 版本
    pub fn version(&self) -> Result<String, String> {
        let out = Command::new(&self.path)
            .arg("-version")
            .output()
            .map_err(|e| e.to_string())?;
        let s = String::from_utf8_lossy(&out.stdout);
        Ok(s.lines().next().unwrap_or("").to_string())
    }

    // ---- 媒体命令构造（M0/M2 复用）----

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

    /// 精确切点：-ss S -to E -i in -c copy。
    pub fn segment_cmd(&self, input: &str, start: f64, end: f64, output: &str) -> Command {
        let mut c = Command::new(&self.path);
        c.args([
            "-ss",
            &format!("{start:.3}"),
            "-to",
            &format!("{end:.3}"),
            "-i",
            input,
            "-c",
            "copy",
            output,
        ]);
        c
    }

    /// 静音检测：silencedetect 输出到 stderr，由 detect_silence 解析。
    pub fn silence_cmd(&self, input: &str, noise_db: &str, duration: f32) -> Command {
        let mut c = Command::new(&self.path);
        c.args([
            "-i",
            input,
            "-af",
            &format!("silencedetect=noise={noise_db}dB:d={duration}"),
            "-f",
            "null",
            "-",
        ]);
        c
    }

    /// 运行静音检测并返回静音段 [(start, end)]。
    pub fn detect_silence(&self, input: &str, noise_db: &str, duration: f32) -> Result<Vec<(f64, f64)>, String> {
        let out = self
            .silence_cmd(input, noise_db, duration)
            .output()
            .map_err(|e| e.to_string())?;
        let stderr = String::from_utf8_lossy(&out.stderr);
        Ok(parse_silence(&stderr))
    }

    pub fn export_cmd(&self, input: &str, output: &str, hw: bool, resolution: &str) -> Command {
        let mut c = Command::new(&self.path);
        c.args(["-i", input]);
        if !resolution.is_empty() {
            c.args(["-vf", &format!("scale={}", resolution.replace('x', ":"))]);
        }
        if hw {
            c.args(["-c:v", "h264_nvenc", "-c:a", "aac", output]);
        } else {
            c.args(["-c:v", "libx264", "-crf", "20", "-c:a", "aac", output]);
        }
        c
    }
}

/// 解析 ffmpeg silencedetect 输出，返回静音段 [(start, end)]。
pub fn parse_silence(s: &str) -> Vec<(f64, f64)> {
    let mut starts: Vec<f64> = Vec::new();
    let mut spans: Vec<(f64, f64)> = Vec::new();
    for line in s.lines() {
        if let Some(rest) = line.split("silence_start:").nth(1) {
            if let Ok(v) = rest.trim().split_whitespace().next().unwrap_or("").parse::<f64>() {
                starts.push(v);
            }
        } else if let Some(rest) = line.split("silence_end:").nth(1) {
            let end = rest
                .trim()
                .split_whitespace()
                .next()
                .and_then(|x| x.parse::<f64>().ok());
            if let (Some(e), Some(st)) = (end, starts.pop()) {
                spans.push((st, e));
            }
        }
    }
    spans
}

// ---------------------------------------------------------------------------
// M2：花字 6 套 ASS 样式模板（固化内置，不支持用户自定义）
// ---------------------------------------------------------------------------

pub struct AssStyle {
    pub name: &'static str,
    pub font_name: &'static str,
    pub font_size: i32,
    pub primary_colour: &'static str,
    pub back_colour: &'static str,
    pub outline: i32,
    pub shadow: i32,
    pub bold: i32,
    pub border_style: i32,
    pub alignment: i32,
    pub margin_v: i32,
    pub margin_l: i32,
}

pub fn flower_style(id: &str) -> &'static AssStyle {
    for s in FLOWER_STYLES {
        if s.name.eq_ignore_ascii_case(id) {
            return s;
        }
    }
    &FLOWER_STYLES[0]
}

pub const FLOWER_STYLES: &[AssStyle] = &[
    // emphasis 重点强调：黄底加粗
    AssStyle {
        name: "Emphasis",
        font_name: "Noto Sans CJK SC",
        font_size: 30,
        primary_colour: "&H00FFFFFF",
        back_colour: "&H0042C8F5",
        outline: 0,
        shadow: 0,
        bold: 1,
        border_style: 3,
        alignment: 2,
        margin_v: 40,
        margin_l: 30,
    },
    // emotion 情绪渲染：粉紫渐变（以亮紫描边近似）
    AssStyle {
        name: "Emotion",
        font_name: "Noto Sans CJK SC",
        font_size: 30,
        primary_colour: "&H00B0A0FF",
        back_colour: "&H00000000",
        outline: 3,
        shadow: 1,
        bold: 0,
        border_style: 1,
        alignment: 2,
        margin_v: 40,
        margin_l: 30,
    },
    // shout 强烈感叹：红字大字
    AssStyle {
        name: "Shout",
        font_name: "Noto Sans CJK SC",
        font_size: 40,
        primary_colour: "&H003838F0",
        back_colour: "&H00000000",
        outline: 3,
        shadow: 2,
        bold: 1,
        border_style: 1,
        alignment: 2,
        margin_v: 60,
        margin_l: 30,
    },
    // keyword 关键词描边：白底边框
    AssStyle {
        name: "Keyword",
        font_name: "Noto Sans CJK SC",
        font_size: 30,
        primary_colour: "&H00FFFFFF",
        back_colour: "&H00000000",
        outline: 3,
        shadow: 0,
        bold: 0,
        border_style: 1,
        alignment: 2,
        margin_v: 40,
        margin_l: 30,
    },
    // title 居中标题：居中大字
    AssStyle {
        name: "Title",
        font_name: "Noto Sans CJK SC",
        font_size: 38,
        primary_colour: "&H00FFFFFF",
        back_colour: "&H00000000",
        outline: 2,
        shadow: 1,
        bold: 1,
        border_style: 1,
        alignment: 5,
        margin_v: 80,
        margin_l: 0,
    },
    // signature 左下角署名：小字左下
    AssStyle {
        name: "Signature",
        font_name: "Noto Sans CJK SC",
        font_size: 20,
        primary_colour: "&H00D0D0D0",
        back_colour: "&H00000000",
        outline: 1,
        shadow: 0,
        bold: 0,
        border_style: 1,
        alignment: 1,
        margin_v: 20,
        margin_l: 20,
    },
];

/// 把秒转为 ASS 时间格式 H:MM:SS.cc。
pub fn ass_time(sec: f64) -> String {
    let total = (sec * 100.0).round() as i64;
    let cs = total % 100;
    let total_s = total / 100;
    let s = total_s % 60;
    let m = (total_s / 60) % 60;
    let h = total_s / 3600;
    format!("{h}:{m:02}:{s:02}.{cs:02}")
}

/// 由 subtitle/gen 轨生成 ASS 文本（窗口 [win_start, win_end] 内、时间轴对齐）。
pub fn build_ass(tracks: &[db::TimelineTrack], win_start: f64, win_end: f64) -> String {
    let mut styles = String::from(
        "Style: Default,Noto Sans CJK SC,28,&H00FFFFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,28,28,28,0",
    );
    for s in FLOWER_STYLES {
        styles.push_str(&format!(
            "\nStyle: {},{},{},{},&H00000000,{},{},{},0,0,0,100,100,0,0,{},{},{},{},{},20,{},1",
            s.name,
            s.font_name,
            s.font_size,
            s.primary_colour,
            s.back_colour,
            s.back_colour,
            s.bold,
            s.border_style,
            s.outline,
            s.shadow,
            s.alignment,
            s.margin_l,
            s.margin_v
        ));
    }

    let mut events = String::new();
    for tr in tracks {
        if tr.kind != "subtitle" && tr.kind != "gen" {
            continue;
        }
        for c in &tr.clips {
            let s = (c.timeline_start.max(win_start) - win_start).max(0.0);
            let e = c.timeline_end.min(win_end) - win_start;
            if e <= s || e <= 0.0 {
                continue;
            }
            let style = if c.flower.is_empty() {
                "Default"
            } else {
                flower_style(&c.flower).name
            };
            let text = c
                .text
                .replace('\\', "\\\\")
                .replace(',', "\\,")
                .replace('\n', "\\N");
            events.push_str(&format!(
                "\nDialogue: 0,{},{},{},,0,0,0,,{}\n",
                ass_time(s),
                ass_time(e),
                style,
                text
            ));
        }
    }

    format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n{}\n\n[Events]\nFormat: Layer, Start, End, Style, Text\n{}",
        styles, events
    )
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
