import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { Envelope, Host, Session } from '../types';

const mock = import.meta.env.DEV && !('__TAURI_INTERNALS__' in window);
type MockPayload = Record<string, unknown>;
const mockHosts: Host[] = [
  { id: 'h-prod', name: 'prod-api-01', connection_type: 'direct_ssh', address: '10.0.10.21', port: 22, username: 'ops', auth_method: 'ssh_agent', group_name: '生产', is_production: true },
  { id: 'h-test', name: 'test-api-02', connection_type: 'direct_ssh', address: '10.0.20.12', port: 22, username: 'tester', auth_method: 'private_key', group_name: '测试', is_production: false },
];
let mockSession: Session | undefined;
const mockFiles: Record<string, { name: string; kind: string; content?: string }[]> = {
  '~': [
    { name: 'releases', kind: 'directory' },
    { name: 'app.conf', kind: 'file', content: '# mock configuration\nPORT=8080\n' },
    { name: 'release.tar.gz', kind: 'file' },
    { name: 'README.md', kind: 'file', content: 'TermPilot isolated mock SFTP file\n' },
  ],
  '~/releases': [{ name: '2026-09-04', kind: 'directory' }],
};
const mockSettings: Record<string, string> = { theme: 'dark', 'remote.default_path': '~' };
const mockTransfers = new Map<string, { status: string; progress: number }>();
const mockListeners = new Map<string, Set<(payload: unknown) => void>>();
let cachedPolicyVersion = 1;
const mockEnvelope = <T>(data: T): Envelope<T> => ({ ok: true, request_id: crypto.randomUUID(), data, error: null });
const emitMock = (event: string, payload: unknown) => mockListeners.get(event)?.forEach(handler => handler(payload));
const mockCall = async <T>(command: string, request: MockPayload): Promise<Envelope<T>> => {
  const sessionId = String(request.session_id ?? mockSession?.id ?? '');
  switch (command) {
    case 'host_list': return mockEnvelope(mockHosts.filter(host => !request.query || `${host.name} ${host.address} ${host.username}`.toLowerCase().includes(String(request.query).toLowerCase())) as T);
    case 'host_upsert': {
      const host = request as unknown as Host;
      const index = mockHosts.findIndex(item => item.id === host.id);
      if (index >= 0) mockHosts[index] = { ...mockHosts[index], ...host };
      else mockHosts.push({ ...host, id: host.id || crypto.randomUUID() });
      return mockEnvelope((host.id || mockHosts[mockHosts.length - 1].id) as T);
    }
    case 'host_delete': {
      const index = mockHosts.findIndex(item => item.id === request.id);
      if (index >= 0) mockHosts.splice(index, 1);
      return mockEnvelope(true as T);
    }
    case 'credential_store': return mockEnvelope({ credential_ref: crypto.randomUUID(), kind: request.kind } as T);
    case 'session_connect': {
      mockSession = { id: crypto.randomUUID(), host_id: String(request.host_id), status: 'ready', started_at: new Date().toISOString() };
      emitMock('session.status', { session_id: mockSession.id, data: { status: 'ready' } });
      return mockEnvelope(mockSession as T);
    }
    case 'session_disconnect':
    case 'session_cancel': mockSession = undefined; return mockEnvelope(true as T);
    case 'session_send_input': emitMock('session.output', { session_id: sessionId, data: { bytes_base64: request.bytes_base64 } }); return mockEnvelope({ accepted_bytes: 1 } as T);
    case 'session_resize': return mockEnvelope({ status: 'ready' } as T);
    case 'sftp_list': return mockEnvelope({ path: String(request.path ?? '~'), entries: (mockFiles[String(request.path ?? '~')] ?? []).map(({ name, kind }) => ({ name, kind })), next_cursor: null } as T);
    case 'read_remote_file': {
      const path = String(request.path ?? ''); const name = path.split('/').pop() ?? '';
      const file = Object.values(mockFiles).flat().find(item => item.name === name && item.content !== undefined);
      return file ? mockEnvelope({ content: file.content, content_hash: 'mock', truncated: false, model_safe: true } as T) : mockEnvelope({ content: 'mock file content\n', content_hash: 'mock', truncated: false, model_safe: true } as T);
    }
    case 'sftp_transfer_start': {
      const id = crypto.randomUUID(); const op = String(request.op); const status = 'completed'; mockTransfers.set(id, { status, progress: 100 });
      if (op === 'mkdir') { const path = String(request.dst ?? '~'); const parent = path.substring(0, path.lastIndexOf('/')) || '~'; mockFiles[parent] = [...(mockFiles[parent] ?? []), { name: path.split('/').pop() ?? 'new-dir', kind: 'directory' }]; }
      emitMock('transfer.progress', { session_id: sessionId, data: { transfer_id: id, status, transferred_bytes: 100, size_bytes: 100 } });
      return mockEnvelope({ transfer_id: id, status } as T);
    }
    case 'transfer_pause': case 'transfer_resume': case 'transfer_cancel': {
      const id = String(request.transfer_id); const status = command === 'transfer_pause' ? 'paused' : command === 'transfer_cancel' ? 'cancelled' : 'running'; mockTransfers.set(id, { status, progress: mockTransfers.get(id)?.progress ?? 0 }); return mockEnvelope({ transfer_id: id, status } as T);
    }
    case 'transfer_retry': { const id = crypto.randomUUID(); mockTransfers.set(id, { status: 'completed', progress: 100 }); return mockEnvelope({ transfer_id: id, status: 'completed' } as T); }
    case 'policy_get': return mockEnvelope({ policy_id: 'default', mode: 'ask_before_execute', version: 1, allow_rules: [], fixed_readonly: [['df', '-h'], ['pwd'], ['whoami']] } as T);
    case 'agent_message_send': return mockEnvelope({ task_id: crypto.randomUUID(), status: 'completed', response: '我会先读取脱敏上下文，并仅建议固定只读命令。' } as T);
    case 'agent_cancel': return mockEnvelope({ task_id: request.task_id, status: 'cancelled' } as T);
    case 'audit_list': return mockEnvelope({ events: [], limit: request.limit ?? 200 } as T);
    case 'audit_export': return mockEnvelope({ export_id: crypto.randomUUID(), path: 'mock/audit.jsonl', manifest_path: 'mock/audit.manifest.json', event_count: 0, file_hash: 'mock', manifest_hash: 'mock' } as T);
    case 'audit_export_verify': return mockEnvelope({ valid: true, file_hash: 'mock', bytes: 0, manifest_path: request.path } as T);
    case 'app_settings_get': return mockEnvelope({ settings: Object.entries(mockSettings).map(([key, value]) => ({ key, value, value_type: 'string' })) } as T);
    case 'app_settings_set': mockSettings[String(request.key)] = String(request.value); return mockEnvelope({ key: request.key } as T);
    case 'database_backup': return mockEnvelope({ path: request.path, file_hash: 'mock' } as T);
    case 'database_restore': return mockEnvelope({ restored: true } as T);
    case 'emergency_stop': return mockEnvelope(true as T);
    case 'emergency_stop_clear': return mockEnvelope(true as T);
    default: return mockEnvelope({} as T);
  }
};
const call = async <T>(command: string, request: unknown = {}): Promise<Envelope<T>> => {
  if (!mock) return invoke<Envelope<T>>(command, { request });
  return mockCall<T>(command, request as MockPayload);
};
const toolRequest = (request: Record<string, unknown>) => ({
  request_id: crypto.randomUUID(),
  policy_version: cachedPolicyVersion,
  deadline: new Date(Date.now() + 5 * 60_000).toISOString(),
  ...request,
});

export const api = {
  hosts: async (): Promise<Host[]> => {
    if (mock) return mockHosts;
    return (await call<Host[]>('host_list', { page_size: 200 })).data ?? [];
  },
  connect: async (host_id: string, fingerprint_confirmation?: string | boolean): Promise<Session> => {
    if (mock) { return (await mockCall<Session>('session_connect', { host_id })).data as Session; }
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
  transferRetry: (transfer_id: string, confirmed = false, resume = true) => call('transfer_retry', { transfer_id, confirmed, resume, resume_confirmed: confirmed }),
  policyGet: async () => { const response = await call('policy_get'); const version = (response.data as { version?: number } | null)?.version; if (typeof version === 'number' && version > 0) cachedPolicyVersion = version; return response; },
  policyAllowRuleUpsert: (request: unknown) => call('policy_allow_rule_upsert', request),
  getTerminalContext: (session_id: string, policy_version = 1) => call('get_terminal_context', toolRequest({ session_id, policy_version })),
  runReadOnlyCommand: (request: Record<string, unknown>) => call('run_read_only_command', toolRequest(request)),
  proposeCommand: (request: Record<string, unknown>) => call('propose_command', toolRequest(request)),
  executeApprovedCommand: (approval_id: string, session_id: string, policy_version = 1) => call('execute_approved_command', toolRequest({ approval_id, session_id, policy_version })),
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
  on: <T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> => {
    if (!mock) return listen<T>(event, e => handler(e.payload));
    const set = mockListeners.get(event) ?? new Set<(payload: unknown) => void>(); set.add(handler as (payload: unknown) => void); mockListeners.set(event, set);
    return Promise.resolve(() => { set.delete(handler as (payload: unknown) => void); });
  },
  emergencyStop: () => call('emergency_stop', { scope: 'all', reason: '用户触发急停' }),
  emergencyStopClear: () => call('emergency_stop_clear', { confirmed: true }),
};
