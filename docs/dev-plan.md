# VideosFlow 功能开发计划（v1.2 · 2026-07-11 决策已固化 + TTS 选型）

> 主理人：**齐活林（Qi）· 交付总监** | 团队：`software-videosflow-plan`
> 配套文档：[`technical-solution.md`](./technical-solution.md)（架构详设）、[`../README.md`](../README.md)（项目说明）
> 本计划覆盖：功能完善 → 测试 → 调试 → 正式启用的完整路线。
> **v1.1 更新**：§1.4 五项待确认问题已由 Boos 拍板，固化为 MVP 基线；同步调整 §1.1/§1.3/§2.5/§2.7/§4.1/§4.4/§6。
> **v1.2 更新**：TTS 选型确认 → XiaomiMimo（`https://api.xiaomimimo.com/v1`，`mimo-v2.5-tts`），与 Agnes 构成双网关；新增「密钥安全红线」（API Key 由用户在设置页填、存系统凭据库、不进 git）；`.gitignore` 追加 `.env` 忽略。

---

## 0. 主理人说明（请先读这一段）

### 0.1 当前基线（已完成的）
项目不是从零开始，P0 工程骨架已立，UI 已成型：

| 项 | 状态 |
|----|------|
| Tauri 2 + React 18 + TypeScript 桌面端骨架 | ✅ 已搭 |
| 四大模块 UI（影片 / 口播 / 创作 / 设置） | ✅ 全部可点验 |
| **Editorial Design System v3.0**（自研 CSS 设计系统 + Lucide 图标） | ✅ 已落地 |
| 响应式 + light/dark 双主题 | ✅ 已落地 |
| 剪映 `draft_content.json` 导出（mock 构造） | ✅ 已落地 |
| AI 引擎 / FFmpeg / SQLite 真实能力 | ❌ **全无，均为 mock** |

**一句话**：UI 是个「漂亮空壳」，所有能力按钮点了跑的是前端模拟（`sim()`），没有真后端。

### 0.2 关于团队协作流程（必须坦诚）
本计划由**主理人整合多视角产出**（产品经理 PRD + 架构师设计 + QA 路线），而非由独立 agent 并行调度。原因：

> 当前 workbuddy 环境**未预置软件团队成员的 agent 定义文件**（`software-product-manager` / `software-architect` / `software-engineer` / `software-qa-engineer` 均不存在），`Agent({subagent_type:"software-*"})` 会因类型不被识别而失败。

若后续要走**真·多智能体 SOP**（各成员独立 agent 输出、主理人中转），需先创建 `C:/Users/HomePC/.workbuddy/agents/software-*.md` 四个定义文件。这一步可作为**可选前置**，不影响本计划本身的执行——计划可由主理人（我）直接在 Craft 模式下一阶段一阶段落地。

### 0.3 两个必须校正的冲突（以「已落地代码」为准）
`technical-solution.md` 写于 UI 落地之前，有两处与实际不符，本计划一律**以已落地代码为准**：

| 方案文档说法 | 实际已落地 | 本计划采用 |
|------|------|------|
| 状态管理用 **Zustand** | 实际用 **React Context**（`src/state/AppContext.tsx`，单一 store + actions） | **沿用 React Context**；仅当模块状态膨胀到难以维护时再迁 Zustand |
| UI 用 **Ant Design 5** | 实际用 **Editorial Design System v3.0**（自研 `global.css` + Lucide） | **沿用自研 UI**，不再引入 AntD |
| 本地模型优先（faster-whisper / diffusers） | — | **MVP 纯云 API（已确认决策 4）**，本地模型从 MVP 范围移除，仅作远期增强 |

---

## 1. 增量 PRD（产品经理：许清楚视角）

### 1.1 目标与成功标准
**目标**：把 VideosFlow 从「可点验的 mock 原型」变为「能端到端跑通真实视频生产」的桌面应用。

**MVP 范围（已确认决策）**：
- **纯云 API**（需联网，决策 4）
- **Windows / Linux / macOS 三端兼容**（决策 1）
- **暂不做代码签名**（决策 1，接受系统安全警告）
- **硬件加速默认关闭**，导出默认 CPU 软编码 `libx264`（决策 5，部分用户无独显）

**成功标准（上线判定）**：
- 四大模块任一完整链路可端到端跑通真实 AI + FFmpeg，产出可用 MP4；
- 配置真实生效（Agnes / Mimo API Key 存系统凭据、连接测试可用）；
- 任务有真实进度反馈（WebSocket）；
- 工程数据持久化（SQLite，重开不丢）；
- 安装包可分发到 Windows / Linux / macOS（MVP 未签名，接受安全警告）。

### 1.2 用户故事（按模块）
- **影片**：作为剪辑师，我导入「素材视频 + 文案」，系统按文案智能粗剪，我在时间线上精修、加字幕花字，导出成片。
- **口播**：作为口播作者，我上传视频，系统抽音轨识别出文案，标出气口/口误/重复，我采纳纠正，系统生成重点高亮 + 花字字幕并烧录。
- **创作**：作为创作者，我填需求，系统自动写文案→去 AI 味→分镜→配图→首尾帧视频→配音字幕→导出成片。
- **设置**：作为用户，我在此配置 Agnes / Mimo 双网关 API、管理提示词、调全局参数（硬件加速/分辨率/并发/清理）。

### 1.3 需求池（优先级）

#### P0（地基，阻塞一切）
- [ ] SQLite 工程库落地（类型/工程/口播/创作/任务等表）
- [ ] Provider 配置框架真实生效 + 连接测试（替代 mock `test:'ok'`）
- [ ] Tauri 命令骨架 + IPC 协议（invoke 调 Rust）
- [ ] Python sidecar 启动/守护/崩溃隔离
- [ ] 任务队列 + WebSocket 进度推送
- [ ] FFmpeg 首启下载器（按平台下载到缓存，不随包）

#### P1（影片模块）
- [ ] 类型树可增删改查（持久化）
- [ ] 工程库持久化（封面/状态/标签）
- [ ] 导入视频 + 文案 → 抽音轨 ASR → 文案-时间戳对齐
- [ ] 基于文案智能剪辑（自动切点生成粗剪时间线）
- [ ] 时间线精修（多轨拖拽/裁剪/转场/音量）
- [ ] 字幕生成 + 花字模板预览烧录（ASS）
- [ ] 合成导出 MP4（默认软编码，可选硬件加速）

#### P2（口播模块）
- [ ] 上传视频 → FFmpeg 抽音轨 → ASR 逐句
- [ ] 文案提取（纯文案 / 带轴）
- [ ] 纠正检测（气口 VAD / 口误 LLM / 重复编辑距离）
- [ ] 采纳/忽略交互（不破坏原片）
- [ ] 关键词抽取 + 重点高亮 + 花字烧录
- [ ] 出口：净化文案可送入「创作」或「影片」

#### P3（创作上：需求→分镜→图片）
- [ ] 需求表单持久化
- [ ] 自动写文案（LLM，已接 prompt 模板）
- [ ] 去 AI 味（LLM + 规则层）
- [ ] 分镜生成（LLM JSON）
- [ ] 分镜图片生成（注入 styleRef + 一致性抽检）

#### P4（创作下：视频→配音→导出）
- [ ] 首尾帧视频生成（图生视频/帧插值）
- [ ] TTS 配音（多音色）
- [ ] 字幕对齐烧录
- [ ] 合成导出 + 归档到影片模块

#### P5（打磨 + 增强，可选）
- [ ] 本地模型适配（faster-whisper / SD / CosyVoice）— **MVP 不做（纯云，见决策 4）**
- [ ] 撤销/重做、代理渲染、批量处理
- [ ] 模板固化工作流、可观测性面板
- [ ] 自动更新

### 1.4 已确认决策（Boos 拍板 · 2026-07-11）
| # | 问题 | 决策 |
|---|------|------|
| 1 | 目标平台 / 签名 | **Windows + Linux + macOS 三端兼容**；**签名证书暂无** → MVP 先不签名，接受 SmartScreen / Gatekeeper 警告，后续采购 EV 证书 / Apple Developer ID 再补 |
| 2 | AI 厂商 | **双网关**：① **Agnes**（LLM/图像/视频）`base_url = https://apihub.agnes-ai.com/v1`（OpenAI 兼容风格，单网关按 ModelID 路由），Model：`agnes-2.0-flash`(LLM) / `agnes-image-2.1-flash`(图像) / `agnes-video-v2.0`(视频)；② **TTS 走 XiaomiMimo** `base_url = https://api.xiaomimimo.com/v1`，Model：`mimo-v2.5-tts`。**密钥管理（红线）**：Agnes 与 Mimo 的 API Key **均由用户在「设置」页填写，存系统凭据库（Windows Credential Manager / macOS Keychain / Linux libsecret），不落代码、不进 git**；应用运行时经 sidecar 读取转发，**切勿硬编码 key 进源码或任何会被 commit 的文件**。 |
| 3 | FFmpeg 分发 | **不随包，首次启动按平台下载**到本地缓存目录（需选国内可达镜像 + SHA256 校验） |
| 4 | 云 vs 离线 | **MVP 纯云 API**（需联网）；本地模型适配从 MVP 范围移除 |
| 5 | 硬件底线 | **用户机器高低不一、部分无独显** → 硬件加速默认关闭；导出默认 CPU 软编码 `libx264`；运行时探测 nvenc / metal / vaapi 后可选开启 |

---

## 2. 系统架构与任务分解（架构师：高见远视角）

### 2.1 三层进程模型（不变，沿用方案）
```
WebView(React) ──Tauri invoke──▶ Rust 编排 ──HTTP──▶ Python AI 引擎 ──▶ Agnes API
   ▲                            │  ▲
   │ 进度 WebSocket             │  └──FFmpeg──▶ 本地媒体
   └────────────────────────────┘
```
- **Rust**：文件系统、SQLite、FFmpeg（首启下载+编排）、Python 守护、任务队列。
- **Python sidecar (FastAPI)**：ASR / LLM / 图 / 视频 / TTS，对上游屏蔽厂商差异；MVP 只做**轻量转发**到 Agnes（不内置 torch/whisper，体积小）。
- **前端**：React Context 单一 store，经 `invoke` 调 Rust，经 WebSocket 订阅进度。

### 2.2 目标态目录结构
```
src-tauri/
├─ src/
│  ├─ main.rs                 # 入口，挂载命令
│  ├─ lib.rs                  # tauri::Builder，注册命令/状态/WebSocket
│  ├─ db.rs                   # SQLite (sqlx) 建表/CRUD
│  ├─ ffmpeg.rs               # FFmpeg 命令封装 + 首启下载器
│  ├─ python.rs               # sidecar 启动/守护/健康检查
│  ├─ tasks.rs                # 任务队列 + 进度广播 (tokio + broadcast)
│  └─ commands.rs             # 所有 #[tauri::command] 实现
├─ binaries/                  # （留空，FFmpeg 不随包，见决策 3）
└─ tauri.conf.json            # 外部 bin / 打包配置 / 三端 target
python-sidecar/
├─ main.py                    # FastAPI 入口
├─ routers/                   # asr / script / spoken / gen / tts
├─ providers/                 # AgnesProvider 实现 LLM/ASR/Image/Video/TTS 统一接口
└─ models.py                  # 统一信封 / 数据模型
src/
├─ state/AppContext.tsx       # 现状：React Context（沿用）
├─ ipc/                       # 新增：invoke 封装 + WebSocket 客户端 + 任务订阅
├─ modules/                   # film/spoken/creation/settings（逐步接真实 action）
└─ styles/global.css          # Editorial Design System v3.0（已落地）
```

### 2.3 数据模型（沿用方案 §7 的 SQLite 表，略）
已在 `technical-solution.md §7` 定义 11 张表（`film_categories` / `film_projects` / `edit_timelines` / `spoken_videos` / `spoken_edits` / `creation_projects` / `storyboards` / `generated_assets` / `voiceovers` / `subtitles` / `provider_config` / `tasks`）。本计划不再重复，实现时照表建。

### 2.4 IPC 协议（沿用方案 §8，略）
`film_*` / `spoken_*` / `creation_*` 系列 invoke + `ws://127.0.0.1:<port>/progress`。实现时把现有 `actions.sim()` 逐一替换为真实 `invoke`。

### 2.5 任务分解（按里程碑，含文件级）

#### M0 — 基础设施（阻塞 P1-P4）
| 任务 | 文件 | 依赖 |
|------|------|------|
| 引入 sqlx，建 11 张表（首次启动 migrate） | `src-tauri/src/db.rs` | — |
| FFmpeg 命令封装（抽音轨/切/拼/烧字幕/导出） | `src-tauri/src/ffmpeg.rs` | — |
| **FFmpeg 首启下载器**（按平台下载到缓存目录 + 完整性校验，不随包内置） | `src-tauri/src/ffmpeg.rs` | — |
| Python sidecar 启动/守护/健康检查 | `src-tauri/src/python.rs` | — |
| FastAPI 脚手架 + 统一信封 + /tasks（**仅轻量转发 Agnes**） | `python-sidecar/main.py`, `routers/`, `models.py`, `providers/AgnesProvider.py` | — |
| 任务队列 + 进度广播 | `src-tauri/src/tasks.rs` | db |
| Tauri 命令注册 + WebSocket 服务（**三端兼容**） | `src-tauri/src/lib.rs`, `commands.rs` | tasks, python |
| 前端 IPC 封装（invoke + WS 客户端 + 任务订阅 hook） | `src/ipc/*.ts` | — |
| Provider 配置真实持久化 + 连接测试（**默认填充 Agnes**：`https://apihub.agnes-ai.com/v1`，模型 `agnes-2.0-flash` / `agnes-image-2.1-flash` / `agnes-video-v2.0`） | `commands.rs`, `db.rs`, `src/ipc/providers.ts` | db |
**验收**：启动 app → Python sidecar 自动起 → FFmpeg 首启下载成功（`ffmpeg -version` 可用）→ 在设置页填真实 Agnes Key → 连接测试返回 `ok`；关闭重启配置不丢。

#### M1 — 设置模块真实化（最早见效，打通链路）
| 任务 | 文件 | 依赖 |
|------|------|------|
| 设置页 API/提示词/参数三个子页接真实读写 | `src/modules/Settings.tsx` | M0 |
| Provider 测试命令接真实 Agnes（LLM ping / 图像试生成 / 视频试生成） | `python-sidecar/routers/*` | M0 |
**验收**：填真实 Key 能跑通一次最小 Agnes LLM 调用；提示词模板 `{{brief}}` 变量可替换。

#### M2 — 影片模块
| 任务 | 文件 | 依赖 |
|------|------|------|
| 类型树增删改查（持久化） | `commands.rs`, `Film.tsx` | M0 |
| 工程库持久化 | `db.rs`, `Film.tsx` | M0 |
| 导入视频 → 抽音轨 → ASR 对齐 | `ffmpeg.rs`, `python/asr`, `Film.tsx` | M1 |
| 基于文案智能剪辑（自动切点） | `commands.rs` + 对齐算法 §6.0 | M1 |
| 时间线精修（多轨） | `Film.tsx` + `edit_timelines` | M2 上 |
| 字幕/花字预览烧录 | `ffmpeg.rs` + ASS | M2 上 |
| 导出 MP4（**默认 libx264 软编码，可选硬件加速**） | `ffmpeg.rs` | M2 上 |
**验收**：导入「素材+文案」→ 自动出粗剪时间线 → 精修 → 加花字 → 导出 MP4，文件可播放。

#### M3 — 口播模块
| 任务 | 文件 | 依赖 |
|------|------|------|
| 上传 + 抽音轨 + ASR | `spoken_upload`, `asr_transcribe` | M1 |
| 文案提取 | `spoken_extract` | M3 上 |
| 纠正检测（VAD+LLM+重复） | `spoken_detect` + §6.1 | M3 上 |
| 采纳/忽略（不破坏原片） | `spoken_apply_edits` | M3 上 |
| 关键词 + 花字烧录 | `spoken_flower_text` + §6.2 | M3 上 |
| 出口到创作/影片 | 跨模块 action | M3 |
**验收**：上传口播 → 出文案 → 标出气口/口误/重复 → 采纳 → 花字烧录成片。

#### M4 — 创作上（需求→分镜→图片）
| 任务 | 文件 | 依赖 |
|------|------|------|
| 需求持久化 | `creation_projects` | M0 |
| 自动写文案（接 Agnes LLM + prompt 模板） | `script_write` | M1 |
| 去 AI 味（LLM + 规则层） | `script_humanize` | M1 |
| 分镜生成（LLM JSON） | `storyboard_gen` | M1 |
| 分镜图片（Agnes 图像 + 一致性抽检） | `image_gen` + §6.5 | M4 上 |
**验收**：填需求 → 走完前 5 步 → 出一致风格分镜图。

#### M5 — 创作下（视频→配音→导出）
| 任务 | 文件 | 依赖 |
|------|------|------|
| 首尾帧视频（Agnes 视频模型） | `frames_gen` + §6.6 | M4 |
| TTS 配音（多音色，走 XiaomiMimo `mimo-v2.5-tts`） | `tts_gen` | M1 |
| 字幕对齐烧录 | `ffmpeg` + §6.7 | M4 |
| 合成导出 + 归档影片 | `export` | M4/M5 |
**验收**：出带配音+字幕成片，可归档到影片模块。

#### M6 — 打磨 + 集成 + 启用
- 撤销/重做、批量、模板固化
- 错误边界、空态、加载态补全（接真实异步）
- 自动更新
- **三端构建分发（无签名，见 §4）**
- 本地模型适配（**仅远期增强，MVP 不做**）

### 2.6 实现顺序（关键路径）
```
M0(基础设施) → M1(设置真实化) → M2(影片) → M3(口播) → M4(创作上) → M5(创作下) → M6(打磨启用)
```
M1 越早做越好——它是验证「前端↔Rust↔Python↔Agnes」全链路的最小闭环，能尽早暴露集成问题。

### 2.7 风险与待明确
- **Python 打包体积**：MVP 纯云 API（决策 4），sidecar 只做轻量转发到 Agnes，不内置 torch/whisper，体积可控。
- **FFmpeg 首启下载**：下载源稳定性/完整性（决策 3，不随包）；需选**国内可达镜像** + SHA256 校验，断网时功能降级提示。
- **WebSocket 端口冲突**：用随机端口 + 健康检查重试。
- **跨平台 WebView**：Windows/macOS 自带 WebView2/WKWebView；**Linux 需确保系统 WebKit2GTK 已装**（Tauri 2 依赖），安装包应提示或打包依赖。
- **硬件加速默认关闭**（决策 5，部分用户无独显）：导出默认 `libx264` 软编码，运行时探测 nvenc / metal / vaapi 后可选开启。
- **签名缺失**（决策 1）：MVP 未签名，Windows SmartScreen / macOS Gatekeeper 会告警，需提供校验与手动允许指引。
- **TTS 厂商已定**（决策 2）：XiaomiMimo `mimo-v2.5-tts`，与 Agnes 分属双网关；sidecar 需支持按能力路由到不同 `base_url`。

---

## 3. 测试与调试策略（QA：严过关视角）

### 3.1 测试分层
| 层 | 工具 | 范围 |
|----|------|------|
| 单元 | `cargo test` / `pytest` | Rust 命令、FFmpeg 参数构造、Python provider 适配、对齐算法 |
| 集成 | 自建 harness | 前端 `invoke` → Rust → Python → Agnes 的端到端命令 |
| E2E | Playwright + Tauri | 走通各模块完整链路（用 Agnes 免费/测试额度） |
| UI 快照 | Chromatic/手写 | Editorial 设计系统组件回归 |

### 3.2 各模块关键用例（节选）
- **M0**：SQLite 建表幂等；sidecar 崩溃自动重启；WebSocket 进度连续不丢；**FFmpeg 首启下载成功且 `ffmpeg -version` 可用**；Agnes 连接测试在错 Key 时返回可读错误。
- **M1**：错误 API Key 返回可读错误；prompt 变量替换正确。
- **M2**：文案-时间戳对齐准确率（人工抽检 5 条）；粗剪时间线片段连续无重叠；导出 MP4 可播放（软编码）。
- **M3**：ASR 字错率抽检；纠正建议无漏判（口误/重复）；采纳后原片不变。
- **M4/M5**：分镜图片跨镜头相似度 > 阈值；配音与字幕时间对齐误差 < 200ms。

### 3.3 调试手段
- **Rust**：`RUST_LOG=debug` + Tauri devtools；命令入口统一打 `invoke` 入参。
- **Python**：FastAPI `/docs` 自测；`python-sidecar/logs/` 落盘；Agnes 调用包 `{request,response,latency,error}`。
- **前端**：`src/ipc` 统一 log 每次 invoke 与 WS 消息；任务栏进度条接真实 `progress`。
- **复现包**：任务失败自动打包 `{input_meta, log, error}` 供回滚分析。

### 3.4 Bug 管理
- 每个 bug 关联 `tasks` 表 `log` 字段 + 复现步骤；
- 回归测试：修复后跑对应模块 E2E；
- 严重阻塞（链路断裂）立即回滚到上一稳定里程碑。

---

## 4. 正式启用路线

### 4.1 构建与签名
- `npm run tauri:build` 产出 **Windows(.msi/.exe) / Linux(.deb/.AppImage) / macOS(.dmg)** 三端安装包；
- **签名证书暂无**（决策 1）：MVP **暂不签名**。
  - Windows：提供 SHA256 校验值 + 手动允许 SmartScreen 指引；
  - macOS：提供「右键 → 打开」绕过 Gatekeeper 指引；
  - 后续采购 EV 证书 / Apple Developer ID 再补签名与公证。
- FFmpeg **不随包**，首次启动按平台下载到缓存（见 §2.7）。

### 4.2 数据安全
- API Key 存**系统凭据库**（Windows Credential Manager / macOS Keychain / Linux libsecret），不落 SQLite 明文（当前 mock 里 `apiKey:'sk-***'` 是明文，必须改）。
- `provider_config` 表只存 `provider/base_url/model`，key 走凭据。

### 4.3 自动更新
- 用 Tauri Updater（需签名私钥 + `latest.json`）；MVP 无签名则**暂缓**，后续补。
- 更新通道：stable / beta。

### 4.4 发布检查清单
- [ ] 三平台（Windows / Linux / macOS）安装包构建通过
- [ ] 未签名场景的校验与手动允许指引就绪（MVP 无证书，见 §4.1）
- [ ] 全模块 E2E 绿灯
- [ ] API Key 不落明文
- [ ] 首次启动引导（填 Agnes Key / Mimo Key / FFmpeg 下载）
- [ ] 崩溃可上报 + 自动恢复未完成任务

---

## 5. 里程碑总表

| 里程碑 | 范围 | 核心交付 | 验收门槛 | 依赖 | 预估工作量* |
|--------|------|----------|----------|------|------------|
| **M0** | 基础设施 | SQLite + FFmpeg(首启下载) + Python sidecar(Agnes 转发) + 任务队列 + IPC + WS | 配置持久化 + FFmpeg 下载成功 + 连接测试通过 | — | 大 |
| **M1** | 设置真实化 | 三子页读真实 Agnes + 连接测试 | 真实 Agnes LLM 最小调用成功 | M0 | 小 |
| **M2** | 影片 | 类型/工程/智能剪辑/时间线/字幕花字/导出 | 导出入可播 MP4（软编码） | M1 | 大 |
| **M3** | 口播 | 上传/ASR/纠正/花字/出口 | 净化成片可播 | M1 | 大 |
| **M4** | 创作上 | 需求→文案→去味→分镜→图片 | 一致分镜图 | M1 | 中 |
| **M5** | 创作下 | 首尾帧→配音→字幕→导出归档 | 带配音字幕成片 | M4 | 中 |
| **M6** | 打磨启用 | 撤销/批量/模板/更新/三端分发（签名采购后补） | 安装包可分发三端 | M2-M5 | 中 |

\* 工作量为主理人经验估算（大/中/小），具体人日需 Boos 确认团队规模与每日投入后细化。

---

## 6. 立即可开始的下一步（给 Boos）

1. **M0 先行**：我先落地 SQLite + Python sidecar(转发 Agnes) + FFmpeg 首启下载 + 一个 `ping` 命令 + WebSocket 进度，打通最小闭环。
2. **（可选）建 agent 定义**：若要走真·多 agent SOP，我创建 4 个 `software-*.md` 后再重跑本计划的分派。
3. **增量提交**：每完成一个里程碑，按现有 SSH remote 提交 GitHub（已沉淀到 MEMORY.md）。
4. **TTS 已定**：XiaomiMimo `mimo-v2.5-tts`（双网关之 TTS 侧），无需再确认；M5 配音走此网关，M0/M1 实现时把双网关路由做进 sidecar。

---

_编制：主理人齐活林（Qi）· 交付总监（整合 PRD/架构/QA 视角） · 2026-07-11 v1.1_
_注：因当前 workbuddy 环境未预置独立软件团队 agent 定义，本报告由主理人整合产出；独立多 agent 调度需先创建 agent 定义文件。MVP 基线见 §1.4 已确认决策。_
