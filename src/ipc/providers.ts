// VideosFlow 前端 IPC 高层封装（Provider 配置 + 连接测试 + 任务提交）
import { invoke, createChannel } from './client';
import type { ProviderRow, ProgressMsg } from './types';

export type { ProviderRow, ProgressMsg } from './types';

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
