// 本地视频预览文件服务器（零依赖，仅用 std::net + std::thread）。
// 绑定 127.0.0.1（仅本进程可访问），支持 HTTP Range 请求，使 Tauri WebView 的
// <video> 元素能流式播放用户选择的本地视频，并支持任意位置 seek。
//
// 设计要点：
// - 仅响应 GET /file?path=<urlencoded 绝对路径>，且扩展名必须是视频类型（白名单）。
// - 支持 `Range: bytes=START-END`，返回 206 + Content-Range，使拖拽进度条可定位。
// - 不复制文件、不进内存，直接流式转发磁盘字节。

use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;

const VIDEO_EXTS: &[&str] = &["mp4", "mov", "mkv", "avi", "webm", "m4v", "flv", "wmv", "m2ts", "ts"];

/// 启动本地文件服务器，返回监听端口（失败返回 0）。
pub fn start() -> u16 {
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[fileserver] 绑定 127.0.0.1:0 失败: {e}");
            return 0;
        }
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    if port == 0 {
        return 0;
    }
    eprintln!("[fileserver] 本地视频预览服务器已启动: http://127.0.0.1:{}", port);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            if let Ok(s) = stream {
                std::thread::spawn(move || handle_client(s));
            }
        }
    });
    port
}

fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                if let (Some(h), Some(l)) = (hi, lo) {
                    out.push((h * 16 + l) as u8);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn content_type(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        "avi" => "video/x-msvideo",
        "webm" => "video/webm",
        "flv" => "video/x-flv",
        "wmv" => "video/x-ms-wmv",
        "m2ts" | "ts" => "video/mp2t",
        _ => "application/octet-stream",
    }
}

fn handle_client(mut stream: TcpStream) {
    let clone = match stream.try_clone() {
        Ok(c) => c,
        Err(_) => return,
    };
    let mut reader = BufReader::new(clone);
    let mut req = Vec::new();
    let mut buf = [0u8; 2048];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => return,
            Ok(n) => {
                req.extend_from_slice(&buf[..n]);
                if req.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
                if req.len() > 64 * 1024 {
                    break;
                }
            }
            Err(_) => return,
        }
    }
    let req_str = String::from_utf8_lossy(&req);
    let first_line = req_str.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 || parts[0] != "GET" {
        write_status(&mut stream, 400, "Bad Request", &[]);
        return;
    }
    let url = parts[1];
    let query = url.split('?').nth(1).unwrap_or("");
    let path_param = query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        if k == "path" {
            Some(v)
        } else {
            None
        }
    });
    let Some(encoded) = path_param else {
        write_status(&mut stream, 400, "Missing path", &[]);
        return;
    };
    let decoded = url_decode(encoded);
    let path = Path::new(&decoded);
    // 安全：仅允许本地绝对路径的视频文件
    if !path.is_absolute() || !path.is_file() {
        write_status(&mut stream, 403, "Forbidden", &[]);
        return;
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    if !VIDEO_EXTS.contains(&ext.as_str()) {
        write_status(&mut stream, 415, "Unsupported Media Type", &[]);
        return;
    }
    let ctype = content_type(path);
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => {
            write_status(&mut stream, 404, "Not Found", &[]);
            return;
        }
    };
    let size = meta.len();
    let range = req_str.lines().find_map(|l| {
        if l.to_lowercase().starts_with("range:") {
            Some(l.splitn(2, ':').nth(1).unwrap_or("").trim().to_string())
        } else {
            None
        }
    });
    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => {
            write_status(&mut stream, 500, "Internal Error", &[]);
            return;
        }
    };
    match range {
        Some(r) if r.to_lowercase().starts_with("bytes=") => {
            let spec = &r["bytes=".len()..];
            let (start, mut end) = parse_range(spec, size);
            if end >= size {
                end = size.saturating_sub(1);
            }
            if start > end {
                write_status(
                    &mut stream,
                    416,
                    "Range Not Satisfiable",
                    &[("Content-Range", &format!("bytes */{size}"))],
                );
                return;
            }
            if file.seek(SeekFrom::Start(start)).is_err() {
                write_status(&mut stream, 500, "Internal Error", &[]);
                return;
            }
            let len = end - start + 1;
            let headers = format!(
                "HTTP/1.1 206 Partial Content\r\n\
                 Content-Type: {ctype}\r\n\
                 Content-Length: {len}\r\n\
                 Content-Range: bytes {start}-{end}/{size}\r\n\
                 Accept-Ranges: bytes\r\n\
                 Connection: close\r\n\
                 Access-Control-Allow-Origin: *\r\n\
                 \r\n"
            );
            if stream.write_all(headers.as_bytes()).is_err() {
                return;
            }
            copy_range(&mut file, &mut stream, len);
        }
        _ => {
            let headers = format!(
                "HTTP/1.1 200 OK\r\n\
                 Content-Type: {ctype}\r\n\
                 Content-Length: {size}\r\n\
                 Accept-Ranges: bytes\r\n\
                 Connection: close\r\n\
                 Access-Control-Allow-Origin: *\r\n\
                 \r\n"
            );
            if stream.write_all(headers.as_bytes()).is_err() {
                return;
            }
            copy_range(&mut file, &mut stream, size);
        }
    }
}

fn parse_range(spec: &str, size: u64) -> (u64, u64) {
    if let Some((s, e)) = spec.split_once('-') {
        let start = if s.is_empty() {
            0
        } else {
            s.parse().unwrap_or(0)
        };
        let end = if e.is_empty() {
            size.saturating_sub(1)
        } else {
            e.parse().unwrap_or(size.saturating_sub(1))
        };
        (start, end)
    } else {
        (0, size.saturating_sub(1))
    }
}

fn copy_range(src: &mut std::fs::File, dst: &mut TcpStream, mut remaining: u64) {
    let mut buf = [0u8; 64 * 1024];
    while remaining > 0 {
        let to_read = (remaining as usize).min(buf.len());
        match src.read(&mut buf[..to_read]) {
            Ok(0) => break,
            Ok(n) => {
                if dst.write_all(&buf[..n]).is_err() {
                    break;
                }
                remaining -= n as u64;
            }
            Err(_) => break,
        }
    }
    let _ = dst.flush();
}

fn write_status(stream: &mut TcpStream, code: u16, msg: &str, extra: &[(&str, &str)]) {
    let mut h = format!("HTTP/1.1 {code} {msg}\r\nContent-Length: 0\r\nConnection: close\r\n");
    for (k, v) in extra {
        h.push_str(&format!("{k}: {v}\r\n"));
    }
    h.push_str("\r\n");
    let _ = stream.write_all(h.as_bytes());
}
