export type Host = {
  id: string; name: string; connection_type: 'direct_ssh' | 'bastion_endpoint'; address: string;
  port: number; username: string; auth_method: 'password' | 'private_key' | 'ssh_agent';
  group_name?: string; is_production: boolean; endpoint_fingerprint?: string;
};
export type Envelope<T> = { ok: boolean; request_id: string; data: T | null; error: { code: string; message: string } | null };
export type Session = { id: string; host_id: string; status: string; started_at: string };
