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
}): Promise<void> {
  await invoke('provider_upsert', {
    kind: row.kind,
    name: row.name,
    provider: row.provider,
    baseUrl: row.baseUrl,
    model: row.model,
    enabled: row.enabled,
  });
}

/** 写入某网关 API Key（存系统凭据库，不落前端/SQLite 明文）。 */
export async function setProviderKey(kind: string, apiKey: string): Promise<void> {
  await invoke('provider_key_set', { kind, key: apiKey });
}

/** 读取某网关 API Key（通常为 null，密钥不回传前端）。 */
export async function getProviderKey(kind: string): Promise<string | null> {
  return invoke<string | null>('provider_key_get', { kind });
}

/** 连接测试：填了真实 Key 时返回 'ok'，否则抛出可读错误。 */
export async function testProvider(kind: string): Promise<string> {
  return invoke<string>('provider_test', { kind });
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
