import { invoke } from '@tauri-apps/api/core';
import type { Envelope, Host, Session } from '../types';

const mock = import.meta.env.DEV && !('__TAURI_INTERNALS__' in window);
const call = async <T>(command: string, request: unknown = {}): Promise<Envelope<T>> => {
  if (!mock) return invoke<Envelope<T>>(command, { request });
  return { ok: true, request_id: crypto.randomUUID(), data: null, error: null };
};

export const api = {
  hosts: async (): Promise<Host[]> => {
    if (mock) return [{ id: 'h-prod', name: 'prod-api-01', connection_type: 'direct_ssh', address: '10.0.10.21', port: 22, username: 'ops', auth_method: 'ssh_agent', group_name: '生产', is_production: true }, { id: 'h-test', name: 'test-api-02', connection_type: 'direct_ssh', address: '10.0.20.12', port: 22, username: 'tester', auth_method: 'private_key', group_name: '测试', is_production: false }];
    return (await call<Host[]>('host_list', { page_size: 200 })).data ?? [];
  },
  connect: async (host_id: string): Promise<Session> => {
    if (mock) return { id: crypto.randomUUID(), host_id, status: 'ready', started_at: new Date().toISOString() };
    return (await call<Session>('session_connect', { host_id, pty: { rows: 30, cols: 120 } })).data!;
  },
  emergencyStop: () => call('emergency_stop', { scope: 'all', reason: '用户触发急停' }),
};
