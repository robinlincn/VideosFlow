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
const LS_SPOKEN_VIDEOS = 'videosflow.spoken.videos';
const LS_SPOKEN_EDITS = 'videosflow.spoken.edits';
const LS_SPOKEN_ASSETS = 'videosflow.spoken.assets';
const LS_SPOKEN_KEYWORDS = 'videosflow.spoken.keywords';
const LS_SPOKEN_MATCHES = 'videosflow.spoken.matches';
const LS_CREATION_PROJECTS = 'videosflow.creation.projects';
const LS_STORYBOARDS = 'videosflow.creation.storyboards';
const LS_GENERATED_ASSETS = 'videosflow.creation.assets';

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

    // ========== M3：口播模块 mock ==========
    case 'spoken_video_list': {
      const all = readJSON<any[]>(LS_SPOKEN_VIDEOS, []);
      return all as any;
    }
    case 'spoken_video_get': {
      const all = readJSON<any[]>(LS_SPOKEN_VIDEOS, []);
      return (all.find((v) => v.id === a.id) || null) as any;
    }
    case 'spoken_video_create': {
      const all = readJSON<any[]>(LS_SPOKEN_VIDEOS, []);
      const id = 'sv-' + Date.now().toString(36);
      const row = {
        id, name: a.name, path: a.path, duration: a.duration,
        transcript: '', script: '', cleanScript: '', createdAt: Date.now(),
      };
      all.unshift(row);
      writeJSON(LS_SPOKEN_VIDEOS, all);
      return id as any;
    }
    case 'spoken_video_delete': {
      const all = readJSON<any[]>(LS_SPOKEN_VIDEOS, []).filter((v) => v.id !== a.id);
      writeJSON(LS_SPOKEN_VIDEOS, all);
      // 级联清理
      const edits = readJSON<Record<string, any[]>>(LS_SPOKEN_EDITS, {});
      delete edits[a.id]; writeJSON(LS_SPOKEN_EDITS, edits);
      const assets = readJSON<Record<string, any[]>>(LS_SPOKEN_ASSETS, {});
      delete assets[a.id]; writeJSON(LS_SPOKEN_ASSETS, assets);
      const kws = readJSON<Record<string, any[]>>(LS_SPOKEN_KEYWORDS, {});
      delete kws[a.id]; writeJSON(LS_SPOKEN_KEYWORDS, kws);
      const matches = readJSON<Record<string, any[]>>(LS_SPOKEN_MATCHES, {});
      delete matches[a.id]; writeJSON(LS_SPOKEN_MATCHES, matches);
      return undefined as any;
    }
    case 'spoken_extract_script': {
      const all = readJSON<any[]>(LS_SPOKEN_VIDEOS, []);
      const v = all.find((x) => x.id === a.videoId);
      if (!v) return '' as any;
      // 从 transcript JSON 解析并提取
      try {
        const segs = JSON.parse(v.transcript || '[]');
        const fillers = ['那个', '呃', '啊', '嗯', '这个'];
        const script = segs.map((s: any) => {
          let t: string = s.text || '';
          for (const f of fillers) t = t.split(f).join('');
          return t.replace(/\s+/g, '').trim();
        }).filter(Boolean).join('。');
        v.script = script + (script && !script.endsWith('。') ? '。' : '');
        writeJSON(LS_SPOKEN_VIDEOS, all);
        return v.script as any;
      } catch { return '' as any; }
    }
    case 'spoken_edits_list': {
      const edits = readJSON<Record<string, any[]>>(LS_SPOKEN_EDITS, {});
      return (edits[a.videoId] || []) as any;
    }
    case 'spoken_edits_set_accepted': {
      const edits = readJSON<Record<string, any[]>>(LS_SPOKEN_EDITS, {});
      const arr = edits[a.videoId] || [];
      const it = arr.find((x) => x.id === a.id);
      if (it) { it.accepted = a.accepted; }
      edits[a.videoId] = arr;
      writeJSON(LS_SPOKEN_EDITS, edits);
      return undefined as any;
    }
    case 'spoken_apply_edits': {
      const all = readJSON<any[]>(LS_SPOKEN_VIDEOS, []);
      const v = all.find((x) => x.id === a.videoId);
      if (!v) return '' as any;
      const edits = readJSON<Record<string, any[]>>(LS_SPOKEN_EDITS, {});
      const accepted = (edits[a.videoId] || []).filter((x: any) => x.accepted === 1);
      // mock：把所有 accepted edits 的 text 从 script 中删除
      let clean = v.script || '';
      for (const e of accepted) {
        if (e.text) clean = clean.split(e.text).join('');
      }
      v.cleanScript = clean;
      writeJSON(LS_SPOKEN_VIDEOS, all);
      return clean as any;
    }
    case 'spoken_assets_list': {
      const assets = readJSON<Record<string, any[]>>(LS_SPOKEN_ASSETS, {});
      return (assets[a.videoId] || []) as any;
    }
    case 'spoken_asset_create': {
      const assets = readJSON<Record<string, any[]>>(LS_SPOKEN_ASSETS, {});
      const id = 'sa-' + Date.now().toString(36);
      const arr = assets[a.videoId] || [];
      arr.push({ id, videoId: a.videoId, name: a.name, type: a.kind, path: a.path });
      assets[a.videoId] = arr;
      writeJSON(LS_SPOKEN_ASSETS, assets);
      return id as any;
    }
    case 'spoken_asset_delete': {
      const assets = readJSON<Record<string, any[]>>(LS_SPOKEN_ASSETS, {});
      for (const k of Object.keys(assets)) {
        assets[k] = (assets[k] || []).filter((x: any) => x.id !== a.id);
      }
      writeJSON(LS_SPOKEN_ASSETS, assets);
      return undefined as any;
    }
    case 'spoken_keywords_list': {
      const kws = readJSON<Record<string, any[]>>(LS_SPOKEN_KEYWORDS, {});
      return (kws[a.videoId] || []) as any;
    }
    case 'spoken_matches_list': {
      const m = readJSON<Record<string, any[]>>(LS_SPOKEN_MATCHES, {});
      return (m[a.videoId] || []) as any;
    }
    case 'spoken_match_toggle': {
      const m = readJSON<Record<string, any[]>>(LS_SPOKEN_MATCHES, {});
      for (const k of Object.keys(m)) {
        for (const x of (m[k] || [])) {
          if (x.id === a.id) x.applied = x.applied ? 0 : 1;
        }
      }
      writeJSON(LS_SPOKEN_MATCHES, m);
      return undefined as any;
    }
    case 'spoken_match_assets': {
      const kws = readJSON<Record<string, any[]>>(LS_SPOKEN_KEYWORDS, {});
      const assets = readJSON<Record<string, any[]>>(LS_SPOKEN_ASSETS, {});
      const arr = (assets[a.videoId] || []).slice();
      const priority: Record<string, number> = { image: 0, clip: 1, bgm: 2, sfx: 3 };
      arr.sort((x: any, y: any) => (priority[x.type] ?? 9) - (priority[y.type] ?? 9));
      const used: Record<string, boolean> = {};
      const out: any[] = [];
      for (const kw of (kws[a.videoId] || [])) {
        const a1 = arr.find((x: any) => !used[x.id]);
        if (a1) {
          used[a1.id] = true;
          out.push({ id: 'sm-' + Math.random().toString(36).slice(2), videoId: a.videoId, segStart: 0, segEnd: 0, segText: kw.text, keyword: kw.text, assetId: a1.id, applied: 1 });
        }
      }
      const m = readJSON<Record<string, any[]>>(LS_SPOKEN_MATCHES, {});
      m[a.videoId] = out;
      writeJSON(LS_SPOKEN_MATCHES, m);
      return out as any;
    }

    // ---- M3 任务类型（task_submit 分支由 on_progress 通道消费） ----
    case 'spoken_asr': {
      const taskId = 'mock-' + Math.random().toString(36).slice(2);
      const ch = a.on_progress;
      const videoId = a.videoId;
      const steps = [20, 60, 100];
      steps.forEach((p, i) => setTimeout(() => {
        if (i === steps.length - 1) {
          const all = readJSON<any[]>(LS_SPOKEN_VIDEOS, []);
          const v = all.find((x) => x.id === videoId);
          if (v) {
            v.transcript = JSON.stringify([{ start: 0, end: 0, text: '大家好，今天给大家介绍我们的新产品 VideosFlow。\n那个，它是一款，呃，基于 AI 的智能视频剪辑工具。\n可以自动根据文案剪辑视频。\n还能修掉口播里的气口和口误，提升观感。' }]);
            v.script = '大家好，今天给大家介绍我们的新产品 VideosFlow。它是一款基于 AI 的智能视频剪辑工具。可以自动根据文案剪辑视频。还能修掉口播里的气口和口误，提升观感。';
            writeJSON(LS_SPOKEN_VIDEOS, all);
          }
          ch && ch._on && ch._on({ taskId, progress: 100, status: 'done', message: '识别完成（mock）', payload: { degraded: true } });
        } else {
          ch && ch._on && ch._on({ taskId, progress: p, status: 'running', message: i === 0 ? '抽取音轨' : '语音识别' });
        }
      }, 300 + i * 500));
      return taskId as any;
    }
    case 'spoken_detect': {
      const taskId = 'mock-' + Math.random().toString(36).slice(2);
      const ch = a.on_progress;
      const videoId = a.videoId;
      const steps = [20, 55, 85, 100];
      steps.forEach((p, i) => setTimeout(() => {
        if (i === steps.length - 1) {
          const edits = readJSON<Record<string, any[]>>(LS_SPOKEN_EDITS, {});
          edits[videoId] = [
            { id: 'e1', videoId, issueType: 'gap', start: 0.5, end: 1.2, text: '静音 0.5s–1.2s', suggestion: '建议裁剪', accepted: 0 },
            { id: 'e2', videoId, issueType: 'mistake', start: 0, end: 0, text: '那个，呃，', suggestion: '删除填充词', accepted: 0 },
            { id: 'e3', videoId, issueType: 'repeat', start: 0, end: 0, text: '气口和口误', suggestion: '与上文重复，建议合并', accepted: 0 },
          ];
          writeJSON(LS_SPOKEN_EDITS, edits);
          ch && ch._on && ch._on({ taskId, progress: 100, status: 'done', message: '检测到 3 个问题（mock）', payload: { count: 3 } });
        } else {
          ch && ch._on && ch._on({ taskId, progress: p, status: 'running', message: ['检测气口', '检测重复', 'Agnes LLM 检测口误'][i] });
        }
      }, 300 + i * 400));
      return taskId as any;
    }
    case 'spoken_keyword': {
      const taskId = 'mock-' + Math.random().toString(36).slice(2);
      const ch = a.on_progress;
      const videoId = a.videoId;
      const steps = [30, 70, 100];
      steps.forEach((p, i) => setTimeout(() => {
        if (i === steps.length - 1) {
          const kws = readJSON<Record<string, any[]>>(LS_SPOKEN_KEYWORDS, {});
          kws[videoId] = [
            { id: 'kw1', videoId, text: '智能', weight: 0.92 },
            { id: 'kw2', videoId, text: '视频', weight: 0.88 },
            { id: 'kw3', videoId, text: '剪辑', weight: 0.85 },
            { id: 'kw4', videoId, text: '气口', weight: 0.72 },
            { id: 'kw5', videoId, text: '口误', weight: 0.68 },
          ];
          writeJSON(LS_SPOKEN_KEYWORDS, kws);
          ch && ch._on && ch._on({ taskId, progress: 100, status: 'done', message: '抽取 5 个关键词（mock）', payload: { count: 5 } });
        } else {
          ch && ch._on && ch._on({ taskId, progress: p, status: 'running', message: i === 0 ? 'Agnes 抽取关键词' : 'TF-IDF 兜底' });
        }
      }, 300 + i * 400));
      return taskId as any;
    }
    case 'spoken_burn': {
      const taskId = 'mock-' + Math.random().toString(36).slice(2);
      const ch = a.on_progress;
      const steps = [30, 60, 100];
      steps.forEach((p, i) => setTimeout(() => {
        if (i === steps.length - 1) {
          ch && ch._on && ch._on({ taskId, progress: 100, status: 'done', message: '花字烧录完成（mock）', payload: { outPath: 'mock_burn.mp4' } });
        } else {
          ch && ch._on && ch._on({ taskId, progress: p, status: 'running', message: i === 0 ? '生成 ASS' : '烧录花字' });
        }
      }, 300 + i * 500));
      return taskId as any;
    }
    case 'spoken_export': {
      const taskId = 'mock-' + Math.random().toString(36).slice(2);
      const ch = a.on_progress;
      const steps = [20, 50, 80, 100];
      steps.forEach((p, i) => setTimeout(() => {
        if (i === steps.length - 1) {
          ch && ch._on && ch._on({ taskId, progress: 100, status: 'done', message: '干净片段导出完成（mock）', payload: { outPath: 'mock_clean.mp4' } });
        } else {
          ch && ch._on && ch._on({ taskId, progress: p, status: 'running', message: ['切片段', '拼接', '导出 MP4'][i] });
        }
      }, 300 + i * 450));
      return taskId as any;
    }

    // ========== M4：创作模块 mock ==========
    case 'creation_project_list': {
      return readJSON<any[]>(LS_CREATION_PROJECTS, []) as any;
    }
    case 'creation_project_get': {
      const all = readJSON<any[]>(LS_CREATION_PROJECTS, []);
      return (all.find((p) => p.id === a.id) || null) as any;
    }
    case 'creation_project_create': {
      const all = readJSON<any[]>(LS_CREATION_PROJECTS, []);
      const id = 'cp-' + Date.now().toString(36);
      const row = { id, brief: a.brief, script: '', humanizedScript: '', status: 'draft', createdAt: Date.now() };
      all.unshift(row);
      writeJSON(LS_CREATION_PROJECTS, all);
      return id as any;
    }
    case 'creation_project_update': {
      const all = readJSON<any[]>(LS_CREATION_PROJECTS, []);
      const it = all.find((p) => p.id === a.id);
      if (it) {
        if (a.brief != null) it.brief = a.brief;
        if (a.script != null) it.script = a.script;
        if (a.humanizedScript != null) it.humanizedScript = a.humanizedScript;
        if (a.status != null) it.status = a.status;
      }
      writeJSON(LS_CREATION_PROJECTS, all);
      return undefined as any;
    }
    case 'creation_project_delete': {
      const all = readJSON<any[]>(LS_CREATION_PROJECTS, []).filter((p) => p.id !== a.id);
      writeJSON(LS_CREATION_PROJECTS, all);
      const sbs = readJSON<Record<string, any>>(LS_STORYBOARDS, {});
      for (const k of Object.keys(sbs)) if (k === a.id) delete sbs[k];
      writeJSON(LS_STORYBOARDS, sbs);
      const assets = readJSON<Record<string, any[]>>(LS_GENERATED_ASSETS, {});
      delete assets[a.id];
      writeJSON(LS_GENERATED_ASSETS, assets);
      return undefined as any;
    }
    case 'storyboard_get': {
      const sbs = readJSON<Record<string, any>>(LS_STORYBOARDS, {});
      return (sbs[a.projectId] || null) as any;
    }
    case 'storyboard_save': {
      const sbs = readJSON<Record<string, any>>(LS_STORYBOARDS, {});
      sbs[a.projectId] = {
        id: sbs[a.projectId]?.id || ('sb-' + Date.now().toString(36)),
        projectId: a.projectId,
        shots: JSON.parse(a.shots || '[]'),
        styleRef: a.styleRef,
        updatedAt: Date.now(),
      };
      writeJSON(LS_STORYBOARDS, sbs);
      return sbs[a.projectId].id as any;
    }
    case 'generated_assets_list': {
      const all = readJSON<Record<string, any[]>>(LS_GENERATED_ASSETS, {});
      return (all[a.projectId] || []) as any;
    }

    // ---- M4 任务类型 ----
    case 'submit_script_write':
    case 'script_write': {
      const taskId = 'mock-' + Math.random().toString(36).slice(2);
      const ch = a.on_progress;
      const projectId = a.projectId;
      const steps = [30, 70, 100];
      steps.forEach((p, i) => setTimeout(() => {
        if (i === steps.length - 1) {
          const all = readJSON<any[]>(LS_CREATION_PROJECTS, []);
          const p1 = all.find((x) => x.id === projectId);
          if (p1) {
            p1.script = '大家好，今天聊一个新手也能上手的事——根据需求自动生成文案。\n\n你只需要给个大体的需求，它就能自动写稿、拆分镜、出图片，还能配音加字幕。\n\n以前剪一条视频要折腾大半天，现在把想法交给它，剩下的交给流程。\n\n如果你也想轻松做视频，不妨试试看。';
            p1.status = 'writing';
            writeJSON(LS_CREATION_PROJECTS, all);
          }
          ch && ch._on && ch._on({ taskId, progress: 100, status: 'done', message: '文案生成完成（mock）', payload: { script: p1?.script || '' } });
        } else {
          ch && ch._on && ch._on({ taskId, progress: p, status: 'running', message: i === 0 ? '调用 Agnes' : '解析文案' });
        }
      }, 300 + i * 500));
      return taskId as any;
    }
    case 'submit_script_humanize':
    case 'script_humanize': {
      const taskId = 'mock-' + Math.random().toString(36).slice(2);
      const ch = a.on_progress;
      const projectId = a.projectId;
      const steps = [30, 70, 100];
      steps.forEach((p, i) => setTimeout(() => {
        if (i === steps.length - 1) {
          const all = readJSON<any[]>(LS_CREATION_PROJECTS, []);
          const p1 = all.find((x) => x.id === projectId);
          if (p1) {
            p1.humanizedScript = '嗨，今天说个特适合新手的事儿——给个想法就能出片。\n\n你大概说个想法就行，它自己写稿、拆镜头、出图，连配音字幕都帮你弄好。\n\n以前剪一条视频得忙活大半天，现在你把点子丢给它，流程自动跑完。\n\n想轻松做视频的话，真的可以试一下。';
            p1.status = 'humanized';
            writeJSON(LS_CREATION_PROJECTS, all);
          }
          ch && ch._on && ch._on({ taskId, progress: 100, status: 'done', message: '去 AI 味完成（mock）', payload: { human: p1?.humanizedScript || '' } });
        } else {
          ch && ch._on && ch._on({ taskId, progress: p, status: 'running', message: i === 0 ? '调用 Agnes 去 AI 味' : '改写文案' });
        }
      }, 300 + i * 500));
      return taskId as any;
    }
    case 'submit_storyboard_gen':
    case 'storyboard_gen': {
      const taskId = 'mock-' + Math.random().toString(36).slice(2);
      const ch = a.on_progress;
      const projectId = a.projectId;
      const steps = [30, 70, 100];
      steps.forEach((p, i) => setTimeout(() => {
        if (i === steps.length - 1) {
          const shots = [
            { index: 0, desc: '开场：主持人近景微笑，背景虚化', dialogue: '嗨，今天说个特适合新手的事儿。', dur: 5, cam: '近景' },
            { index: 1, desc: '界面展示：AI 剪辑按钮高亮', dialogue: '你大概说个想法就行。', dur: 6, cam: '推近' },
            { index: 2, desc: '动画：文案自动变成时间线', dialogue: '连配音字幕都帮你弄好。', dur: 6, cam: '平摇' },
            { index: 3, desc: '结尾：主持人比赞，品牌浮现', dialogue: '想轻松做视频，真的可以试一下。', dur: 4, cam: '中景' },
          ];
          const sbs = readJSON<Record<string, any>>(LS_STORYBOARDS, {});
          sbs[projectId] = {
            id: 'sb-' + Date.now().toString(36),
            projectId,
            shots,
            styleRef: '现实',
            updatedAt: Date.now(),
          };
          writeJSON(LS_STORYBOARDS, sbs);
          const all = readJSON<any[]>(LS_CREATION_PROJECTS, []);
          const p1 = all.find((x) => x.id === projectId);
          if (p1) { p1.status = 'storyboard'; writeJSON(LS_CREATION_PROJECTS, all); }
          ch && ch._on && ch._on({ taskId, progress: 100, status: 'done', message: '分镜生成完成（mock）', payload: { shots: JSON.stringify(shots) } });
        } else {
          ch && ch._on && ch._on({ taskId, progress: p, status: 'running', message: i === 0 ? 'Agnes 生成中' : '解析 JSON' });
        }
      }, 300 + i * 600));
      return taskId as any;
    }
    case 'submit_image_gen':
    case 'image_gen': {
      const taskId = 'mock-' + Math.random().toString(36).slice(2);
      const ch = a.on_progress;
      const projectId = a.projectId;
      const shotIndex = a.shotIndex ?? 0;
      const steps = [30, 60, 90, 100];
      steps.forEach((p, i) => setTimeout(() => {
        if (i === steps.length - 1) {
          const assets = readJSON<Record<string, any[]>>(LS_GENERATED_ASSETS, {});
          if (!assets[projectId]) assets[projectId] = [];
          assets[projectId].push({
            id: 'ga-' + Math.random().toString(36).slice(2),
            projectId,
            shotId: shotIndex,
            kind: 'image',
            path: `mock://shot_${shotIndex}_${Date.now()}.png`,
            createdAt: Date.now(),
          });
          writeJSON(LS_GENERATED_ASSETS, assets);
          ch && ch._on && ch._on({ taskId, progress: 100, status: 'done', message: '图片生成完成（mock）', payload: { path: `mock://shot_${shotIndex}.png` } });
        } else {
          ch && ch._on && ch._on({ taskId, progress: p, status: 'running', message: i === 0 ? '调用 Agnes /images/generations' : i === 1 ? '解码 base64' : '写入本地' });
        }
      }, 300 + i * 500));
      return taskId as any;
    }

    // ========== M2.5：影片解说生成 mock ==========
    case 'submit_film_script_gen':
    case 'film_script_gen': {
      const taskId = 'mock-' + Math.random().toString(36).slice(2);
      const ch = a.on_progress;
      const projectId = a.projectId;
      const steps = [15, 30, 55, 85, 100];
      steps.forEach((p, i) => setTimeout(() => {
        if (i === steps.length - 1) {
          // 模拟 6 段式 LLM 输出 → 落 film_projects.script
          const mockScript = [
            '[开端] 00:00-00:30 当城市被第一缕阳光唤醒，主人公缓缓走入我们的视野，一个平凡的清晨却暗藏波澜。',
            '[铺垫] 00:30-01:30 镜头切换到办公室，电话铃声打破宁静，一通改变命运的电话即将响起。',
            '[冲突] 01:30-02:45 突然闯入的不速之客打破了生活的平衡，气氛瞬间紧张到极点。',
            '[高潮] 02:45-04:00 追逐、躲藏、反击，每一个画面都扣人心弦，看得人屏住呼吸。',
            '[反转] 04:00-05:00 真相浮出水面，原来所有线索都指向一个意想不到的答案。',
            '[结局] 05:00-05:45 故事在一声叹息中落幕，生活的真相远比想象中复杂。',
          ].join('\n');
          const films = readJSON<any[]>(LS_FILM_PROJECTS, []);
          const film = films.find(f => f.id === projectId);
          if (film) {
            film.script = mockScript;
            writeJSON(LS_FILM_PROJECTS, films);
          }
          ch && ch._on && ch._on({ taskId, progress: 100, status: 'done', message: '解说文案生成完成（mock）', payload: { script: mockScript } });
        } else {
          ch && ch._on && ch._on({ taskId, progress: p, status: 'running', message: ['抽取音轨', 'XiaomiMimo ASR 转写', 'Agnes 六段式生成', '写入影片库'][i] });
        }
      }, 300 + i * 500));
      return taskId as any;
    }

    default:
      throw new Error(`未知命令: ${cmd}`);
  }
}
