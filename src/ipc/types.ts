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
