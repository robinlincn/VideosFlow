// VideosFlow — Tauri2 应用入口
// 当前为工程骨架：仅承载 WebView 与基础窗口生命周期。
// 后续 AI 引擎（Python sidecar）、FFmpeg 编排、SQLite 工程库将在此挂载。

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|_app| {
            #[cfg(debug_assertions)]
            {
                // 开发期占位：可在此注入 devtools / 预置目录等
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running VideosFlow");
}
