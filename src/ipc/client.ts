// VideosFlow 前端 IPC 客户端
// 双模式：
//  - Tauri 运行时：走 window.__TAURI__.core.invoke + Tauri Channel（进度推送）
//  - 普通浏览器（npm run dev）：localStorage 回退，保证 UI 可点验

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

function seedIfEmpty(): MockProvider[] {
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

function delay(ms: number) {
  return new Promise((r) => setTimeout(r, ms));
}

async function mockInvoke<T>(cmd: string, args?: InvokeArgs): Promise<T> {
  switch (cmd) {
    case 'ping':
      return 'pong' as any;
    case 'provider_list': {
      const list = seedIfEmpty();
      return list as any;
    }
    case 'provider_upsert': {
      const a = (args || {}) as any;
      const list = seedIfEmpty();
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
      const a = (args || {}) as any;
      const keys = readJSON<Record<string, string>>(LS_KEYS, {});
      if (a.key) keys[a.kind] = a.key;
      else delete keys[a.kind];
      writeJSON(LS_KEYS, keys);
      const list = seedIfEmpty();
      const p = list.find((x) => x.kind === a.kind);
      if (p) {
        p.hasKey = !!a.key;
        writeJSON(LS_PROVIDERS, list);
      }
      return undefined as any;
    }
    case 'provider_key_get': {
      const a = (args || {}) as any;
      const keys = readJSON<Record<string, string>>(LS_KEYS, {});
      return (keys[a.kind] || null) as any;
    }
    case 'provider_test': {
      await delay(700);
      const a = (args || {}) as any;
      const list = seedIfEmpty();
      const p = list.find((x) => x.kind === a.kind);
      if (p) {
        p.hasKey = true;
        writeJSON(LS_PROVIDERS, list);
      }
      return 'ok' as any; // 浏览器回退：模拟连接成功
    }
    case 'task_submit': {
      const a = (args || {}) as any;
      const taskId = 'mock-' + Math.random().toString(36).slice(2);
      const ch = a.on_progress;
      const kind = a.kind;
      if (ch && ch.__mockChannel && ch._on) {
        if (kind === 'chat') {
          // 浏览器回退：模拟真实 Agnes 对话链路（无 Rust/sidecar 时 UI 可点验）
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
      const a = (args || {}) as any;
      return { id: a.id, status: 'done', progress: 100, log: '完成（mock）' } as any;
    }
    default:
      throw new Error(`未知命令: ${cmd}`);
  }
}
