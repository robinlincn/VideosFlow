// VideosFlow 前端 IPC 高层封装（Provider 配置 + 连接测试 + 任务提交 + M2 影片模块）
import { invoke, createChannel } from './client';
import type {
  ProviderRow,
  ProgressMsg,
  FilmCategory,
  FilmProject,
  TimelineRow,
  TimelineEnvelope,
  TimelineClip,
  FlowerTemplate,
  FilmExportOptions,
  FilmScriptGenOptions,
  SpokenVideo,
  SpokenEdit,
  SpokenAsset,
  SpokenKeyword,
  SpokenMatch,
  SpokenExportOptions,
  CreationProject,
  Shot,
  Storyboard,
  GeneratedAsset,
} from './types';

export type {
  ProviderRow,
  ProgressMsg,
  FilmCategory,
  FilmProject,
  TimelineRow,
  TimelineEnvelope,
  TimelineClip,
  FlowerTemplate,
  FilmExportOptions,
  FilmScriptGenOptions,
  SpokenVideo,
  SpokenEdit,
  SpokenAsset,
  SpokenKeyword,
  SpokenMatch,
  SpokenExportOptions,
  CreationProject,
  Shot,
  Storyboard,
  GeneratedAsset,
} from './types';

/** 读取全部 Provider 配置（含 hasKey 标记）。 */
export async function loadProviders(): Promise<ProviderRow[]> {
  return invoke<ProviderRow[]>('provider_list');
}

/** 写入单个 Provider 配置（不含密钥；密钥走 setProviderKey）。 */
export async function saveProvider(row: {
  kind: string;
  name: string;
  provider: string;
  baseUrl: string;
  model: string;
  enabled: boolean;
  mode?: string;
}): Promise<void> {
  await invoke('provider_upsert', {
    kind: row.kind,
    name: row.name,
    provider: row.provider,
    baseUrl: row.baseUrl,
    model: row.model,
    enabled: row.enabled,
    mode: row.mode ?? 'cloud',
  });
}

/** 返回本地模型目录（项目内 models 目录），用于放置 faster-whisper / Whisper 等本地大模型权重。 */
export async function getModelsDir(): Promise<string> {
  return invoke<string>('get_models_dir');
}

/** 本地 ASR 模型尺寸清单（faster-whisper CTranslate2 权重在 HuggingFace 上的 repo 尺寸）。 */
export const LOCAL_ASR_MODELS = [
  { id: 'tiny', label: 'tiny（约 75MB · 最快·精度低）' },
  { id: 'base', label: 'base（约 140MB · 推荐起步）' },
  { id: 'small', label: 'small（约 460MB · 均衡）' },
  { id: 'medium', label: 'medium（约 1.5GB · 高精度）' },
  { id: 'large-v3', label: 'large-v3（约 3GB · 最高精度）' },
];

/** 下载本地 ASR 模型到项目内 models 目录，经 Tauri Channel 回报进度。 */
export async function downloadModel(
  model: string,
  source: string,
  onProgress: (p: { phase: string; file?: string; current?: number; total?: number; dir?: string }) => void,
): Promise<string> {
  const ch = createChannel(onProgress);
  return invoke<string>('download_model', { model, source, onProgress: ch });
}

/** 检查指定尺寸的本地模型是否已下载（models/{model} 下含 model.bin + config.json）。 */
export async function checkLocalModel(model: string): Promise<boolean> {
  return invoke<boolean>('check_local_model', { model });
}

/** 写入某网关 API Key（存系统凭据库，不落前端/SQLite 明文）。 */
export async function setProviderKey(kind: string, apiKey: string): Promise<void> {
  await invoke('provider_key_set', { kind, key: apiKey });
}

/** 读取某网关 API Key（通常为 null，密钥不回传前端）。 */
export async function getProviderKey(kind: string): Promise<string | null> {
  return invoke<string | null>('provider_key_get', { kind });
}

/** 连接测试：传入正在填写（未保存）的 Key 可直接测，否则回退到已保存的 Key。返回 'ok' 表示连通。 */
export async function testProvider(kind: string, apiKey?: string): Promise<string> {
  return invoke<string>('provider_test', { kind, apiKey: apiKey ?? null });
}

/** 提交任务并订阅进度（经 Tauri Channel 或浏览器伪通道）。 */
export async function submitTask(input: {
  kind: string;
  projectId?: string;
  payload?: unknown;
  onProgress: (m: ProgressMsg) => void;
}): Promise<string> {
  const ch = createChannel(input.onProgress);
  return invoke<string>('task_submit', {
    kind: input.kind,
    projectId: input.projectId,
    payload: input.payload ?? null,
    onProgress: ch,
  });
}

/** M1 链路验证：提交一个真实 Agnes /v1/chat 任务，订阅进度（含回答）。 */
export async function submitChatTask(
  prompt: string,
  onProgress: (m: ProgressMsg) => void,
  maxTokens = 512,
): Promise<string> {
  return submitTask({ kind: 'chat', payload: { prompt, maxTokens }, onProgress });
}

// ===========================================================================
// M2：影片模块高层封装
// ===========================================================================

/** 读取类型树。 */
export async function loadFilmCats(): Promise<FilmCategory[]> {
  return invoke<FilmCategory[]>('film_category_list');
}

/** 新建类型（order 由前端计算为末位+1）。 */
export async function createFilmCategory(name: string, order: number): Promise<string> {
  return invoke<string>('film_category_create', { name, order });
}

/** 重命名类型。 */
export async function renameFilmCategory(id: string, name: string): Promise<void> {
  await invoke('film_category_rename', { id, name });
}

/** 调整类型排序。 */
export async function reorderFilmCategory(id: string, order: number): Promise<void> {
  await invoke('film_category_reorder', { id, order });
}

/** 删除类型：strategy = "merge" 归并到 targetId；"cascade" 级联删工程+timeline。 */
export async function deleteFilmCategory(
  id: string,
  strategy: string,
  targetId?: string,
): Promise<void> {
  await invoke('film_category_delete', { id, strategy, targetId: targetId ?? null });
}

/** 读取某类型下的工程列表。 */
export async function loadFilmProjects(categoryId: string): Promise<FilmProject[]> {
  return invoke<FilmProject[]>('film_project_list', { categoryId });
}

/** 新建工程。 */
export async function createFilmProject(
  categoryId: string,
  title: string,
  cover?: string | null,
): Promise<string> {
  return invoke<string>('film_project_create', { categoryId, title, cover: cover ?? null });
}

/** 更新工程（标题/封面/状态/标签，按需传）。 */
export async function updateFilmProject(
  id: string,
  patch: { title?: string; cover?: string | null; status?: string; tags?: string },
): Promise<void> {
  await invoke('film_project_update', {
    id,
    title: patch.title ?? null,
    cover: patch.cover ?? null,
    status: patch.status ?? null,
    tags: patch.tags ?? null,
  });
}

/** 删除工程（级联 timeline）。 */
export async function deleteFilmProject(id: string): Promise<void> {
  await invoke('film_project_delete', { id });
}

/** 读取工程时间线（草稿/粗剪/精修）。 */
export async function loadTimeline(projectId: string): Promise<TimelineRow | null> {
  return invoke<TimelineRow | null>('film_timeline_load', { projectId });
}

/** 保存时间线（envelope + 扁平 clips 序列化为 JSON 传入）。 */
export async function saveTimeline(
  projectId: string,
  envelope: TimelineEnvelope,
  clips: TimelineClip[],
): Promise<string> {
  return invoke<string>('film_timeline_save', {
    projectId,
    tracks: JSON.stringify(envelope),
    clips: JSON.stringify(clips),
  });
}

/** 导入对齐：抽音轨 → ASR → 对齐 → 存草稿时间线（经 film_import 任务 + 进度通道）。 */
export async function submitFilmImport(
  projectId: string,
  videoPath: string,
  script: string,
  onProgress: (m: ProgressMsg) => void,
): Promise<string> {
  const ch = createChannel(onProgress);
  return invoke<string>('film_import', {
    projectId,
    videoPath,
    script,
    onProgress: ch,
  });
}

/** 智能粗剪：分段+对齐+静音检测 → 多轨粗剪时间线（film_smart_cut 任务）。 */
export async function submitFilmSmartCut(
  projectId: string,
  script: string,
  onProgress: (m: ProgressMsg) => void,
): Promise<string> {
  const ch = createChannel(onProgress);
  return invoke<string>('film_smart_cut', {
    projectId,
    script,
    onProgress: ch,
  });
}

/** 导出 MP4：切→烧字幕→concat→可选 TTS 混音→导出（film_export 任务）。 */
export async function submitFilmExport(
  projectId: string,
  opts: FilmExportOptions,
  onProgress: (m: ProgressMsg) => void,
): Promise<string> {
  const ch = createChannel(onProgress);
  return invoke<string>('film_export', {
    projectId,
    hw: opts.hw,
    resolution: opts.resolution,
    burnSub: opts.burnSub,
    mixVoice: opts.mixVoice,
    voiceMix: opts.voiceMix,
    script: opts.script,
    onProgress: ch,
  });
}

// ===========================================================================
// M3：口播模块高层封装
// ===========================================================================

export async function loadSpokenVideos(): Promise<SpokenVideo[]> {
  return invoke<SpokenVideo[]>('spoken_video_list');
}

export async function getSpokenVideo(id: string): Promise<SpokenVideo> {
  return invoke<SpokenVideo>('spoken_video_get', { id });
}

export async function createSpokenVideo(
  name: string,
  path: string,
  duration: number,
): Promise<string> {
  return invoke<string>('spoken_video_create', { name, path, duration });
}

export async function deleteSpokenVideo(id: string): Promise<void> {
  await invoke('spoken_video_delete', { id });
}

export async function extractSpokenScript(videoId: string): Promise<string> {
  return invoke<string>('spoken_extract_script', { videoId });
}

export async function loadSpokenEdits(videoId: string): Promise<SpokenEdit[]> {
  return invoke<SpokenEdit[]>('spoken_edits_list', { videoId });
}

export async function setSpokenEditAccepted(id: string, accepted: 0 | 1 | -1): Promise<void> {
  await invoke('spoken_edits_set_accepted', { id, accepted });
}

export async function applySpokenEdits(videoId: string): Promise<string> {
  return invoke<string>('spoken_apply_edits', { videoId });
}

export async function loadSpokenAssets(videoId: string): Promise<SpokenAsset[]> {
  return invoke<SpokenAsset[]>('spoken_assets_list', { videoId });
}

export async function createSpokenAsset(
  videoId: string,
  name: string,
  kind: string,
  path: string,
): Promise<string> {
  return invoke<string>('spoken_asset_create', { videoId, name, kind, path });
}

export async function deleteSpokenAsset(id: string): Promise<void> {
  await invoke('spoken_asset_delete', { id });
}

export async function loadSpokenKeywords(videoId: string): Promise<SpokenKeyword[]> {
  return invoke<SpokenKeyword[]>('spoken_keywords_list', { videoId });
}

export async function loadSpokenMatches(videoId: string): Promise<SpokenMatch[]> {
  return invoke<SpokenMatch[]>('spoken_matches_list', { videoId });
}

export async function toggleSpokenMatch(id: string): Promise<void> {
  await invoke('spoken_match_toggle', { id });
}

export async function submitSpokenAsr(
  videoId: string,
  onProgress: (m: ProgressMsg) => void,
): Promise<string> {
  const ch = createChannel(onProgress);
  return invoke<string>('spoken_asr', { videoId, onProgress: ch });
}

export async function submitSpokenDetect(
  videoId: string,
  onProgress: (m: ProgressMsg) => void,
): Promise<string> {
  const ch = createChannel(onProgress);
  return invoke<string>('spoken_detect', { videoId, onProgress: ch });
}

export async function submitSpokenKeyword(
  videoId: string,
  onProgress: (m: ProgressMsg) => void,
): Promise<string> {
  const ch = createChannel(onProgress);
  return invoke<string>('spoken_keyword', { videoId, onProgress: ch });
}

export async function matchSpokenAssets(videoId: string): Promise<SpokenMatch[]> {
  return invoke<SpokenMatch[]>('spoken_match_assets', { videoId });
}

export async function submitSpokenBurn(
  videoId: string,
  flower: string,
  onProgress: (m: ProgressMsg) => void,
): Promise<string> {
  const ch = createChannel(onProgress);
  return invoke<string>('spoken_burn', { videoId, flower, onProgress: ch });
}

export async function submitSpokenExport(
  videoId: string,
  opts: SpokenExportOptions,
  onProgress: (m: ProgressMsg) => void,
): Promise<string> {
  const ch = createChannel(onProgress);
  return invoke<string>('spoken_export', {
    videoId,
    burnFlower: opts.burnFlower,
    flower: opts.flower,
    onProgress: ch,
  });
}

// ===========================================================================
// M4：创作模块高层封装
// ===========================================================================

export async function loadCreationProjects(): Promise<CreationProject[]> {
  return invoke<CreationProject[]>('creation_project_list');
}

export async function getCreationProject(id: string): Promise<CreationProject> {
  return invoke<CreationProject>('creation_project_get', { id });
}

export async function createCreationProject(brief: string): Promise<string> {
  return invoke<string>('creation_project_create', { brief });
}

export async function updateCreationProject(
  id: string,
  patch: { brief?: string; script?: string; humanizedScript?: string; status?: string },
): Promise<void> {
  await invoke('creation_project_update', {
    id,
    brief: patch.brief ?? null,
    script: patch.script ?? null,
    humanizedScript: patch.humanizedScript ?? null,
    status: patch.status ?? null,
  });
}

export async function deleteCreationProject(id: string): Promise<void> {
  await invoke('creation_project_delete', { id });
}

export async function loadStoryboard(projectId: string): Promise<Storyboard | null> {
  return invoke<Storyboard | null>('storyboard_get', { projectId });
}

export async function saveStoryboard(
  projectId: string,
  shots: Shot[],
  styleRef: string,
): Promise<string> {
  return invoke<string>('storyboard_save', {
    projectId,
    shots: JSON.stringify(shots),
    styleRef,
  });
}

export async function loadGeneratedAssets(projectId: string): Promise<GeneratedAsset[]> {
  return invoke<GeneratedAsset[]>('generated_assets_list', { projectId });
}

export async function submitScriptWrite(
  projectId: string,
  onProgress: (m: ProgressMsg) => void,
): Promise<string> {
  const ch = createChannel(onProgress);
  return invoke<string>('submit_script_write', { projectId, onProgress: ch });
}

export async function submitScriptHumanize(
  projectId: string,
  onProgress: (m: ProgressMsg) => void,
): Promise<string> {
  const ch = createChannel(onProgress);
  return invoke<string>('submit_script_humanize', { projectId, onProgress: ch });
}

export async function submitStoryboardGen(
  projectId: string,
  onProgress: (m: ProgressMsg) => void,
): Promise<string> {
  const ch = createChannel(onProgress);
  return invoke<string>('submit_storyboard_gen', { projectId, onProgress: ch });
}

export async function submitImageGen(
  projectId: string,
  shotIndex: number,
  styleRef: string,
  onProgress: (m: ProgressMsg) => void,
): Promise<string> {
  const ch = createChannel(onProgress);
  return invoke<string>('submit_image_gen', {
    projectId,
    shotIndex,
    styleRef,
    onProgress: ch,
  });
}

// ===========================================================================
// M2.5：影片解说生成
// ===========================================================================

export async function submitFilmScriptGen(
  projectId: string,
  opts: FilmScriptGenOptions,
  onProgress: (m: ProgressMsg) => void,
): Promise<string> {
  const ch = createChannel(onProgress);
  return invoke<string>('submit_film_script_gen', {
    projectId,
    videoPath: opts.videoPath,
    title: opts.title,
    style: opts.style,
    styleName: opts.styleName ?? '',
    language: opts.language,
    duration: opts.duration,
    hint: opts.hint,
    mode: opts.mode ?? 'ai',
    view: opts.view ?? 'third',
    model: opts.model ?? 'default',
    analysisMode: opts.analysisMode ?? 0,
    voiceId: opts.voiceId ?? '',
    subtitleStyle: opts.subtitleStyle ?? '',
    analysis: opts.analysis ?? '',
    rolePrompt: opts.rolePrompt ?? '',
    onProgress: ch,
  });
}

export interface FilmVideoAnalysisOptions {
  videoPath: string;
  title: string;
  styleName?: string;
  start: number;
  end: number;
}

/// M2.6：提交影片视频分析（多模态大模型）。确认视频范围后触发，进度经 Channel 上报十步。
export async function submitFilmVideoAnalysis(
  projectId: string,
  opts: FilmVideoAnalysisOptions,
  onProgress: (m: ProgressMsg) => void,
): Promise<string> {
  const ch = createChannel(onProgress);
  return invoke<string>('submit_film_video_analysis', {
    projectId,
    videoPath: opts.videoPath,
    title: opts.title,
    styleName: opts.styleName ?? '',
    start: opts.start,
    end: opts.end,
    onProgress: ch,
  });
}

/// M2.6：读取影片视频分析总结报告（落库结果），供解说工作台重新进入时回填。
export async function getFilmAnalysis(projectId: string): Promise<string | null> {
  try {
    const r = await invoke<{ Ok: string | null } | string | null>('get_film_analysis', { projectId });
    if (r == null) return null;
    if (typeof r === 'string') return r;
    if (typeof r === 'object' && r && 'Ok' in r) return r.Ok;
    return null;
  } catch {
    return null;
  }
}
