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
        // 关键：-map 0:v:0 -map 1:a:0 强制「视频取自输入0、配音取自输入1(wav)」。
        // 若不指定，ffmpeg 默认从第一个输入（视频文件本身含原声）取音轨，
        // 导致 TTS 配音被整体丢弃、成片保留原声。
        c.args([
            "-i", video, "-i", audio,
            "-map", "0:v:0", "-map", "1:a:0",
            "-c:v", "copy", "-c:a", "aac", "-shortest", output,
        ]);
        c
    }

    pub fn burn_ass_cmd(&self, input: &str, ass: &str, output: &str) -> Command {
        let mut c = Command::new(&self.path);
        // Windows 路径含盘符 C: 与反斜杠 \，ffmpeg 的 ass/subtitles 滤镜会把 ':' 当作协议
        // 分隔符、把 '\' 当作转义符，导致字幕烧录静默失败（最终输出无字幕）。统一转正斜杠
        // 并对盘符冒号转义，再用 subtitles=filename='...' 形式包裹，跨平台稳健。
        let normalized = ass.replace('\\', "/");
        let escaped = normalized.replace(":", "\\:");
        let filter = format!("subtitles=filename='{}'", escaped);
        // 显式只取视频+音频流，丢弃源视频自带的字幕流（避免旧字幕轨道被带入成片），
        // 视频因 -vf 必须重编码。0:a? 表示音频可选（无音轨也不报错）。
        c.args([
            "-i", input,
            "-vf", &filter,
            "-map", "0:v:0", "-map", "0:a?",
            "-c:v", "libx264", "-preset", "veryfast", "-crf", "20",
            "-c:a", "aac",
            output,
        ]);
        c
    }

    /// 烧录解说词字幕（drawtext 滤镜版）。比 ASS 更可靠：不依赖 libass、无 ASS CSV 格式陷阱、
    /// 任何分辨率/编码都能稳定渲染。text_file 为 UTF-8 文本文件（直接写 s.text 即可，无需转义）。
    /// 半透明黑底框确保任何背景上都清晰可读。
    pub fn burn_subtitle_cmd(&self, input: &str, text_file: &str, output: &str) -> Command {
        let mut c = Command::new(&self.path);
        // 字体文件：Windows 自带 Microsoft YaHei（msyh.ttc）。textfile 路径做冒号转义。
        let font = "C\\:/Windows/Fonts/msyh.ttc";
        let normalized = text_file.replace('\\', "/");
        let escaped_text = normalized.replace(":", "\\:");
        let filter = format!(
            "drawtext=fontfile='{}':textfile='{}':fontcolor=white:fontsize=48:box=1:boxcolor=black@0.5:boxborderw=10:x=(w-text_w)/2:y=h-text_h-30",
            font, escaped_text
        );
        c.args([
            "-i", input,
            "-vf", &filter,
            "-c:v", "libx264", "-preset", "veryfast", "-crf", "20",
            "-c:a", "aac",
            output,
        ]);
        c
    }

    /// 探测音频/视频时长（秒）。优先 ffprobe，失败回退解析 WAV 头（TTS 返回的多为 PCM wav）。
    pub fn probe_duration(&self, path: &str) -> Option<f64> {
        let probe = self.path.to_string_lossy().replace("ffmpeg", "ffprobe");
        if let Ok(o) = Command::new(&probe)
            .args(["-v", "error", "-show_entries", "format=duration", "-of", "default=nw=1:nk=1", path])
            .output()
        {
            if let Ok(s) = String::from_utf8_lossy(&o.stdout).trim().parse::<f64>() {
                if s > 0.0 { return Some(s); }
            }
        }
        parse_wav_duration(path)
    }

    /// 将音频归一化到目标时长（target_dur 秒），使配音与视频切片严格等长、消除累积漂移。
    /// - 配音偏短：静音补齐到 target_dur
    /// - 配音偏长：加速（atempo 链）到 target_dur，保留全部台词
    /// - 时长已接近：直接拷贝
    pub fn normalize_audio_to(&self, input: &str, output: &str, src_dur: f64, target_dur: f64) -> Command {
        let mut c = Command::new(&self.path);
        c.args(["-i", input]);
        if target_dur <= 0.0 || (target_dur - src_dur).abs() < 0.05 {
            c.args(["-c:a", "copy", output]);
            return c;
        }
        let filter = if target_dur > src_dur {
            // 静音补齐到目标时长（apad 延展 + atrim 精确截断）
            format!("apad,atrim=0:{:.3}", target_dur)
        } else {
            // 加速到目标时长保留全部台词（atempo 链支持任意倍率），再精确截断
            format!("{},atrim=0:{:.3}", Self::atempo_filter(target_dur / src_dur), target_dur)
        };
        c.args([
            "-af", &filter,
            "-c:a", "pcm_s16le", "-ar", "24000", "-ac", "1",
            output,
        ]);
        c
    }

    /// 生成 atempo 滤镜链：ffmpeg atempo 接受 (0.5, 2.0]，超出范围需多级串联。
    fn atempo_filter(rate: f64) -> String {
        let mut filters: Vec<String> = Vec::new();
        let mut r = rate;
        while r > 2.0 { filters.push("atempo=2.0".to_string()); r /= 2.0; }
        while r < 0.5 { filters.push("atempo=0.5".to_string()); r /= 0.5; }
        filters.push(format!("atempo={:.4}", r));
        filters.join(",")
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

    /// 由单张首帧图生成「运镜视频片段」（Ken Burns 缓慢推近）。离线可用，无需视频大模型。
    /// 输出固定 1920x1080 / 30fps / yuv420p，时长 dur 秒，便于后续 concat copy。
    pub fn gen_clip_single_cmd(&self, image: &str, dur: f64, output: &str) -> Command {
        let mut c = Command::new(&self.path);
        let d = format!("{:.3}", dur.max(1.0));
        c.args(["-loop", "1", "-i", image]);
        c.args([
            "-vf",
            "scale=1920:1080:force_original_aspect_ratio=increase,crop=1920:1080,zoompan=z='min(zoom+0.0015,1.25)':d=1:x='iw/2-(iw/zoom/2)':y='ih/2-(ih/zoom/2)':fps=30:s=1920x1080",
        ]);
        c.args([
            "-c:v", "libx264", "-pix_fmt", "yuv420p", "-r", "30", "-t", &d, output,
        ]);
        c
    }

    /// 由首帧 + 尾帧两张图生成「首尾帧视频」，中间以淡入淡出（xfade）过渡。离线可用。
    pub fn gen_clip_xfade_cmd(&self, a: &str, b: &str, dur: f64, output: &str) -> Command {
        let d = dur.max(2.0);
        let x = (d / 2.0).min(1.0); // 过渡时长（秒）
        let half = (d + x) / 2.0; // 每段时长，使 xfade 后总时长恰为 d
        let offset = half - x; // 过渡起始点
        let mut c = Command::new(&self.path);
        c.args(["-loop", "1", "-i", a, "-loop", "1", "-i", b]);
        c.args([
            "-filter_complex",
            &format!(
                "[0:v]scale=1920:1080:force_original_aspect_ratio=increase,crop=1920:1080,trim=duration={half:.3},setpts=PTS-STARTPTS[f0];\
                 [1:v]scale=1920:1080:force_original_aspect_ratio=increase,crop=1920:1080,trim=duration={half:.3},setpts=PTS-STARTPTS[f1];\
                 [f0][f1]xfade=transition=fade:duration={x:.3}:offset={offset:.3}[fv]"
            ),
        ]);
        c.args([
            "-map", "[fv]", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-r", "30", "-t",
            &format!("{d:.3}"), output,
        ]);
        c
    }

    /// 合成单个分镜成片片段：视频（来自 frames 片段）+ 可选配音（wav）+ 烧录该镜台词字幕（drawtext）。
    /// 统一 1920x1080 / 30fps / yuv420p，方便最终 concat copy 拼接。sub_text_file 为台词 UTF-8 文本。
    pub fn compose_seg_cmd(&self, video: &str, audio: Option<&str>, sub_text_file: &str, output: &str) -> Command {
        let font = "C\\:/Windows/Fonts/msyh.ttc";
        let sub = sub_text_file.replace('\\', "/").replace(":", "\\:");
        let filter = format!(
            "scale=1920:1080,drawtext=fontfile='{}':textfile='{}':fontcolor=white:fontsize=48:box=1:boxcolor=black@0.5:boxborderw=10:x=(w-text_w)/2:y=h-text_h-30",
            font, sub
        );
        let mut c = Command::new(&self.path);
        c.args(["-i", video]);
        if let Some(a) = audio {
            c.args(["-i", a]);
        }
        c.args([
            "-vf", &filter, "-c:v", "libx264", "-crf", "20", "-pix_fmt", "yuv420p", "-r", "30",
        ]);
        if audio.is_some() {
            c.args(["-map", "0:v:0", "-map", "1:a:0", "-c:a", "aac", "-ar", "44100", "-shortest"]);
        } else {
            c.args(["-an"]);
        }
        c.arg(output);
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

/// 解析 PCM WAV 文件头得到时长（秒）。TTS 返回的 wav 多为 PCM，无需外部依赖即可估算。
fn parse_wav_duration(path: &str) -> Option<f64> {
    let data = std::fs::read(path).ok()?;
    if data.len() < 44 { return None; }
    if &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" { return None; }
    let num_channels = u16::from_le_bytes([data[22], data[23]]) as f64;
    let sample_rate = u32::from_le_bytes([data[24], data[25], data[26], data[27]]) as f64;
    let bits = u16::from_le_bytes([data[34], data[35]]) as f64;
    // 定位 data 块大小
    let mut i = 12usize;
    let mut data_size = 0u32;
    while i + 8 <= data.len() {
        let ck = &data[i..i + 4];
        let sz = u32::from_le_bytes([data[i + 4], data[i + 5], data[i + 6], data[i + 7]]);
        if ck == b"data" {
            data_size = sz;
            break;
        }
        i += 8 + sz as usize;
    }
    if sample_rate <= 0.0 || num_channels <= 0.0 || bits <= 0.0 { return None; }
    let bytes_per_sec = sample_rate * num_channels * (bits / 8.0);
    if bytes_per_sec <= 0.0 { return None; }
    Some(data_size as f64 / bytes_per_sec)
}
