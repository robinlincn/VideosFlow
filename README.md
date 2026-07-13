# VideosFlow · 智能视频生产工作室

> 一款基于 Web 技术的桌面端视频生产工具。把三类高频工作收敛为**三大模块**：**影片**（类型化工程库 + 成片剪辑）、**口播**（上传视频识别音频、提取文案、净化与花字）、**创作视频**（从大体需求自动生成完整视频：文案 → 去AI味 → 分镜 → 图片 → 首尾帧视频 → 配音 → 字幕）。

---

## 一、产品定位

VideosFlow 是面向"视频生产全流程"的桌面工作台。**本三大模块是在用户最初提出的三大原始需求之上优化重组而成**（见第二节），既保留了全部原始能力，又让组织方式更清晰、各自闭环。

一句话概括：**片库有人管、口播能净化、想法能成片、成片能精剪。**

---

## 二、需求演进：从三大原始需求 → 三大模块

用户最初提出的三大原始需求，与最终三大模块的对应关系如下：

| 原始需求 | 内容 | 优化后归属模块 | 说明 |
|----------|------|----------------|------|
| **A. 现成视频按文案剪辑** | 已有视频 + 文案 → AI 按文案结构自动剪辑 | **影片模块（剪辑功能）** | 类型化工程库之上，增加"基于文案的智能剪辑 + 时间线精修 + 字幕花字" |
| **B. 口播净化 + 花字** | 口播视频里的气口/口误/重复纠正剪辑，字幕重点标注与花字 | **口播模块** | 在"识别音频→提取文案"基础上，**保留**气口/口误/重复纠正与花字能力 |
| **C. 文案二创 + 分镜 + 首尾帧** | 文案优化二创 → 分镜描述（一致）→ 在可配大模型 API 中生成首尾帧视频 | **创作视频模块** | 七步顺序不变，端到端生成 |

> 优化要点：**影片**不再只是"片库"，它承载原始需求 A 的剪辑能力；**口播**在提取文案之外明确保留原始需求 B 的纠正/花字；**创作视频**完整承接原始需求 C。

---

## 三、三大模块（需求映射与功能）

### 模块 1 · 影片（Film）—— 类型化工程库 + 成片剪辑
> 既是"片库中枢"，也是"剪辑台"。对已有影片工程做分类组织，并支持基于文案的智能剪辑。

**1）类型体系（可扩展）**
- 内置 电影 / 故事 / 电视剧 / 动画片 / 记录片，支持新增、重命名、删除、排序，**后续可持续扩展**。
- `film_categories` 数据驱动，UI 动态渲染，加类型无需改代码。

**2）工程库**
- 每个类型下管理影片工程（素材、成片、草稿），支持封面、标签、检索。
- 口播提取的文案、创作视频的产物都可归档到这里。

**3）剪辑功能（承接原始需求 A：现成视频按文案剪辑）**
- **导入素材**：把现成视频与配套文案导入同一工程。
- **基于文案智能剪辑**：AI 将文案分段 → 与视频音轨 ASR 时间戳对齐 → 在视频中定位对应片段 → 自动切点（删除静音/废片/不相关片段）→ 生成粗剪时间线。
- **时间线精修**：多轨（视频 / 音频 / 字幕 / 生成）可视化，支持拖拽、裁剪、转场、音量调整。
- **字幕 & 花字**：为成片生成/烧录字幕，支持重点句高亮与花字模板。
- **导出**：合成导出 MP4（支持硬件加速）。

### 模块 2 · 口播（Spoken）—— 识别音频 + 提取文案 + 纠正/花字
> 上传视频 → 识别音频 → 提取文案；并**保留**原始需求 B 的净化与花字能力。

- **上传/导入**：本地视频（mp4/mov/…）。
- **音频识别（ASR）**：抽音轨 → Whisper/云 ASR → 带时间戳的逐句转写。
- **文案提取**：输出纯文案（可带时间轴 / 可清理填充词），支持复制到创作模块或存为影片工程脚本。
- **纠正剪辑（保留）**：检测并标记**气口**（静音段）、**口误**（LLM 判断的错词/卡顿）、**重复**（相邻相似句/啰嗦），可一键采纳裁剪/替换，生成干净口播。
- **字幕重点标注 + 花字（保留）**：对文案关键词做重点高亮，套用花字模板（如强调、情绪、感叹），预览并烧录。
- **出口**：净化后的文案与成片可复制到创作视频作为初始需求/脚本，或归档进影片模块。

### 模块 3 · 创作视频（Creation）—— 从需求到成片，端到端七步
> 承接原始需求 C：从大体需求自动生成完整视频。

| 步骤 | 能力 | 说明 |
|------|------|------|
| 1 需求 | 大体需求输入 | 主题/风格/时长/受众等要点 |
| 2 文案 | 自动写文案 | LLM 基于需求生成初稿 |
| 3 去AI味 | 文案人文化 | LLM 改写去除 AI 痕迹，更自然口语化 |
| 4 分镜 | 生成分镜文案 | 拆为镜头：画面描述、台词、时长、运镜 |
| 5 图片 | 生成分镜图片 | 图像生成 API + 风格约束，保持跨镜头一致 |
| 6 首尾帧 | 生成首尾帧视频 | 由图片+分镜文案生成每镜首/尾帧视频，保证连贯 |
| 7 配音+字幕 | 配音与字幕 | TTS 按文案配音；字幕由文案/语音对齐生成并烧录 |

---

## 四、产品形态与技术栈

### 4.1 形态：Web 桌面端（Tauri 2）
采用 **Tauri 2 + React + TypeScript** 桌面外壳，内部跑本地 Web 应用。比 Electron 更轻、启动快、可用 Rust 直接调度本地进程。

### 4.2 三层进程

```
┌──────────────────────────────────────────────────────────┐
│                    桌面外壳 (Tauri 2)                       │
│  ┌────────────────────────────────────────────────────┐  │
│  │              前端 (React + Vite + TS)                │  │
│  │   影片库/剪辑台 · 口播提取/净化 · 创作七步 · 配置中心   │  │
│  └───────────────┬──────────────────────────────────────┘  │
│                  │ Tauri IPC (invoke)                       │
│  ┌───────────────▼─────────────────────────────────────┐  │
│  │   Rust 后端：媒体引擎(FFmpeg) + AI 网关(reqwest 直连)  │  │
│  │   抽音轨/切分/拼接/烧字幕/图生视频/导出 · ASR/LLM/图视频/TTS │  │
│  └───────────────┬─────────────────────────────────────┘  │
│                  │ HTTPS (Bearer, Key 取自系统凭据库)        │
│  ┌───────────────▼─────────────────────────────────────┐  │
│  │   外部大模型 API（可配置）                            │  │
│  │   Agnes(LLM/图/视频) · XiaomiMimo(ASR/TTS)          │  │
│  └─────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────┘
                        本地 SQLite（影片/口播/创作/任务）
```

### 4.3 UI 设计系统（Editorial Design System v3.0）

界面采用**编辑式设计语言**（Editorial Design），灵感来自杂志/期刊排版，强调内容层级、留白和衬线字体温度。抛弃 AI 默认审美（紫色渐变、Inter 字体、emoji 图标、居中卡片）。

**设计 DNA**

| 维度 | 选型 |
|------|------|
| 美学方向 | 编辑式 · 温度 · 期刊感（Brutally minimal + Editorial） |
| 主色 | 赭石 `#b85c38`（替代紫色，更稳重有质感） |
| 辅色 | 茶绿 `#6b7a4b` · 墨蓝 `#2d3e5f` |
| 背景 | 奶油白 `#f5f1ea`（替代纯灰，带纸质温度） |
| 显示字体 | **Fraunces** 衬线（标题/数字，可变字重 + 光学尺寸） |
| 正文字体 | **DM Sans**（现代无衬线） |
| 数字字体 | **Geist Mono**（时间戳、序号、状态标签） |
| 图标 | **Lucide React** 专业图标库（1.6 stroke，与编辑式克制风格统一） |
| 网格 | 8pt 基础 · 12 列内容网格 |
| 圆角 | 偏小（5/7/10/14），避免过分圆润 |

**关键设计元素**

- **侧栏**：每模块带罗马序号 `01/02/03/04` + 中文名 + 英文小标（Film / Spoken / Create / Config），左侧 2px 主题色竖线标记激活态
- **view-header**：每个模块顶部用期刊式大标题（罗马数字 eyebrow `I · FILM` + 衬线大字号 + 斜体强调词）+ 右侧元信息（No. 编号 + 模块名 + 年份）
- **步骤胶囊**：从纵向圆胶囊改为水平连接条，激活态用纯黑背景
- **按钮**：从圆角胶囊改为扁平小圆角 + 黑色填充 + hover 变赭石色
- **花字模板**：从彩色渐变改为编辑式（赭石/茶绿/危险红/纯黑），更克制有重量感
- **Insp­ector 标题**：用 `§` 符号前缀 + 衬线字
- **任务栏**：黑色背景 + 衬线大写 STATUS 标签 + 顶部橙条进度
- **正文文案框**：左侧 3px 赭石竖线 + Fraunces 衬线字体（像排版过的稿子）
- **分镜卡**：左侧 80px 黑色大数字（斜体） + 右侧编辑栏
- **波形条**：黑/赭石/茶绿三种颜色交替（n-th-child 选择器）

**双主题（Light / Editorial Noir）**

通过 `<html data-theme="light|dark">` 切换：
- **Light**：奶油白 `#f5f1ea` 底 + 赭石强调
- **Dark (Editorial Noir)**：深邃黑 `#16140f` 底 + 暖橙 `#d68b6a` 强调（不是普通深色，是带温度的编辑部夜间版）

**响应式断点（4 档）**

| 断点 | 布局 |
|------|------|
| ≥1440px | 三栏 + 加宽内容（max 1320px） |
| 1024-1439px | 三栏默认 |
| 1024-1279px | 双栏 + Inspector 抽屉（可切换） |
| 768-1023px | 侧栏抽屉（汉堡按钮触发）+ 单列 |
| <768px | 顶部精简 + 底部导航 + 单列 |

**图标清单（Lucide）**

Clapperboard（影片）· Mic（口播）· Sparkles（创作）· Settings（设置）· Moon/Sun（主题切换）· PanelRight/PanelRightOpen（Inspector 切换）· Menu/X（移动端汉堡/关闭）· Download（导出）· ChevronRight（导航）

### 4.4 技术栈
| 层 | 选型 | 用途 |
|----|------|------|
| 外壳 | Tauri 2 (Rust) | 桌面封装、进程管理 |
| 前端 | React 18 + Vite + TypeScript | UI、状态、交互 |
| 状态 | React Context（`AppProvider` + 单一 store/actions） | 全局状态，单一数据源 |
| UI | 自研**编辑式设计系统 v3.0**（`src/styles/global.css`）+ **Lucide React** 专业图标 | 温暖赭石/茶绿/奶油白三色系，Fraunces 衬线显示 + DM Sans 正文 + Geist Mono 数字；侧栏序号+中英双语；期刊式 view-header；8pt 网格 |
| 媒体 | FFmpeg（Rust 编排） | 抽音轨/切分/拼接/烧字幕/图生视频/导出 |
| AI 网关 | Rust reqwest 直连云端 API（**无 sidecar**） | 统一调度 ASR/LLM/图视频/TTS，Key 取自系统凭据库 |
| ASR | XiaomiMimo 云端 `mimo-v2.5-asr`（`/v1/chat/completions`，音频 base64 入 `messages[].input_audio`） | 影片导入对齐、口播识别、字幕对齐（当前返回整段文本，无逐句时间轴） |
| LLM | Agnes OpenAI 兼容（`base_url` 可配） | 写文案、去AI味、分镜、口误判断、真实对话 |
| 图/视频生成 | Agnes（`agnes-image-2.1-flash` / `agnes-video-v2.0`） | 分镜图片、首尾帧视频 |
| TTS | XiaomiMimo 云端 `mimo-v2.5-tts`（`/v1/audio/speech`） | 配音、影片导出混音 |
| 存储 | SQLite | 类型/工程/文案/分镜/任务 |

---

## 五、模块与功能映射（M1–M11）

| 原模块 | 归属 | 新职责 |
|--------|------|--------|
| M1 项目资产 | 影片 + 全局 | 影片类型与工程库、资产索引 |
| M2 文案 | 创作(2/3/4) + 口播(提取) | 自动写文案、去AI味、分镜、口播提取 |
| M3 转写 | 口播 | ASR 识别音频→提取文案 |
| M4 智能剪辑分析 | **影片(剪辑) + 口播(纠正)** | 影片：按文案自动切点/删除废片；口播：气口/口误/重复标记与裁剪 |
| M5 时间线 | 影片(剪辑) | 成片多轨预览与精修 |
| M6 字幕花字 | 影片(剪辑) + 口播(花字) + 创作(7) | 字幕生成/烧录、重点高亮、花字模板 |
| M7 二创分镜 | 创作(4/5) | 分镜文案、分镜图片、一致性 |
| M8 首尾帧视频 | 创作(6) | 图片→首尾帧视频 |
| M9 合成导出 | 创作(7) + 影片(剪辑) | 配音+字幕+片段合成导出 |
| M10 API配置 | 全局 | LLM/ASR/图视频/TTS 统一配置 |
| M11 任务队列 | 全局 | 长耗时任务异步/进度/取消 |

---

## 六、核心数据流

```
影片模块：  类型(可扩展) ──▶ 工程库（素材/成片/草稿）
           工程内：导入视频+文案 ──▶ 按文案智能剪辑 ──▶ 时间线精修 ──▶ 字幕/花字 ──▶ 导出
口播模块：  上传视频 ──▶ 抽音轨 ──▶ ASR ──▶ 文案提取 ──▶ 纠正(气口/口误/重复) ──▶ 字幕重点/花字 ──▶ (存入影片/送创作)
创作模块：  需求 ──▶ 文案 ──▶ 去AI味 ──▶ 分镜 ──▶ 图片(一致) ──▶ 首尾帧视频 ──▶ 配音+字幕 ──▶ 合成导出 ──▶ (归档影片)
```

---

## 六·补、界面原型与已落地能力

### 界面原型（设计蓝本）
`preview/prototype.html` 是一个**单文件高保真可点验原型**（原生 HTML + JS，免构建，浏览器直接打开即可预览），用于在设计阶段快速验证三大模块的交互与流程，是后续 React 工程骨架的设计蓝本。其交互流程与本文档三大模块一一对应。

### 工程骨架（P0 已落地）
已搭建可运行的 **Tauri 2 + React 18 + Vite + TypeScript** 实际工程，采用**编辑式设计系统 v3.0**（赭石/茶绿/奶油白三色系、Fraunces 衬线 + DM Sans 正文 + Geist Mono 数字、Lucide 专业图标），四大模块界面与核心交互均以 mock 数据驱动、可点验；`npm run build` 通过类型检查与打包。

### 近期新增交互能力（原型与骨架同步）
- **口播·花字字幕导出剪映工程**：在口播「⑤花字字幕」步骤新增「🎬 导出剪映工程」，生成 `draft_content.json`（视频轨 + 字幕/花字轨）可导入剪映继续精修。
- **创作·去 AI 味可选提示词模板**：去 AI 味步骤提供提示词模板下拉（取自系统配置），按所选模板做口语化改写。
- **创作·分镜文案可编辑**：分镜步骤的画面/台词/时长/运镜均可在界面直接修改，改后实时写入状态。
- **创作·图片参考图分类**：每个镜头可上传参考图，并按 `IP形象 / 场景 / 产品 / 风格 / 材质 / 其他` 分类管理与分组展示，可改类目。
- **创作·配音多声音 + IP 形象**：配音步骤音色可多选，每个声音绑定一个 IP 形象（如"主播小幂"），导出草稿时音频轨按声音 + IP 组织。
- **创作·风格约束卡可选**：分镜步骤的"风格约束卡"支持在 `现实 / 科幻 / 卡通 / 写实 / 动漫 / 水彩` 间切换，切换后实时显示对应色调/字体/运镜约束（保证跨镜头一致）。
- **创作·合成导出剪映工程**：导出步骤新增「🎬 导出剪映工程」，按分镜/字幕/已选声音构造 `draft_content.json` 下载。

> 说明：剪映工程导出在原型/骨架中为模拟实现（构造并下载草稿 JSON）；正式版由 Rust 端生成完整草稿文件夹（draft_content.json + draft_meta_info.json + materials/）并拷贝素材，默认导出 PC 版剪映。

---

## 七、目录结构（实际工程骨架）

```
VideosFlow/
├─ README.md
├─ package.json / vite.config.ts / tsconfig.json   # 前端工程（React18 + Vite + TS）
├─ index.html
├─ docs/technical-solution.md                       # 详细技术方案
├─ preview/prototype.html                           # 高保真界面原型（设计蓝本，可浏览器预览）
├─ src/                                             # 前端 React（编辑式 UI v3.0）
│  ├─ main.tsx                # 入口
│  ├─ App.tsx                 # 应用骨架：侧栏（序号+中英） + 顶栏（breadcrumb） + 内容（view-header） + 右栏（§） + 任务栏（黑底橙条）
│  ├─ styles/global.css       # 编辑式设计系统 v3.0（Fraunces+DM Sans+Geist Mono / 赭石+茶绿 / 8pt网格 / light+dark 双主题 / 4断点响应式）
│  ├─ data/mock.ts            # 全局类型 + 初始 mock 数据 + 常量（风格/分类/花字/步骤…）
│  ├─ state/AppContext.tsx    # 全局状态(store) + 全部 actions（含剪映导出）
│  ├─ lib/jianying.ts         # 剪映 draft_content.json 构造与下载
│  ├─ components/icons.tsx    # Lucide React 图标（Clapperboard / Mic / Sparkles / Settings 等）
│  └─ modules/
│     ├─ Film.tsx             # 影片：类型工程库 + 剪辑台六步
│     ├─ Spoken.tsx           # 口播：上传→识别→纠正→匹配素材→花字字幕（五步）
│     ├─ Creation.tsx         # 创作：需求→文案→去AI味→分镜→图片→首尾帧→配音→导出（八步）
│     └─ Settings.tsx         # 设置：模型API / 提示词 / 其他参数（三步）
├─ src-tauri/                                       # Tauri 2 + Rust
│  ├─ Cargo.toml  tauri.conf.json  build.rs  capabilities/default.json
│  └─ src/{main.rs, lib.rs}    # 应用入口（骨架：装载 WebView + 窗口生命周期）
└─ python-sidecar/            # Python sidecar（历史参考；2026-07-12 已移除编译引用，改为 Rust reqwest 直连云端 API）
```

> 工程骨架已可 `npm run build`（tsc 类型检查 + Vite 打包）通过；四大模块界面与核心交互均以 mock 数据驱动，可点验。

---

## 八、数据库模型（概要）

- `film_categories`：影片类型（id, name, order, editable）— 可扩展
- `film_projects`：影片工程（id, category_id, title, cover, status, tags）
- `edit_timelines`：影片剪辑时间线（id, project_id, tracks JSON, clips JSON）— 成片剪辑
- `spoken_videos`：口播视频（id, path, duration, transcript, script, clean_script）
- `spoken_edits`：口播纠正记录（id, video_id, issue_type, span, action）— 气口/口误/重复
- `creation_projects`：创作工程（id, brief, script, humanized_script）
- `storyboards`：分镜（id, project_id, shots JSON, style_ref JSON）
- `generated_assets`：生成的图片/首尾帧视频（id, shot_id, type, path）
- `voiceovers` / `subtitles`：配音与字幕（按工程/镜头）
- `provider_config` / `tasks`：API 配置、异步任务

---

## 九、关键算法与技术点

1. **基于文案的智能剪辑（影片）**：文案分段 → ASR 时间戳对齐 → 在视频中定位对应片段 → 静音/废片检测 → 自动切点生成粗剪时间线；人工在时间线精修。
2. **口播纠正（口播）**：VAD 静音检测标气口；LLM 逐句判断口误/卡顿；相邻句语义/编辑距离检测重复；标记后一键裁剪或保留。
3. **口播字幕重点 + 花字（口播）**：关键词/重点句高亮；6+ 花字模板（强调/情绪/感叹/关键词描边）预览并烧录。
4. **文案提取**：FFmpeg 抽音轨 → Whisper/云 ASR → 纯文案/带轴文案；可选填充词清理。
5. **去 AI 味**：LLM 以"口语化、去套路、加具体细节"为约束改写；规则层替换高频 AI 词（"首先/其次/综上所述/赋能"等）。
6. **一致性分镜图片**：风格约束卡（角色参考图 + 色调 + 字体 + 运镜）+ IP-Adapter/参考图 + 固定 seed 族，保证跨镜头一致。
7. **首尾帧视频**：以分镜图片作首/尾帧，用图生视频或帧插值生成短片；跨镜头"尾帧→下一镜首帧"做连贯约束。
8. **配音与字幕**：TTS 按文案生成语音；字幕由文案时间轴或语音对齐得到，导出时烧录（ASS/drawtext）。

---

## 十、开发路线图

- **P0 地基**：Tauri 骨架 + SQLite + 三大模块框架 + 全局 API 配置中心。
- **P1 影片模块**：类型可扩展管理 + 工程库 + **基于文案的智能剪辑台（时间线/字幕花字/导出）**。
- **P2 口播模块**：上传 + ASR + 文案提取 + **气口/口误/重复纠正 + 字幕重点/花字**。
- **P3 创作模块（上）**：需求→文案→去AI味→分镜→图片（一致性）。
- **P4 创作模块（下）**：首尾帧视频→配音→字幕→合成导出→归档影片。
- **P5 打磨**：模板、撤销重做、代理渲染、批量、可观测。

---

## 十一、快速开始

### 11.1 三行启动
```bash
# 1) 安装前端依赖并启动开发（Vite 热更新）
npm install
npm run dev

# 2) 以 Tauri 桌面窗口运行（需 Rust 工具链 + WebView2 + VS Build Tools）
npm run tauri:dev

# 3) 生产构建
npm run build          # tsc 类型检查 + Vite 打包到 dist/
npm run tauri:build    # 打包为平台安装包
```

> 仅前端 UI 验证：`npm run dev` 后用浏览器打开 http://localhost:5173 即可，无需 Rust 环境。
> 但**纯浏览器走的是 localStorage 假后端**（参见 §十一·补 已知限制），真实 AI 调用、SQLite、凭据库均要桌面版才能跑。

### 11.2 首次启动 → 填 Key → 自动下载 FFmpeg

桌面版（`npm run tauri:dev`）首次启动按顺序完成四件事：

**(1) 自动初始化数据库 + 种子数据**
应用启动时 `db.rs::init` 幂等建 16 张 SQLite 表（12 张主表 + M3 口播 3 张 + M3+ 凭据表 `provider_secrets`），向 `provider_config` 写入 5 个默认 Provider（Agnes LLM/图/视频 + XiaomiMimo ASR/TTS），向 `film_categories` 写入 5 个默认影片分类（电影/故事/电视剧/动画片/记录片）。SQLite 文件位于 `<应用数据目录>/videosflow.db`（Windows：`%APPDATA%\com.videosflow.app\`）。

**(2) 配置主密钥 `VF_MASTER_KEY`（AES-256-GCM 加密 Key 用）**
桌面版用 **AES-256-GCM** 加密存 API Key（替代 M0-M2 的 keyring 方案——keyring 在本机静默成功但不写 Windows Credential Manager，AES-GCM 更可靠）。

主密钥 32 字节从环境变量 `VF_MASTER_KEY` 读；**未设置时降级到 dev 默认 key**（stderr 警告，生产必须设）：

```powershell
# PowerShell（仅首次启动前设一次；32 字符任取）：
$env:VF_MASTER_KEY = "my-super-secret-32-byte-key!"
[System.Environment]::SetEnvironmentVariable("VF_MASTER_KEY", $env:VF_MASTER_KEY, "User")
```

> **注意**：变更主密钥后，旧 AES-GCM 密文全部失效，需在设置页重新保存 5 个 Provider Key。

**(3) 在「设置」页填 API Key 并测试连接**
打开应用 → 左侧栏点 **设置 → 模型 API** → 在 LLM / 图片 / 视频 / ASR / TTS 五个卡片里依次填 Base URL 和 API Key → 点 **🔌 测试连接**。
- **Key 不落 SQLite 明文**——用主密钥 AES-256-GCM 加密后存 `provider_secrets` 表（仅 `provider_config.has_key` 布尔标记回 UI）。解密需同主密钥。
- 测试连接用 Rust reqwest 直连云端 `Base URL/models`，HTTP 401/403 会给出可读错误，无需 Python sidecar。
- 如 Agnes / XiaomiMimo 暂未提供 API Key，可先跳过，UI 可点验；调用时会按"未保存 KEY"降级提示。

**(4) 导出影片时按需下载 FFmpeg**
FFmpeg **不随包**。导出影片（`film_export` 任务）时若 Rust 端检测不到 ffmpeg 二进制，会自动触发首启下载器：
```bash
# 仅 Windows 示例（需国内可达镜像 + SHA256 校验）：
set VF_FFMPEG_URL=https://your-mirror/ffmpeg-windows.tar.gz
npm run tauri:dev
```
下载到的临时包会通过系统 `tar` 解包到 `<应用数据目录>/ffmpeg/bin/ffmpeg(.exe)`。未配置 `VF_FFMPEG_URL` 时返回可读错误「请放置到 PATH 或 data_dir/ffmpeg/bin，或配置首启下载」，**不阻塞应用启动**。

### 11.3 常见验证步骤
1. `npm run build` → 期望 tsc + vite 零错误，生成 `dist/`。
2. `npm run tauri:dev` → 启动桌面窗口，左侧栏四个模块可切换。
3. **影片 → 导入影片** → 选 mp4 → 进剪辑台 → 「自动切点」→ 「时间线精修」→ 「导出 MP4」（会触发 FFmpeg + 可选 TTS 配音链路）。
4. **设置 → 模型 API → 测试连接** → 任一 Provider 返回 `ok` 即可。
5. 重启应用 → 影片分类、工程、Provider 配置、时间线均不丢。

> 详细技术实现见 [`docs/technical-solution.md`](./docs/technical-solution.md)；开发计划见 [`docs/dev-plan.md`](./docs/dev-plan.md)；M2 影片设计见 [`docs/m2-film-design.md`](./docs/m2-film-design.md)；M3 口播设计见 [`docs/m3-spoken-design.md`](./docs/m3-spoken-design.md)；M4 创作上设计见 [`docs/m4-creation-design.md`](./docs/m4-creation-design.md)；界面原型见 [`preview/prototype.html`](./preview/prototype.html)。

---

## 十一·补、M0–M2 实现进展（2026-07-12）

### 里程碑状态
- **M0 / M1 / M2 全部完成并 push 到 `origin/main`**；Rust 端 `cargo build` 与前端 `npm run build` 均通过；桌面端 `npm run tauri:dev` 已可运行并接受运行时验收。
- 本机 Rust 工具链就绪：rustup 1.97.0，`x86_64-pc-windows-msvc`，MSVC 链接器 + Windows SDK 齐全。

### 近期已实现 / 修复
1. **影片模块（M2）运行时验收修复**
   - 侧边栏 brand/footer 与 topbar/taskbar 垂直对齐；影片首页新增「导入影片」入口；接入 `tauri-plugin-dialog` 实现真实文件选择导入。
   - 影片分类 seed：首次启动向 `film_categories` 写入 电影/故事/电视剧/动画片/记录片（仅空表时），修复桌面版首次进入影片界面分类为空的问题。
2. **设置中心 · 模型 API**
   - 语音识别（ASR）改为 **XiaomiMimo**（`https://api.xiaomimimo.com/v1`，`mimo-v2.5-asr`）。
   - 「测试连接」改为 **Rust reqwest 直连**探测（`provider_test`），不再依赖 Python sidecar；保存接口参数扁平化（`provider_upsert`），修复 `missing required key p` 报错；测试可传未保存 Key 并显示真实错误。
   - **API Key 已保存提示**：保存成功后文本框下方按 `hasKey` 标记显示「已保存 API Key / 如需更换请填写新 KEY」等提示（不回显明文）。
3. **凭据库回读解耦（关键修复）**
   - 现象：桌面版保存后一直显示「尚未保存 API Key」，而网页版（localhost:5173）显示「已保存」——网页版是 localStorage 假后端假象，非真实保存。
   - 修复：`provider_config` 新增 `has_key` 布尔列；`provider_key_set` 写密钥成功后置位；`provider_list` 以 DB 标记为权威，仅 false 时回退凭据库。**密钥本体仍只存系统凭据库，SQLite 仅存布尔标记**（安全红线不动）。
4. **AI 调用链路全面改为 Rust reqwest 直连（彻底移除 Python sidecar）**
   - 真实对话（`chat`）：`run_chat` 直连 Agnes `/chat/completions`，进度通道回传回答。
   - 影片 ASR：新增 `transcribe_asr` 直连 XiaomiMimo `/v1/chat/completions`（音频 wav base64 入 `messages[].input_audio`），修复此前恒降级。
   - 影片 TTS：新增 `synthesize_tts` 直连 XiaomiMimo `/audio/speech`，音频字节写本地文件后由 `mux_cmd` 混音，修复此前 TTS 混音恒不生效。
   - `lib.rs` 删除 `mod python` + `spawn_sidecar`；`python` 模块不再被编译引用（目录保留作参考）。所有云调用均经 `cred` 取 Key 直连，无任何 sidecar 进程。

### 十二·续、M3–M4 实现进展（2026-07-13）
- **M3 口播模块**：13 个 Tauri 命令 + 5 个任务类型全链路真实化。
  - 上传（tauri-plugin-dialog 真实文件选择）→ 抽音轨 → XiaomiMimo ASR（降级整段文本）→ 文案提取（标点切 + 去填充词）。
  - 检测任务：FFmpeg silencedetect (gap) + Rust 编辑距离 (repeat) + Agnes LLM (mistake 失败降级静默) → 写 spoken_edits；前 2 个 issue 类目可零 LLM 成本跑通。
  - 采纳/忽略（单条 + 全部）+ 一键应用生成 `cleanScript`（不破坏 transcript）。
  - 关键词：Agnes LLM（prompt=keywords）→ 失败降级 Rust 端 TF-IDF 字符 2-3-gram 兜底。
  - 素材：4 类（image/bgm/sfx/clip）真实落盘到 `data_dir/spoken_assets/{videoId}/`；类型嗅探按扩展名。
  - 花字烧录：复用 M2 `build_ass` + `burn_ass_cmd`。
  - 干净片段导出：基于原片裁剪（accepted edits 切 + concat + 可选烧录 + 软编码）。
  - 剪映工程导出：前端构造 `draft_content.json`（视频轨 + 字幕/花字轨）+ 下载。
  - 设计/决策见 [`docs/m3-spoken-design.md`](./docs/m3-spoken-design.md)。
- **M4 创作上**：11 个 Tauri 命令 + 4 个任务类型全链路真实化。
  - 需求 → 自动写文案（Agnes `script` 提示词）→ 去 AI 味（Agnes `humanize`）→ 生成分镜（Agnes `storyboard` 严格 JSON）→ 单镜图片（Agnes `/images/generations`，base64 写本地）。
  - 风格约束卡 6 套（现实/科幻/卡通/写实/动漫/水彩）沿用 M3 `stylePresets`。
  - 一致性抽检：图片大小粗筛（5KB-5MB）+ UI 横向对比；图片失败任务 failed + UI 允许重试。
  - 设计/决策见 [`docs/m4-creation-design.md`](./docs/m4-creation-design.md)。
- **凭据库方案切换 AES-256-GCM（关键修复）**
  - 现象：M0-M2 用 `keyring` 写 Windows Credential Manager，桌面版保存后一直显示「尚未保存」，而 `cmdkey /list` 和 Win32 CredEnumerateW 查不到 keyring 写入的条目——keyring 3.6 在本机静默成功但不实际写凭据。
  - 修复：移除 `keyring` 依赖，**AES-256-GCM 加密存 SQLite `provider_secrets(kind, ciphertext, updated_at)` 表**。主密钥 32 字节从环境变量 `VF_MASTER_KEY` 读；未设置时降级到 dev 默认 key（stderr 警告）。详情见 §11.2 (2)。
  - 链路验证 · 真实 Agnes /v1/chat：**已通过**（保存后立即可读到 51 字符 Key 并完成调用）。
- **清理 dead-code 警告**：6 个 M2 警告清零（删除 `parse_asr_data`、`_port` 加下划线前缀、`version` 加 `#[allow]`、`sniff_asset_kind` 备用保留）。
- **Rust 端**：30 个 Tauri 命令、12 个任务类型，`cargo build` 零错误零警告（移除 keyring 后 4.85s 增量）。
- **前端**：`npm run build` 1.15s 零错误，JS 256 kB。

### 已知限制（受真实 API 能力边界，非 bug）
- **XiaomiMimo ASR 仅返回整段文本、无逐句时间轴**：影片「导入对齐」目前退化为「拿到整段 ASR 文本」（单 segment）。精准逐句对齐需未来接入带时间戳的 ASR 或后处理切分。
- **网页版 vs 桌面版是两套后端**：纯浏览器 `localhost:5173` 走 localStorage 假后端，很多「已保存 / 连接成功」是模拟假象，不代表真实后端；桌面版才是真实 Rust+SQLite+凭据库。凡两端表现不一致，先怀疑假后端兜底。
- FFmpeg 不随包，首次启动按平台下载到缓存（需国内可达镜像 + SHA256 校验）。

### 安全红线
- API Key 仅在「设置」页由用户填写，运行时存**系统凭据库**（Windows Credential Manager / macOS Keychain / Linux libsecret），绝不硬编码、绝不落 SQLite 明文、绝不进 git。

---

## 十二、风险与对策

| 风险 | 对策 |
|------|------|
| 多类生成 API 不稳定/贵 | 全部 Provider 可插拔 + 本地 Whisper/SD 兜底；任务可重试、断点续跑 |
| 按文案剪辑对齐不准 | 文案分段 + ASR 时间戳双路对齐；粗剪后支持人工时间线精修 |
| 口播误删关键内容 | 纠正建议默认"采纳/忽略"双选，标记可回溯，不自动破坏原片 |
| 首尾帧/图片风格不一致 | 风格约束卡 + 参考图 + seed 约束，生成后自动抽检对比 |
| 配音与口型/字幕不同步 | 字幕由同一文案时间轴生成，导出统一对齐 |
| 桌面大文件性能 | Rust 媒体引擎 + FFmpeg 硬件加速 + 懒加载缩略图 |

---

_由 大幂幂 规划整理 · 2026-07-10（三大模块优化重组版 · 同日落地 Tauri2 + React + TS 工程骨架，UI 经两轮迭代至 Editorial Design System v3.0 编辑式设计）；2026-07-12 补充 M0–M2 实现进展与架构更正（移除 Python sidecar、AI 调用改为 Rust reqwest 直连云端 API、凭据库回读解耦）。_
