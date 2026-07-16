// VideosFlow 前端 IPC 类型定义
// M0：ProviderRow / ProgressMsg。M2：影片模块领域类型（与 Rust db.rs 序列化结构对齐）。

/** 后端 provider_config 表行（密钥不在此，hasKey 表示凭据库是否有 key）。 */
export interface ProviderRow {
  id: string;
  kind: string;
  name: string;
  provider: string;
  baseUrl: string;
  model: string;
  enabled: boolean;
  hasKey: boolean;
  /** 运行模式：'cloud'（云端 API）或 'local'（本地推理，如 faster-whisper/Whisper） */
  mode?: string;
}

/** 任务进度消息（经 Tauri Channel 实时推送）。 */
export interface ProgressMsg {
  taskId: string;
  progress: number;
  status: string;
  message?: string;
  payload?: unknown;
}

// ---------------------------------------------------------------------------
// M2：影片模块领域类型
// ---------------------------------------------------------------------------

/** film_categories 行（editable: 0|1）。 */
export interface FilmCategory {
  id: string;
  name: string;
  order: number;
  editable: number;
}

/** film_projects 行。 */
export interface FilmProject {
  id: string;
  categoryId: string;
  title: string;
  cover: string | null;
  status: string;
  tags: string;
  createdAt: number;
}

/** ASR 单句结果（与 §3.4 对齐）。 */
export interface AsrSegment {
  start: number;
  end: number;
  text: string;
  confidence: number;
}

/** 文案分段。 */
export interface ScriptSeg {
  index: number;
  text: string;
}

/** 时间线 Clip（edit_timelines.clips 扁平化）。 */
export interface TimelineClip {
  id: string;
  source: string; // material|voice|subtitle|gen
  timelineStart: number;
  timelineEnd: number;
  srcStart: number;
  srcEnd: number;
  label: string;
  text: string;
  flower: string; // 花字模板 id
  transition: string; // none|fade|dissolve
}

/** 时间线轨道（video|audio|subtitle|gen）。 */
export interface TimelineTrack {
  id: string;
  kind: string;
  name: string;
  volume: number; // 0-1
  muted: boolean;
  clips: TimelineClip[];
}

/** 时间线信封（edit_timelines.tracks = JSON.stringify(TimelineEnvelope)）。 */
export interface TimelineEnvelope {
  asr: AsrSegment[];
  scriptSegs: ScriptSeg[];
  alignment: Record<string, [number, number]>;
  tracks: TimelineTrack[];
  videoPath: string;
}

/** edit_timelines 行（tracks/clips 为 JSON 字符串）。 */
export interface TimelineRow {
  id: string;
  projectId: string;
  tracks: string;
  clips: string;
  updatedAt: number;
}

/** 花字模板（固化 6 套，M2 不支持自定义）。 */
export interface FlowerTemplate {
  id: string;
  name: string;
  kind: string;
  assStyle: {
    Name: string;
    FontName: string;
    FontSize: number;
    PrimaryColour: string;
    BackColour: string;
    Outline: number;
    Shadow: number;
    Bold: 0 | 1;
    BorderStyle: 0 | 1 | 3;
    Alignment: number;
    MarginV: number;
    MarginL: number;
  };
}

/** 导出参数（film_export 任务 payload）。 */
export interface FilmExportOptions {
  hw: boolean;
  resolution: string;
  burnSub: boolean;
  mixVoice: boolean;
  voiceMix: number;
  script: string;
}

// ---------------------------------------------------------------------------
// M3：口播模块领域类型
// ---------------------------------------------------------------------------

/** 口播视频行（spoken_videos）。 */
export interface SpokenVideo {
  id: string;
  name: string;
  path: string;
  duration: number;
  /** JSON 字符串，解析后为 AsrSegment[]（XiaomiMimo 当前仅返回单段无时间轴） */
  transcript: string;
  /** 提取的纯文案（按标点切 + 去填充词） */
  script: string;
  /** 干净文案（采纳所有 accepted=1 edits 后生成） */
  cleanScript: string;
  createdAt: number;
}

export type SpokenIssueKind = 'gap' | 'mistake' | 'repeat';

/** 单条纠正建议（spoken_edits）。 */
export interface SpokenEdit {
  id: string;
  videoId: string;
  issueType: SpokenIssueKind;
  start: number;
  end: number;
  text: string;
  suggestion: string;
  /** 0 待定 / 1 采纳 / -1 忽略 */
  accepted: 0 | 1 | -1;
}

/** 素材库（spoken_assets）。 */
export interface SpokenAsset {
  id: string;
  videoId: string;
  name: string;
  type: 'image' | 'bgm' | 'sfx' | 'clip';
  path: string;
}

/** 关键词（spoken_keywords）。 */
export interface SpokenKeyword {
  id: string;
  videoId: string;
  text: string;
  weight: number;
}

/** 句 ↔ 关键词 ↔ 素材 匹配（spoken_matches）。 */
export interface SpokenMatch {
  id: string;
  videoId: string;
  segStart: number;
  segEnd: number;
  segText: string;
  keyword: string;
  assetId: string;
  applied: boolean;
}

/** 干净片段导出选项。 */
export interface SpokenExportOptions {
  burnFlower: boolean;
  flower: string;
}

// ---------------------------------------------------------------------------
// M2.5：影片解说生成
// ---------------------------------------------------------------------------

/** M2.5 影片解说生成 payload。 */
export interface FilmScriptGenOptions {
  videoPath: string;
  title: string;
  style: string;        // movie / series / variety / anime / doc / horror / funny / emotion / knowledge / sarcastic-suspense ...
  styleName?: string;   // 风格展示名（如「毒舌悬疑」），用于提示词
  language: string;     // zh / en / ja
  duration: number;     // 秒
  hint: string;         // 辅助提示
  mode?: 'ai' | 'custom' | 'imitate'; // AI 帮我写 / 我有文案 / AI 仿写
  view?: 'first' | 'third';           // 解说视角：第一/第三人称
  model?: string;       // 解说模型：default / god（上帝视角）
  analysisMode?: number;// 穿插原片密度 0-1
  voiceId?: string;     // 语音克隆：知性女声 / 磁性男声 / 温暖男声
  subtitleStyle?: string; // 字幕样式
  analysis?: string;    // M2.6：影片视频分析结果（markdown），与所选参数共同驱动解说文案生成
  rolePrompt?: string;  // 设置中「角色设定」提示词模板内容，作为解说生成的角色身份/口吻基准
}

// ---------------------------------------------------------------------------
// M4：创作模块领域类型
// ---------------------------------------------------------------------------

export type CreationStatus = 'draft' | 'writing' | 'humanized' | 'storyboard' | 'images' | 'done';

/** 创作工程（creation_projects）。 */
export interface CreationProject {
  id: string;
  brief: string;
  script: string;
  humanizedScript: string;
  status: CreationStatus;
  createdAt: number;
}

/** 单个分镜镜头（M2.5 增 start/end/style 字段；M2 已 index/desc/dialogue/dur/cam）。 */
export interface Shot {
  index: number;
  desc: string;
  dialogue: string;
  dur: number;
  cam: string;
  start?: number;
  end?: number;
  style?: string;
}

/** 分镜（storyboards）。 */
export interface Storyboard {
  id: string;
  projectId: string;
  shots: Shot[];
  styleRef: string;
  updatedAt: number;
}

/** 生成的素材（generated_assets）。 */
export interface GeneratedAsset {
  id: string;
  projectId: string;
  shotId: number;
  kind: 'image';
  path: string;
  createdAt: number;
}
