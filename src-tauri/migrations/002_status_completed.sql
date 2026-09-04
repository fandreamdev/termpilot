BEGIN;
UPDATE sftp_operations SET status='completed' WHERE status='succeeded';
CREATE UNIQUE INDEX IF NOT EXISTS idx_credential_host_kind ON credential_refs(host_id,kind);
COMMIT;
