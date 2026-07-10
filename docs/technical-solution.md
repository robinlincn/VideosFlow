# VideosFlow 技术开发解决方案（详细版 · 三大模块优化重组版）

> 配套文档：[`../README.md`](../README.md)、界面原型 [`../preview/prototype.html`](../preview/prototype.html)。
> 本文在规划基础上，给出可落地的工程实现细节：**三大模块**（影片 / 口播 / 创作视频）的接口、AI 引擎协议、媒体引擎命令、数据模型、关键算法、分阶段实施计划与验收标准。
> **演进说明**：本方案的三大模块是基于用户最初的三大原始需求优化重组而来——
> - 原始需求 A（现成视频按文案剪辑）→ **影片模块（剪辑功能）**
> - 原始需求 B（口播净化：气口/口误/重复纠正 + 字幕重点高亮 + 花字）→ **口播模块（保留纠正/花字）**
> - 原始需求 C（文案二创 → 分镜 → 首尾帧一致性生成视频）→ **创作视频模块（七步）**

---

## 1. 总体架构

### 1.1 进程模型（三层）
桌面应用由 Tauri 启动并管理三个进程：
1. **前端（WebView）**：React 应用，三大模块 UI 与交互，经 Tauri `invoke` 调 Rust，经 WebSocket 订阅长任务进度。
2. **Rust 媒体/编排进程**：文件系统、SQLite、FFmpeg（抽音轨/切分/拼接/烧字幕/图生视频/导出）、守护并调用 Python AI 引擎、统一任务队列。
3. **Python AI 引擎（sidecar，FastAPI）**：封装 ASR / LLM / 图视频生成 / TTS，对上游屏蔽厂商差异，对下游暴露统一 REST。

```
WebView ──Tauri IPC──▶ Rust 编排 ──HTTP──▶ Python AI 引擎 ──▶ 外部大模型 API
   ▲                      │  ▲
   │  进度 WebSocket       │  └──FFmpeg──▶ 本地媒体文件
   └──────────────────────┘
```

### 1.2 为什么这样分
- 媒体处理放 Rust/FFmpeg：大文件、长耗时、需硬件加速，稳定且不阻塞 UI。
- AI 放 Python：Whisper/diffusers/TTS 生态成熟；sidecar 隔离，崩了不影响主程序。
- **三大模块共享同一套 Provider 协议**：LLM/ASR/图·视频/TTS 全部可插拔，密钥存系统凭据。

---

## 2. 技术栈与版本

| 层 | 技术 | 版本/说明 |
|----|------|-----------|
| 桌面 | Tauri 2 (Rust 1.78+) | 外壳 + 媒体编排 |
| 前端 | React 18 + Vite + TS + Zustand | UI 与状态 |
| UI | Ant Design 5 + 自绘组件 | 专业控件 |
| 媒体 | FFmpeg 7.x | libx264/265、libass、fontconfig |
| 编排 | Rust tokio + reqwest + sqlx | 异步 + SQLite |
| AI | Python 3.11 + FastAPI + httpx | sidecar |
| ASR | faster-whisper（本地）/ 云 ASR 可插拔 | 口播识别、字幕对齐、剪辑对齐 |
| LLM | OpenAI 兼容 SDK（base_url 可配） | 写文案、去AI味、分镜、口误判断、关键词抽取 |
| 图/视频生成 | diffusers(SDXL+IP-Adapter)/云 API 可插拔 | 分镜图片、首尾帧视频 |
| TTS | CosyVoice / Edge / 云 TTS 可插拔 | 配音 |
| 存储 | SQLite | 单文件工程库 |

---

## 3. 模块详细设计

### 3.0 三大模块总览
| 模块 | 入口 | 关键能力 |
|------|------|----------|
| **影片** | 类型树 + 工程库 + 剪辑台 | 类型可扩展（电影/故事/电视剧/动画片/记录片…）；工程归档、检索；**基于文案的智能剪辑 + 时间线精修 + 字幕花字** |
| **口播** | 上传/列表/详情 | 上传视频 → 抽音轨 → ASR → 文案提取；**保留气口/口误/重复纠正 + 字幕重点高亮/花字** |
| **创作视频** | 七步向导 | 需求→文案→去AI味→分镜→图片→首尾帧视频→配音+字幕→导出 |

### 3.1 影片模块（类型化工程库 + 成片剪辑）
- `film_categories`（可扩展）：`{id, name, order, editable}`。支持新增/重命名/删除/排序；删除类型时归并或迁移其下工程。
- `film_projects`：`{id, category_id, title, cover, status, tags, created_at}`。
- `edit_timelines`（剪辑台）：`{id, project_id, tracks JSON, clips JSON, updated_at}` —— 承载基于文案剪辑后的时间线。
- **剪辑能力（承接原始需求 A）**：
  1. 导入素材视频 + 配套文案到同一工程。
  2. 基于文案智能剪辑：文案分段 → 与视频音轨 ASR 时间戳对齐 → 在视频中定位对应片段 → 静音/废片检测 → 自动切点生成粗剪时间线。
  3. 时间线精修：多轨（视频/音频/字幕/生成）拖拽、裁剪、转场、音量。
  4. 字幕 & 花字：成片字幕生成/烧录，重点句高亮 + 花字模板。
  5. 导出：合成 MP4（硬件加速可选）。

### 3.2 口播模块（识别音频 + 提取文案 + 纠正/花字）
- 流程：上传视频 → FFmpeg 抽音轨（16k 单声道 wav）→ AI 引擎 ASR → 逐句 `{id,start,end,text,confidence}`。
- 文案提取：输出纯文案（去时间戳）与"带轴文案"两种；可选填充词清理（嗯/啊/那个…）。
- **纠正剪辑（保留原始需求 B）**：
  - `spoken_edits`：`{id, video_id, issue_type, start, end, text, suggestion, accepted}` —— issue_type ∈ {gap(气口), mistake(口误/卡顿), repeat(重复/啰嗦)}。
  - 检测：VAD 静音标气口；LLM 逐句判断口误/卡顿；相邻句语义/编辑距离检测重复。
  - 交互：建议默认"采纳/忽略"双选，标记可回溯，**不自动破坏原片**；采纳后产出 `clean_script` 与干净口播片段。
- **字幕重点标注 + 花字（保留原始需求 B）**：
  - LLM 抽取关键词/重点句 → 字幕轨道重点高亮。
  - 6+ 花字模板（强调/情绪/感叹/关键词描边…）预览并烧录（ASS）。
- 出口：净化后的文案与成片可"复制到创作视频"作为初始需求/脚本，或存为影片工程脚本。

### 3.3 创作视频模块（七步向导）
状态机：`brief → script → humanized → storyboard → images → frames → voice_sub → exported`。

1. **需求**：表单（主题/风格/时长/受众/平台）+ 自由文本。
2. **自动写文案**：LLM 基于需求生成初稿 `script`。
3. **去 AI 味**：LLM 以"口语化、去套路、加具体细节、避免空话"约束改写 → `humanized_script`；规则层替换高频 AI 词（"首先/其次/综上所述/赋能/至关重要"等）。
4. **分镜文案**：LLM 将定稿拆为镜头数组：
   ```ts
   { index, shotDesc, dialogue, durationSec, camera, styleRef }
   ```
   `styleRef` 全局共享（角色参考图+色调+字体+运镜）以保证一致性。
5. **分镜图片**：对每镜调用图像生成 API，prompt 注入 `styleRef` + IP-Adapter/参考图 + 固定 seed 族 → 图片；生成后做跨镜头相似度抽检。
6. **首尾帧视频**：以图片作首/尾帧，用图生视频或帧插值生成每镜短片；约束"尾帧→下一镜首帧"连贯。
7. **配音 + 字幕**：TTS 按 `dialogue`/定稿生成语音（可多音色）；字幕由同一文案时间轴对齐生成，导出时烧录（ASS/drawtext）。
8. **合成导出**：拼接 图片视频片段 + 配音 + 字幕 → MP4；归档到影片模块。

---

## 4. AI 引擎统一协议（FastAPI）

统一信封 `{ ok, data, task_id, progress? }`。

| 端点 | 方法 | 说明 |
|------|------|------|
| `/asr/transcribe` | POST | 视频/音频 → 逐句字幕（口播 + 影片剪辑对齐） |
| `/script/write` | POST | 需求 → 初稿文案（创作 step2） |
| `/script/humanize` | POST | 文案 → 去AI味（创作 step3） |
| `/script/storyboard` | POST | 定稿 → 分镜数组（创作 step4） |
| `/spoken/detect` | POST | 转写 → 气口/口误/重复建议（口播纠正） |
| `/spoken/keywords` | POST | 文案 → 重点/关键词（口播花字高亮） |
| `/gen/image` | POST | 分镜+styleRef → 图片（创作 step5） |
| `/gen/frames` | POST | 图片+分镜 → 首尾帧视频（创作 step6） |
| `/tts/voice` | POST | 文案 → 配音音频（创作 step7 / 口播配音） |
| `/tasks/{id}` | GET | 任务进度/结果 |

**Provider 抽象**：`providers/` 下每厂商实现 `LLMProvider` / `ASRProvider` / `ImageProvider` / `VideoProvider` / `TTSProvider` 接口，配置从全局 API 配置注入；新增厂商只加适配文件。

---

## 5. 媒体引擎（FFmpeg）命令要点

- 抽音轨：`ffmpeg -i in.mp4 -vn -ac 1 -ar 16000 audio.wav`
- 静音检测（气口）：`ffmpeg -i audio.wav -af silencedetect=noise=-35dB:d=0.3 -f null -`
- 精确切割：`ffmpeg -ss S -to E -i in.mp4 -c copy seg.mp4`（不准时 `-i` 在前重编码）
- 拼接：`ffmpeg -f concat -safe 0 -i list.txt -c copy out.mp4`
- 图生视频（首尾帧）：`ffmpeg -loop 1 -i first.png -loop 1 -i last.png -filter_complex "..." -t D out.mp4` 或调用生成模型 API
- 配音混音：`ffmpeg -i video.mp4 -i voice.wav -c:v copy -c:a aac -shortest out.mp4`
- 烧字幕/花字：`ffmpeg -i in.mp4 -vf "ass=sub.ass" out.mp4`
- 导出：`ffmpeg -i ... -c:v libx264 -crf 20 -c:a aac out.mp4`（硬件加速可选 `-c:v h264_nvenc`）

---

## 6. 关键算法

### 6.0 基于文案的智能剪辑（影片）
```
script_segs = segment_by_punctuation(script)     # 按标点把文案分段
asr = ASR(video_audio)                           # [{start,end,text}]
alignment = align_text(script_segs, asr)         # 文本对齐 → 每段起止时间
timeline = []
for seg in script_segs:
    span = alignment[seg]                         # (start, end)
    if is_silence_or_junk(video, span): continue # 丢弃静音/废片
    timeline.append(clip(span))
# 粗剪时间线 → editor 中人工精修（拖拽/裁剪/转场）
```

### 6.1 口播纠正（气口/口误/重复）
```
audio = ffmpeg_extract(video)
asr = ASR(audio)                                 # 带时间戳句
gaps   = VAD(audio, min_dur=0.3)                 # 气口（静音段）
issues = LLM(asr, task="detect_mistakes")        # 口误/卡顿 [{span, suggestion}]
dups   = detect_repeat(asr, edit_distance)       # 重复/啰嗦
suggestions = gaps + issues + dups               # 全部可采纳/忽略
clean_script = apply_accepted(suggestions)       # 干净文案 + 干净片段
```

### 6.2 口播字幕重点 + 花字
```
keywords = LLM(script, task="extract_emphasis")  # 重点/关键词
subtitle = render(script, highlight=keywords)    # 重点高亮字幕轨道
flower   = apply_template(subtitle, templateId)  # 6+ 花字模板
burn(ass=flower) -> video                         # 烧录
```

### 6.3 文案提取（口播）
```
audio = ffmpeg_extract(in.mp4)
sentences = ASR(audio)            # [{start,end,text}]
script_plain = join(text)         # 纯文案
script_typed = sentences          # 带轴文案
optional: clean_fillers(script_typed)   # 去 嗯/啊/那个
```

### 6.4 去 AI 味
```
prompt: 将文案改写为自然口语，去掉 AI 套话与空泛表达，
        增加具体细节与停顿感，保持原意。
rules: 替换 首先/其次/综上所述/赋能/至关重要/毋庸置疑 等高频词
return humanized_script
```

### 6.5 一致性分镜图片
```
styleRef = load(global_style)     # 角色参考图+palette+font+mood
for shot in storyboard:
  img = image_gen(prompt=shot.shotDesc + styleRef.prompt,
                  ref_image=styleRef.charRef,   # IP-Adapter
                  seed=styleRef.seed + shot.index)
  verify(img, prev_img) by CLIP-sim > threshold
```

### 6.6 首尾帧视频与连贯
```
for shot in storyboard:
  first, last = shot.image_first, shot.image_last
  clip = video_gen(first, last, shot.durationSec)   # 图生视频/帧插值
# 跨镜头：约束 last(shot i) ≈ first(shot i+1) 视觉流
```

### 6.7 配音与字幕对齐
```
voice = TTS(dialogue_per_shot, voice_id)          # 每段配音
subtitles = align(dialogue, voice_timestamps)     # 由文案时间轴生成
export: mux(video_clips, voice, burn(subtitles))
```

---

## 7. 数据模型（SQLite 详表）

```sql
CREATE TABLE film_categories(
  id TEXT PRIMARY KEY, name TEXT, "order" INT, editable INT DEFAULT 1);

CREATE TABLE film_projects(
  id TEXT PRIMARY KEY, category_id TEXT, title TEXT, cover TEXT,
  status TEXT, tags TEXT, created_at INTEGER);

CREATE TABLE edit_timelines(
  id TEXT PRIMARY KEY, project_id TEXT, tracks TEXT,   -- 多轨 JSON
  clips TEXT, updated_at INTEGER);                      -- 片段 JSON

CREATE TABLE spoken_videos(
  id TEXT PRIMARY KEY, path TEXT, duration REAL,
  transcript TEXT, script TEXT, clean_script TEXT, created_at INTEGER);

CREATE TABLE spoken_edits(
  id TEXT PRIMARY KEY, video_id TEXT, issue_type TEXT,  -- gap/mistake/repeat
  start REAL, end REAL, text TEXT, suggestion TEXT, accepted INT DEFAULT 0);

CREATE TABLE creation_projects(
  id TEXT PRIMARY KEY, brief TEXT, script TEXT,
  humanized_script TEXT, status TEXT, created_at INTEGER);

CREATE TABLE storyboards(
  id TEXT PRIMARY KEY, project_id TEXT, shots TEXT, style_ref TEXT);

CREATE TABLE generated_assets(
  id TEXT PRIMARY KEY, shot_id TEXT, kind TEXT,   -- image / frame_video
  path TEXT, created_at INTEGER);

CREATE TABLE voiceovers(
  id TEXT PRIMARY KEY, project_id TEXT, shot_id TEXT,
  voice_id TEXT, path TEXT);

CREATE TABLE subtitles(
  id TEXT PRIMARY KEY, project_id TEXT, start REAL, end REAL,
  text TEXT, style_id TEXT);

CREATE TABLE provider_config(
  id TEXT PRIMARY KEY, kind TEXT,   -- llm/asr/image/video/tts
  provider TEXT, base_url TEXT, api_key TEXT, model TEXT, extra TEXT);

CREATE TABLE tasks(
  id TEXT PRIMARY KEY, project_id TEXT, type TEXT,
  status TEXT, progress REAL, log TEXT, created_at INTEGER);
```

---

## 8. 前端 ⇄ 后端 IPC 协议（节选）

```
-- 影片：类型/工程/剪辑 --
invoke('film_category_create', {name}) -> category
invoke('film_project_create', {categoryId, title}) -> project
invoke('film_edit_open', {projectId}) -> timeline
invoke('film_edit_smart_cut', {projectId, script}) -> taskId   # 基于文案智能剪辑
invoke('film_edit_timeline_save', {projectId, tracks, clips}) -> ok
invoke('film_export', {projectId, params}) -> taskId

-- 口播：上传/ASR/提取/纠正/花字 --
invoke('spoken_upload', {path}) -> video
invoke('asr_transcribe', {videoId}) -> taskId
invoke('spoken_extract', {videoId}) -> script
invoke('spoken_detect', {videoId}) -> taskId                  # 气口/口误/重复
invoke('spoken_apply_edits', {videoId, acceptedIds}) -> cleanScript
invoke('spoken_flower_text', {videoId, templateId}) -> taskId # 字幕重点/花字预览烧录

-- 创作：七步 --
invoke('creation_create', {brief}) -> project
invoke('script_write', {projectId}) -> taskId
invoke('script_humanize', {projectId}) -> taskId
invoke('storyboard_gen', {projectId}) -> taskId
invoke('image_gen', {projectId, shotIndex}) -> taskId
invoke('frames_gen', {projectId, shotIndex}) -> taskId
invoke('tts_gen', {projectId, shotIndex, voiceId}) -> taskId
invoke('export', {projectId, params}) -> taskId

ws://127.0.0.1:<port>/progress -> {taskId, progress, status, payload}
```

---

## 9. 分阶段实施计划与验收

| 阶段 | 交付 | 验收 |
|------|------|------|
| P0 地基 | Tauri 骨架 + SQLite + 三模块框架 + 全局 API 配置 | 三模块可切换、能存配置 |
| P1 影片 | 类型可扩展 + 工程库 + **基于文案剪辑台（时间线/字幕花字/导出）** | 增删改类型、建工程、导入视频+文案出粗剪、精修导出 |
| P2 口播 | 上传 + ASR + 文案提取 + **气口/口误/重复纠正 + 字幕重点/花字** | 出文案、标注并采纳纠正、花字预览烧录 |
| P3 创作(上) | 需求→文案→去AI味→分镜→图片(一致) | 走完前五步、图片一致 |
| P4 创作(下) | 首尾帧视频→配音→字幕→合成导出→归档 | 出带配音字幕成片并归档 |
| P5 打磨 | 模板/撤销/代理渲染/批量/可观测 | 性能与体验达标 |

---

## 10. 扩展性设计
- **Provider 即插即用**：新增 LLM/ASR/图/视频/TTS 厂商 = 在 `providers/` 加一个实现 + 配置加选项。
- **影片类型可扩展**：`film_categories` 数据驱动，UI 动态渲染，无需改代码即可加类型。
- **七步向导可固化**：把"需求→…→导出"存为工作流模板，一键重跑同类任务。
- **本地优先**：默认本地 Whisper/SD/TTS 可离线；云 API 仅作增强。
- **剪辑可回溯**：口播纠正与影片粗剪均保留原片与标记，修正不破坏源素材。

---

_由 大幂幂 整理 · 2026-07-10 · 三大模块优化重组版 · 与 README.md / prototype.html 配套_
