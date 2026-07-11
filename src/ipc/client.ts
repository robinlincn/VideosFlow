// VideosFlow 前端 IPC 客户端
// 双模式：
//  - Tauri 运行时：走 window.__TAURI__.core.invoke + Tauri Channel（进度推送）
//  - 普通浏览器（npm run dev）：localStorage 回退，保证 UI 可点验（含 M2 影片全链路模拟）

const hasTauri =
  typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export type InvokeArgs = Record<string, unknown> | undefined;

export async function invoke<T = unknown>(
  cmd: string,
  args?: InvokeArgs,
): Promise<T> {
  if (hasTauri) {
    const core = (window as any).__TAURI__.core;
    return core.invoke(cmd, args) as Promise<T>;
  }
  return mockInvoke<T>(cmd, args);
}

// 创建进度通道：Tauri 下为原生 Channel；浏览器下为伪通道（由 mockInvoke 模拟推送）。
export function createChannel(onMessage: (msg: any) => void): any {
  if (hasTauri) {
    const { Channel } = (window as any).__TAURI__.core;
    return new Channel(onMessage);
  }
  return { __mockChannel: true, _on: onMessage };
}

// ---------------- 浏览器回退实现 ----------------
const LS_PROVIDERS = 'videosflow.providers.mock';
const LS_KEYS = 'videosflow.keys.mock';
const LS_FILM_CATS = 'videosflow.film.cats';
const LS_FILM_PROJECTS = 'videosflow.film.projects';
const LS_FILM_TIMELINES = 'videosflow.film.timelines';

interface MockProvider {
  id: string;
  kind: string;
  name: string;
  provider: string;
  baseUrl: string;
  model: string;
  enabled: boolean;
  hasKey: boolean;
}

function readJSON<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : fallback;
  } catch {
    return fallback;
  }
}
function writeJSON(key: string, val: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(val));
  } catch {
    /* ignore */
  }
}

// ---- 影片 mock 种子 ----
interface MockCat { id: string; name: string; order: number; editable: number; }
interface MockProj { id: string; categoryId: string; title: string; cover: string | null; status: string; tags: string; createdAt: number; }
interface MockTimeline { id: string; projectId: string; tracks: string; clips: string; updatedAt: number; }

function seedProvidersIfEmpty(): MockProvider[] {
  let list = readJSON<MockProvider[] | null>(LS_PROVIDERS, null);
  if (list && list.length) return list;
  list = [
    { id: 'p-llm', kind: 'llm', name: '文字大模型', provider: 'agnes', baseUrl: 'https://apihub.agnes-ai.com/v1', model: 'agnes-2.0-flash', enabled: true, hasKey: false },
    { id: 'p-img', kind: 'img', name: '图片大模型', provider: 'agnes', baseUrl: 'https://apihub.agnes-ai.com/v1', model: 'agnes-image-2.1-flash', enabled: true, hasKey: false },
    { id: 'p-video', kind: 'video', name: '视频大模型', provider: 'agnes', baseUrl: 'https://apihub.agnes-ai.com/v1', model: 'agnes-video-v2.0', enabled: true, hasKey: false },
    { id: 'p-asr', kind: 'asr', name: '语音识别', provider: 'agnes', baseUrl: 'https://apihub.agnes-ai.com/v1', model: 'agnes-asr-1.0', enabled: true, hasKey: false },
    { id: 'p-tts', kind: 'tts', name: '语音合成', provider: 'mimo', baseUrl: 'https://api.xiaomimimo.com/v1', model: 'mimo-v2.5-tts', enabled: true, hasKey: false },
  ];
  writeJSON(LS_PROVIDERS, list);
  return list;
}

function seedFilmIfEmpty(): void {
  if (!readJSON<MockCat[] | null>(LS_FILM_CATS, null)) {
    const cats: MockCat[] = [
      { id: 'c1', name: '电影', order: 1, editable: 1 },
      { id: 'c2', name: '故事', order: 2, editable: 1 },
      { id: 'c3', name: '电视剧', order: 3, editable: 1 },
      { id: 'c4', name: '动画片', order: 4, editable: 1 },
      { id: 'c5', name: '记录片', order: 5, editable: 1 },
    ];
    writeJSON(LS_FILM_CATS, cats);
  }
  if (!readJSON<MockProj[] | null>(LS_FILM_PROJECTS, null)) {
    const projs: MockProj[] = [
      { id: 'p-c1-1', categoryId: 'c1', title: '城市之光', cover: null, status: '已发布', tags: '', createdAt: 1 },
      { id: 'p-c1-2', categoryId: 'c1', title: '归途', cover: null, status: '草稿', tags: '', createdAt: 2 },
      { id: 'p-c1-3', categoryId: 'c1', title: '暗涌', cover: null, status: '草稿', tags: '', createdAt: 3 },
      { id: 'p-c2-1', categoryId: 'c2', title: '外婆的菜园', cover: null, status: '已发布', tags: '', createdAt: 4 },
      { id: 'p-c2-2', categoryId: 'c2', title: '雨夜书店', cover: null, status: '草稿', tags: '', createdAt: 5 },
      { id: 'p-c3-1', categoryId: 'c3', title: '长河', cover: null, status: '制作中', tags: '', createdAt: 6 },
      { id: 'p-c4-1', categoryId: 'c4', title: '喵星日记', cover: null, status: '已发布', tags: '', createdAt: 7 },
      { id: 'p-c4-2', categoryId: 'c4', title: '齿轮王国', cover: null, status: '已发布', tags: '', createdAt: 8 },
      { id: 'p-c4-3', categoryId: 'c4', title: '云朵工厂', cover: null, status: '草稿', tags: '', createdAt: 9 },
      { id: 'p-c4-4', categoryId: 'c4', title: '小灯塔', cover: null, status: '已发布', tags: '', createdAt: 10 },
      { id: 'p-c5-1', categoryId: 'c5', title: '候鸟', cover: null, status: '已发布', tags: '', createdAt: 11 },
      { id: 'p-c5-2', categoryId: 'c5', title: '匠心', cover: null, status: '制作中', tags: '', createdAt: 12 },
    ];
    writeJSON(LS_FILM_PROJECTS, projs);
  }
  if (!readJSON<Record<string, MockTimeline> | null>(LS_FILM_TIMELINES, null)) {
    writeJSON(LS_FILM_TIMELINES, {});
  }
}

function readCats(): MockCat[] { seedFilmIfEmpty(); return readJSON<MockCat[]>(LS_FILM_CATS, []); }
function readProjs(): MockProj[] { seedFilmIfEmpty(); return readJSON<MockProj[]>(LS_FILM_PROJECTS, []); }
function readTimelines(): Record<string, MockTimeline> { seedFilmIfEmpty(); return readJSON<Record<string, MockTimeline>>(LS_FILM_TIMELINES, {}); }

function segScript(script: string): { index: number; text: string }[] {
  const out: { index: number; text: string }[] = [];
  let buf = '';
  let idx = 0;
  for (const ch of script) {
    buf += ch;
    if ('。！？!?，,；;\n'.includes(ch)) {
      const t = buf.trim();
      if (t) { out.push({ index: idx, text: t }); idx++; }
      buf = '';
    }
  }
  const t = buf.trim();
  if (t) out.push({ index: idx, text: t });
  return out;
}

function saveMockTimeline(projectId: string, envelope: any, clips: any): string {
  const all = readTimelines();
  const id = all[projectId]?.id || 'tl-' + projectId;
  all[projectId] = {
    id,
    projectId,
    tracks: JSON.stringify(envelope),
    clips: JSON.stringify(clips),
    updatedAt: Date.now(),
  };
  writeJSON(LS_FILM_TIMELINES, all);
  return id;
}

function buildMockRoughCut(script: string, videoPath: string): { envelope: any; clips: any[] } {
  const segs = segScript(script);
  const video: any[] = [];
  const audio: any[] = [];
  const subtitle: any[] = [];
  const clips: any[] = [];
  segs.forEach((seg, i) => {
    const start = i * 4;
    const end = i * 4 + 4;
    const vId = 'clip-v-' + i;
    const aId = 'clip-a-' + i;
    const sId = 'clip-s-' + i;
    video.push({ id: vId, source: 'material', timelineStart: start, timelineEnd: end, srcStart: start, srcEnd: end, label: seg.text.slice(0, 12), text: '', flower: '', transition: 'none' });
    audio.push({ id: aId, source: 'material', timelineStart: start, timelineEnd: end, srcStart: start, srcEnd: end, label: '原声', text: '', flower: '', transition: 'none' });
    subtitle.push({ id: sId, source: 'subtitle', timelineStart: start, timelineEnd: end, srcStart: start, srcEnd: end, label: '', text: seg.text, flower: '', transition: 'none' });
    clips.push(...[video[video.length - 1], audio[audio.length - 1], subtitle[subtitle.length - 1]]);
  });
  const envelope = {
    asr: [],
    scriptSegs: segs,
    alignment: {},
    tracks: [
      { id: 'video', kind: 'video', name: '视频', volume: 1, muted: false, clips: video },
      { id: 'audio', kind: 'audio', name: '音频', volume: 1, muted: false, clips: audio },
      { id: 'subtitle', kind: 'subtitle', name: '字幕', volume: 1, muted: false, clips: subtitle },
    ],
    videoPath,
  };
  return { envelope, clips };
}

function delay(ms: number) {
  return new Promise((r) => setTimeout(r, ms));
}

async function mockInvoke<T>(cmd: string, args?: InvokeArgs): Promise<T> {
  const a = (args || {}) as any;
  switch (cmd) {
    case 'ping':
      return 'pong' as any;
    case 'provider_list': {
      const list = seedProvidersIfEmpty();
      return list as any;
    }
    case 'provider_upsert': {
      const list = seedProvidersIfEmpty();
      const idx = list.findIndex((p) => p.kind === a.kind);
      const row: MockProvider = {
        id: idx >= 0 ? list[idx].id : 'p-' + a.kind,
        kind: a.kind,
        name: a.name || a.kind,
        provider: a.provider || '',
        baseUrl: a.baseUrl || '',
        model: a.model || '',
        enabled: a.enabled !== false,
        hasKey: idx >= 0 ? list[idx].hasKey : false,
      };
      if (idx >= 0) list[idx] = row;
      else list.push(row);
      writeJSON(LS_PROVIDERS, list);
      return undefined as any;
    }
    case 'provider_key_set': {
      const keys = readJSON<Record<string, string>>(LS_KEYS, {});
      if (a.key) keys[a.kind] = a.key;
      else delete keys[a.kind];
      writeJSON(LS_KEYS, keys);
      const list = seedProvidersIfEmpty();
      const p = list.find((x) => x.kind === a.kind);
      if (p) {
        p.hasKey = !!a.key;
        writeJSON(LS_PROVIDERS, list);
      }
      return undefined as any;
    }
    case 'provider_key_get': {
      const keys = readJSON<Record<string, string>>(LS_KEYS, {});
      return (keys[a.kind] || null) as any;
    }
    case 'provider_test': {
      await delay(700);
      const list = seedProvidersIfEmpty();
      const p = list.find((x) => x.kind === a.kind);
      if (p) {
        p.hasKey = true;
        writeJSON(LS_PROVIDERS, list);
      }
      return 'ok' as any;
    }
    case 'task_submit': {
      const taskId = 'mock-' + Math.random().toString(36).slice(2);
      const ch = a.on_progress;
      const kind = a.kind;
      if (ch && ch.__mockChannel && ch._on) {
        if (kind === 'chat') {
          const prompt = (a.payload && a.payload.prompt) || 'ping';
          const steps = [5, 60, 100];
          steps.forEach((p, i) =>
            setTimeout(() => {
              if (i === steps.length - 1) {
                ch._on({
                  taskId,
                  progress: 100,
                  status: 'done',
                  message: '真实对话完成（mock）',
                  payload: {
                    answer: `（mock）关于「${prompt}」：VideosFlow 是一款基于 AI 的智能视频生产工作室，帮你把想法变成成片。`,
                  },
                });
              } else {
                ch._on({
                  taskId,
                  progress: p,
                  status: 'running',
                  message: i === 0 ? '任务入队' : 'AI 引擎可达',
                });
              }
            }, 300 + i * 400),
          );
        } else {
          const steps = [5, 30, 60, 100];
          steps.forEach((p, i) =>
            setTimeout(
              () =>
                ch._on({
                  taskId,
                  progress: p,
                  status: i === steps.length - 1 ? 'done' : 'running',
                  message: i === steps.length - 1 ? '完成' : '处理中',
                }),
              300 + i * 300,
            ),
          );
        }
      }
      return taskId as any;
    }
    case 'task_status': {
      return { id: a.id, status: 'done', progress: 100, log: '完成（mock）' } as any;
    }

    // ---- M2：影片模块 ----
    case 'film_category_list':
      return readCats() as any;
    case 'film_category_create': {
      const cats = readCats();
      const row: MockCat = { id: 'c-' + Date.now().toString(36), name: a.name, order: a.order, editable: 1 };
      cats.push(row);
      writeJSON(LS_FILM_CATS, cats);
      return row.id as any;
    }
    case 'film_category_rename': {
      const cats = readCats().map((c) => (c.id === a.id ? { ...c, name: a.name } : c));
      writeJSON(LS_FILM_CATS, cats);
      return undefined as any;
    }
    case 'film_category_reorder': {
      const cats = readCats().map((c) => (c.id === a.id ? { ...c, order: a.order } : c));
      writeJSON(LS_FILM_CATS, cats);
      return undefined as any;
    }
    case 'film_category_delete': {
      let cats = readCats();
      const projs = readProjs();
      if (a.strategy === 'merge' && a.targetId) {
        const moved = projs.map((p) => (p.categoryId === a.id ? { ...p, categoryId: a.targetId } : p));
        writeJSON(LS_FILM_PROJECTS, moved);
      } else {
        const remain = projs.filter((p) => p.categoryId !== a.id);
        const timelines = readTimelines();
        for (const p of projs.filter((p) => p.categoryId === a.id)) delete timelines[p.id];
        writeJSON(LS_FILM_TIMELINES, timelines);
        writeJSON(LS_FILM_PROJECTS, remain);
      }
      cats = cats.filter((c) => c.id !== a.id);
      writeJSON(LS_FILM_CATS, cats);
      return undefined as any;
    }
    case 'film_project_list':
      return readProjs().filter((p) => p.categoryId === a.categoryId) as any;
    case 'film_project_create': {
      const projs = readProjs();
      const row: MockProj = {
        id: 'p-' + Date.now().toString(36),
        categoryId: a.categoryId,
        title: a.title,
        cover: a.cover ?? null,
        status: '草稿',
        tags: '',
        createdAt: Date.now(),
      };
      projs.push(row);
      writeJSON(LS_FILM_PROJECTS, projs);
      return row.id as any;
    }
    case 'film_project_update': {
      const projs = readProjs().map((p) => {
        if (p.id !== a.id) return p;
        return {
          ...p,
          title: a.title ?? p.title,
          cover: a.cover !== null && a.cover !== undefined ? a.cover : p.cover,
          status: a.status ?? p.status,
          tags: a.tags ?? p.tags,
        };
      });
      writeJSON(LS_FILM_PROJECTS, projs);
      return undefined as any;
    }
    case 'film_project_delete': {
      const projs = readProjs().filter((p) => p.id !== a.id);
      writeJSON(LS_FILM_PROJECTS, projs);
      const timelines = readTimelines();
      delete timelines[a.id];
      writeJSON(LS_FILM_TIMELINES, timelines);
      return undefined as any;
    }
    case 'film_timeline_load': {
      const all = readTimelines();
      return (all[a.projectId] || null) as any;
    }
    case 'film_timeline_save': {
      const id = saveMockTimeline(a.projectId, JSON.parse(a.tracks), JSON.parse(a.clips));
      return id as any;
    }
    case 'film_import': {
      const taskId = 'mock-' + Math.random().toString(36).slice(2);
      const ch = a.on_progress;
      if (ch && ch.__mockChannel && ch._on) {
        const steps = [15, 40, 90, 100];
        steps.forEach((p, i) =>
          setTimeout(() => {
            if (i === steps.length - 1) {
              const envelope = {
                asr: [],
                scriptSegs: segScript(a.script || ''),
                alignment: {},
                tracks: [],
                videoPath: a.videoPath || '',
              };
              saveMockTimeline(a.projectId, envelope, []);
              ch._on({
                taskId,
                progress: 100,
                status: 'done',
                message: '导入完成（mock 降级）',
                payload: { degraded: true, alignedPct: 0 },
              });
            } else {
              ch._on({
                taskId,
                progress: p,
                status: 'running',
                message: i === 1 ? '抽取音轨' : i === 2 ? '语音识别(ASR)' : '处理中',
              });
            }
          }, 300 + i * 400),
        );
      }
      return taskId as any;
    }
    case 'film_smart_cut': {
      const taskId = 'mock-' + Math.random().toString(36).slice(2);
      const ch = a.on_progress;
      if (ch && ch.__mockChannel && ch._on) {
        const steps = [15, 55, 100];
        steps.forEach((p, i) =>
          setTimeout(() => {
            if (i === steps.length - 1) {
              const { envelope, clips } = buildMockRoughCut(a.script || '', '');
              saveMockTimeline(a.projectId, envelope, clips);
              ch._on({
                taskId,
                progress: 100,
                status: 'done',
                message: '自动切点完成（mock）',
                payload: { clips: clips.length },
              });
            } else {
              ch._on({
                taskId,
                progress: p,
                status: 'running',
                message: i === 0 ? '载入时间线' : '智能切点',
              });
            }
          }, 300 + i * 450),
        );
      }
      return taskId as any;
    }
    case 'film_export': {
      const taskId = 'mock-' + Math.random().toString(36).slice(2);
      const ch = a.on_progress;
      if (ch && ch.__mockChannel && ch._on) {
        const steps = [20, 50, 75, 100];
        steps.forEach((p, i) =>
          setTimeout(() => {
            if (i === steps.length - 1) {
              ch._on({
                taskId,
                progress: 100,
                status: 'done',
                message: '导出 MP4 完成（mock）',
                payload: { outPath: 'mock_export_' + a.projectId + '.mp4' },
              });
            } else {
              ch._on({
                taskId,
                progress: p,
                status: 'running',
                message: i === 0 ? '切片段' : i === 1 ? '合成时间线' : '导出',
              });
            }
          }, 300 + i * 450),
        );
      }
      return taskId as any;
    }
    default:
      throw new Error(`未知命令: ${cmd}`);
  }
}
