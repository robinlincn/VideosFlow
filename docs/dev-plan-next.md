# VideosFlow 下一步开发计划（v1.3 · 2026-07-13）

> 状态：M0–M4 全部完成且真实化（Rust + SQLite + AES-GCM 加密存）。剩余 M5 创作下 + M6 打磨。
> 主理人：齐活林（Qi）| 配合文档：dev-plan.md（里程碑总表）、m3-spoken-design.md / m4-creation-design.md（已落地设计）

---

## 0. 当前基线（2026-07-13）

| 项 | 状态 | 备注 |
|----|------|------|
| M0 基础设施 | ✅ | SQLite 16 表 + 双网关 Provider seed + 任务队列 + IPC |
| M1 设置真实化 | ✅ | provider_test + chat 真实链路验证已通过 |
| M2 影片模块 | ✅ | 智能粗剪 + 时间线精修 + 字幕花字 + 导出 |
| M3 口播模块 | ✅ | 上传+ASR+检测(gap/repeat/mistake)+关键词+素材+烧录+干净导出 |
| M4 创作上 | ✅ | 需求→写文案→去AI味→分镜→图片 |
| M5 创作下 | ⏳ | 首尾帧→配音→字幕→合成→归档 |
| M6 打磨启用 | ⏳ | 模板/撤销/批量/自动更新/三端分发 |
| 凭据库 | ✅ | AES-256-GCM（替代 keyring） |
| 桌面版验证 | ✅ | Boos 本机 MSVC + Windows SDK 10.0.26100 跑通 |
| 链路验证 · 真实 Agnes | ✅ | "获取到正确内容" |

---

## 1. M5 — 创作下（首尾帧→配音→字幕→合成→归档影片）

### 1.1 目标
把 M4 生成的图片 + 文案，串成可播放的成片，并归档到影片模块。

### 1.2 任务列表（有序、含依赖）

| 任务 | 文件 | 依赖 | 优先级 |
|------|------|------|------|
| **M5-T1** 数据层：voiceovers / subtitles 已有 schema，补 CRUD | `db.rs` | M0 | P0 |
| **M5-T2** 首尾帧视频任务 `frames_gen`：Agnes `/videos/generations`（image→video），base64 → mp4 → `data_dir/creation_assets/{pid}/shot_{i}_{ts}.mp4` | `tasks.rs` | M4 image_gen | P0 |
| **M5-T3** 多声音配音任务 `tts_voice_gen`：XiaomiMimo `/audio/speech`，多 voice_id 并行（已是 M2 链路，封装为独立 task） | `tasks.rs` | M2 tts | P0 |
| **M5-T4** 字幕生成任务 `subtitle_gen`：从 humanized_script 标点切分 → 按 TTS 时长对齐 → 写 `subtitles` 表 | `tasks.rs` | M5-T3 | P0 |
| **M5-T5** 合成导出任务 `creation_export`：每镜按 dur 切图片帧视频 → concat + 配音 + 烧字幕 → 软编码 mp4 | `tasks.rs` | M5-T2/T3/T4 | P0 |
| **M5-T6** 归档任务 `archive_to_film`：mp4 复制到 `data_dir/film_projects/{id}/output.mp4` + INSERT `film_projects` | `tasks.rs` | M5-T5 | P0 |
| **M5-T7** 前端 5 步 UI 接真实 IPC：首尾帧/配音/字幕/导出 4 步按钮 + 进度条 + 归档到影片 | `Creation.tsx`, `AppContext.tsx` | M5-T2..T6 | P0 |
| **M5-T8** 剪映工程导出（最终版）：视频轨 + 配音轨 + 字幕轨 + 花字轨，构造 `draft_content.json` 下载 | `lib/jianying.ts` | M3 mock 升级 | P1 |
| **M5-T9** 移除 Creation.tsx 的 M5 占位提示（frames/voice/export 三步接真实功能） | `Creation.tsx` | M5-T7 | P0 |
| **M5-T10** 校验：Boos 本机从「创作需求」到「归档影片」跑通端到端 | 验收 | M5-T1..T9 | P0 |

### 1.3 关键算法
- **首尾帧视频**（Agnes `agnes-video-v2.0`）：以分镜图作首帧 + 文案色调作末帧 + 运镜提示作 motion，2-6s/镜。
- **字幕对齐**：用 TTS 音频时长 / 句数 ≈ 句均时长，按标点切分给字幕。
- **合成**：参考 M2 `film_export` 的 segment + concat + burn_ass + mux 链路。
- **降级**：首尾帧视频失败 → 静态图 + Ken Burns 效果（ffmpeg zoompan filter）；TTS 失败 → 静音输出 + 字幕优先。

### 1.4 数据约束
- `subtitles(id, project_id, start, end, text, style_id)` 已有 schema。
- `voiceovers(id, project_id, shot_id, voice_id, path)` 已有 schema。
- 生成的视频和音频统一存 `data_dir/creation_assets/{projectId}/`。

### 1.5 验收门槛
- 在桌面版走完「创作需求 → 写文案 → 去 AI 味 → 分镜 → 图片 → 首尾帧 → 配音 → 字幕 → 合成 → 归档」全链路，最终能在「影片」模块里看到新归档的工程，mp4 可播放。
- 字幕时间轴误差 < 200ms。
- 整链路断网/缺 Key 时降级路径走通，UI 不崩。

---

## 2. M6 — 打磨启用

### 2.1 目标
让 VideosFlow 进入"能上生产线"的状态：撤销/重做、批量、模板、自动更新、跨平台打包。

### 2.2 任务列表

| 任务 | 范围 | 优先级 |
|------|------|------|
| **M6-T1** 撤销/重做：AppContext 加 `history` 栈 + `undo`/`redo` actions（最大 50 步） | AppContext | P1 |
| **M6-T2** 批量操作：影片库多选删除/移动分类；口播批量纠正；创作批量重生成 | Film/Spoken/Creation | P2 |
| **M6-T3** 模板固化：把"需求+风格+分镜"保存为模板，下次直接套用 | Creation | P1 |
| **M6-T4** 自动更新：Tauri Updater（需签名私钥 + latest.json，MVP 暂不签） | Tauri | P2 |
| **M6-T5** 三端打包：Windows .msi / Linux .deb + .AppImage / macOS .dmg（无签名，提示用户允许） | Tauri | P1 |
| **M6-T6** 错误边界：ErrorBoundary 组件捕获渲染异常 + 上报日志 | App.tsx | P1 |
| **M6-T7** 可观测：启动时输出环境（rustc/tauri/SQLite 路径/Key 状态） | lib.rs | P2 |
| **M6-T8** i18n：settings.other.lang 已有字段，加 i18next / 自研轻量字典 | Settings | P2 |
| **M6-T9** UI 主题切换：已有 light / Editorial Noir，加 system 跟随 | App.tsx | P2 |
| **M6-T10** 性能：大文件懒加载缩略图 / 虚拟列表 / Web Worker 处理 FFmpeg 进度 | 全局 | P2 |

### 2.3 验收门槛
- 撤销/重做跨模块工作正常。
- 模板新建工程可一键复用，节省 70% 重复操作。
- 三端安装包可分发（无签名警告属预期）。
- 崩溃有友好降级 UI，不白屏。

---

## 3. 已知 Bug / 待修

| ID | 描述 | 严重性 | 涉及文件 | 状态 |
|----|------|--------|----------|------|
| BUG-1 | M4 创作上 `image_gen` mock 路径下 React state 时序问题（ImageView 偶现需手动 step 切换触发 re-render）；生产 Rust 端无此问题 | 中 | `Creation.tsx`, `AppContext.tsx` | 已知，桌面版不触发 |
| BUG-2 | `tasks.rs::run_film_export` 警告：`unused variable: _port`（已修）+ M3 接口相关 `_port` 警告残留 | 低 | `tasks.rs` | 残留 |
| BUG-3 | `app_spoken_drafts` 没存编辑草稿——刷新后 `cState.story` 编辑丢失 | 中 | `AppContext.tsx`, `Creation.tsx` | M5 顺手做 |
| BUG-4 | Vite dev server 默认 port 5173 抢占；`vite.config.ts` 已加 watch.ignored 排除 target | 低 | `vite.config.ts` | 已修 |
| BUG-5 | 链上无 Tauri 自启动 devtools（开发期需要按 F12 看不到 console） | 低 | `tauri.conf.json` | 后续 |

---

## 4. 关键技术债

| 项 | 说明 | 优先级 |
|----|------|------|
| **TTS 时长探测** | XiaomiMimo TTS 返回音频字节但没时长，需要 ffprobe 探测；M5 字幕对齐前要先解时长 | P0 |
| **CLAUDE.md 同步** | 仓库根没 CLAUDE.md（与 Claude Code 配合），可加开发指南 | P2 |
| **mock 路径生产化** | `ipc/client.ts::mockInvoke` 仅 dev 兜底，应加 `if (import.meta.env.DEV)` 包裹避免打包进 prod | P1 |
| **错误码规范化** | Rust 端 `Result<T, String>` 改 `Result<T, AppError>` 含 code+message | P2 |
| **可观测性** | 加 `tracing` crate + 任务执行时间日志 | P2 |
| **FFmpeg 错误友好化** | 抽音轨失败时 stderr 输出原始 ffmpeg 命令 + 末尾 200 字符 | P2 |
| **跨平台编译** | 当前 MSVC 验证通过；需补充 gnu target 跨平台测试 | P2 |
| **AGPL 第三方风险** | SQLite 默认无 AGPL 问题（`bundled-sqlcipher` 才需 AGPL），但要做 license audit | P2 |

---

## 5. 下一步立即执行（按优先级）

1. **M5 完整设计文档**：照 M3/M4 同款结构出 `docs/m5-creation-design.md`（数据 / 接口 / 任务依赖 / Boos 待确认）
2. **M5 Boos 拍板**：图片→视频 风格、字幕对齐策略、配音 voice_id 列表
3. **M5 实施**：T1-T9 顺序推进
4. **M5 桌面端验收**：Boos 跑全链路
5. **M5 提交 + push**：commit message 用 `feat(M5): 创作下端到端真实化`

---

## 6. 路线图总览

```
M0 ✅ → M1 ✅ → M2 ✅ → M3 ✅ → M4 ✅ → M5 ⏳ → M6 ⏳
                ↓
        AES-GCM 凭据 (2026-07-13 切换)
```

预计：
- M5（创作下）：**3-4 周**（含 5 个异步任务 + 复杂合成）
- M6 打磨 + 上线：并行推进，**1-2 个月**

---

_编制：齐活林（Qi） · 2026-07-13 · v1.3_
_配套：dev-plan.md（v1.2 历史） / README.md（产品定位） / m3/m4 设计文档（已完成模板）_