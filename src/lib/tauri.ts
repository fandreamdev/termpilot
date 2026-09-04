import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { Envelope, Host, Session } from '../types';

const mock = import.meta.env.DEV && !('__TAURI_INTERNALS__' in window);
const call = async <T>(command: string, request: unknown = {}): Promise<Envelope<T>> => {
  if (!mock) return invoke<Envelope<T>>(command, { request });
  return { ok: true, request_id: crypto.randomUUID(), data: null, error: null };
};
const toolRequest = (request: Record<string, unknown>) => ({
  request_id: crypto.randomUUID(),
  policy_version: 1,
  deadline: new Date(Date.now() + 5 * 60_000).toISOString(),
  ...request,
});

export const api = {
  hosts: async (): Promise<Host[]> => {
    if (mock) return [{ id: 'h-prod', name: 'prod-api-01', connection_type: 'direct_ssh', address: '10.0.10.21', port: 22, username: 'ops', auth_method: 'ssh_agent', group_name: '生产', is_production: true }, { id: 'h-test', name: 'test-api-02', connection_type: 'direct_ssh', address: '10.0.20.12', port: 22, username: 'tester', auth_method: 'private_key', group_name: '测试', is_production: false }];
    return (await call<Host[]>('host_list', { page_size: 200 })).data ?? [];
  },
  connect: async (host_id: string, fingerprint_confirmation: string | boolean = true): Promise<Session> => {
    if (mock) return { id: crypto.randomUUID(), host_id, status: 'ready', started_at: new Date().toISOString() };
    const response = await call<Session>('session_connect', { host_id, fingerprint_confirmation, pty: { rows: 30, cols: 120 } });
    if (!response.ok || !response.data) throw new Error(response.error?.message ?? '连接失败');
    return response.data;
  },
  hostUpsert: (request: Record<string, unknown>) => call<string>('host_upsert', request),
  hostDelete: (id: string) => call<boolean>('host_delete', { id }),
  credentialStore: (request: unknown) => call<{ credential_ref: string }>('credential_store', request),
  sessionSendInput: (request: { session_id: string; bytes_base64: string }) => call('session_send_input', request),
  sessionResize: (request: { session_id: string; rows: number; cols: number }) => call('session_resize', request),
  sessionDisconnect: (request: { session_id: string; reason?: string }) => call('session_disconnect', request),
  sessionCancel: (request: { session_id: string; reason?: string }) => call('session_cancel', request),
  sftpList: (request: { session_id: string; path?: string; limit?: number; cursor?: string }) => call('sftp_list', { ...request, path: request.path ?? '~' }),
  sftpTransferStart: (request: unknown) => call<{ transfer_id: string; status: string }>('sftp_transfer_start', request),
  listRemoteDirectory: (request: { session_id: string; path?: string; limit?: number }) => call('list_remote_directory', toolRequest({ ...request, path: request.path ?? '~' })),
  readRemoteFile: (request: { session_id: string; path: string; max_bytes?: number }) => call('read_remote_file', toolRequest(request)),
  uploadFile: (request: Record<string, unknown>) => call('upload_file', toolRequest(request)),
  downloadFile: (request: Record<string, unknown>) => call('download_file', toolRequest(request)),
  transferPause: (transfer_id: string) => call('transfer_pause', { transfer_id }),
  transferResume: (transfer_id: string) => call('transfer_resume', { transfer_id }),
  transferCancel: (transfer_id: string) => call('transfer_cancel', { transfer_id }),
  transferRetry: (transfer_id: string, confirmed = false) => call('transfer_retry', { transfer_id, confirmed }),
  policyGet: () => call('policy_get'),
  policyAllowRuleUpsert: (request: unknown) => call('policy_allow_rule_upsert', request),
  getTerminalContext: (session_id: string) => call('get_terminal_context', { session_id }),
  runReadOnlyCommand: (request: Record<string, unknown>) => call('run_read_only_command', toolRequest(request)),
  proposeCommand: (request: Record<string, unknown>) => call('propose_command', toolRequest(request)),
  executeApprovedCommand: (approval_id: string, policy_version = 1) => call('execute_approved_command', toolRequest({ approval_id, policy_version })),
  agentMessageSend: (request: unknown) => call('agent_message_send', request),
  agentCancel: (task_id: string, reason?: string) => call('agent_cancel', { task_id, reason }),
  approvalDecide: (approval_id: string, decision: 'approve' | 'reject', phrase?: string) => call('approval_decide', { approval_id, decision, phrase }),
  auditExport: (request: unknown = {}) => call('audit_export', request),
  auditExportVerify: (path: string) => call('audit_export_verify', { path }),
  auditList: (limit = 200) => call('audit_list', { limit }),
  appSettingsGet: () => call('app_settings_get'),
  appSettingsSet: (request: unknown) => call('app_settings_set', request),
  databaseBackup: (path: string) => call('database_backup', { path }),
  databaseRestore: (path: string, confirmed = false) => call('database_restore', { path, confirmed }),
  on: <T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> => mock ? Promise.resolve(() => undefined) : listen<T>(event, e => handler(e.payload)),
  emergencyStop: () => call('emergency_stop', { scope: 'all', reason: '用户触发急停' }),
  emergencyStopClear: () => call('emergency_stop_clear', { confirmed: true }),
};
