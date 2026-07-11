// VideosFlow 前端 IPC 类型定义

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
