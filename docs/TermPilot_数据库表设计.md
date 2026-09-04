# TermPilot 个人版数据库表设计

版本：2.0  
日期：2026-09-04  
范围：单用户、本地 SQLite

数据库只保存非秘密业务数据。密码、私钥、Token、API Key、完整终端输出和文件正文不进入 SQLite、WAL、SHM、日志或审计正文。模型配置为单一用户级 TOML，不在数据库缓存。

## 1. 数据库位置和约定

路径：`%LOCALAPPDATA%\\TermPilot\\data\\termpilot.db`。启用 WAL、外键和 busy timeout；时间为 UTC RFC3339；ID 为 UUID/ULID；删除主机使用软删除。

审计哈希：`hash = SHA256(canonical_json(event_without_hash_fields) || prev_hash)`；第一条事件的 `prev_hash` 为 64 个零字符。审计写入串行化，授权事件必须先于远端执行提交。

个人版不创建 `model_profile_cache`、项目配置、企业审计或多用户相关表。

## 2. 表结构

### `schema_migrations`

```sql
CREATE TABLE schema_migrations(
  version INTEGER PRIMARY KEY,
  checksum TEXT NOT NULL,
  applied_at TEXT NOT NULL
);
```

### `app_settings`

保存主题、快捷键、默认 workspace、审计保留天数和单一模型配置路径等非秘密设置。禁止保存密码、Token 或 API Key。

```sql
CREATE TABLE app_settings(
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL,
  value_type TEXT NOT NULL CHECK(value_type IN('string','integer','boolean','json')),
  updated_at TEXT NOT NULL
);
```

### `security_policies`

个人版只保留一个活动策略。规则 JSON 必须由 Rust Schema 校验。

```sql
CREATE TABLE security_policies(
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL COLLATE NOCASE,
  mode TEXT NOT NULL CHECK(mode IN('readonly','ask_before_execute','allow_safe_commands','manual_only')),
  allow_rules_json TEXT NOT NULL DEFAULT '[]',
  deny_rules_json TEXT NOT NULL DEFAULT '[]',
  limits_json TEXT NOT NULL DEFAULT '{}',
  version INTEGER NOT NULL CHECK(version>0),
  is_active INTEGER NOT NULL DEFAULT 1 CHECK(is_active IN(0,1)),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_policy_active ON security_policies(is_active) WHERE is_active=1;
```

### `hosts`

```sql
CREATE TABLE hosts(
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL COLLATE NOCASE,
  connection_type TEXT NOT NULL CHECK(connection_type IN('direct_ssh','bastion_endpoint')),
  address TEXT NOT NULL,
  port INTEGER NOT NULL CHECK(port BETWEEN 1 AND 65535),
  username TEXT NOT NULL,
  auth_method TEXT NOT NULL CHECK(auth_method IN('password','private_key','ssh_agent')),
  group_name TEXT,
  is_production INTEGER NOT NULL DEFAULT 0 CHECK(is_production IN(0,1)),
  workspace_root TEXT,
  endpoint_fingerprint TEXT,
  remote_identity_hmac TEXT,
  policy_id TEXT NOT NULL,
  notes TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  FOREIGN KEY(policy_id) REFERENCES security_policies(id)
);
CREATE UNIQUE INDEX idx_hosts_name_active ON hosts(name) WHERE deleted_at IS NULL;
```

### `credential_refs`

只保存 Credential Manager target 或 SSH Agent 标识，不保存秘密正文。

```sql
CREATE TABLE credential_refs(
  id TEXT PRIMARY KEY,
  host_id TEXT NOT NULL,
  kind TEXT NOT NULL CHECK(kind IN('password','private_key','ssh_agent')),
  target_name TEXT NOT NULL,
  secret_location TEXT NOT NULL CHECK(secret_location IN('windows_credential_manager','user_file','ssh_agent')),
  retention_mode TEXT NOT NULL CHECK(retention_mode IN('never','app_session')),
  created_at TEXT NOT NULL,
  last_used_at TEXT,
  revoked_at TEXT,
  FOREIGN KEY(host_id) REFERENCES hosts(id) ON DELETE CASCADE,
  UNIQUE(host_id,kind)
);
```

### `sessions`

```sql
CREATE TABLE sessions(
  id TEXT PRIMARY KEY,
  host_id TEXT NOT NULL,
  status TEXT NOT NULL CHECK(status IN('connecting','ready','reconnecting','disconnected','closed','error')),
  observed_endpoint_fingerprint TEXT,
  observed_remote_identity_hmac TEXT,
  pty_rows INTEGER CHECK(pty_rows BETWEEN 1 AND 1000),
  pty_cols INTEGER CHECK(pty_cols BETWEEN 1 AND 1000),
  started_at TEXT NOT NULL,
  ended_at TEXT,
  disconnect_reason TEXT,
  FOREIGN KEY(host_id) REFERENCES hosts(id)
);
```

### `agent_conversations`、`agent_messages`

保存脱敏后的问题、回答和结构化工具调用。模型不可用时仍保留人工建议。

```sql
CREATE TABLE agent_conversations(
  id TEXT PRIMARY KEY, session_id TEXT NOT NULL, model_provider TEXT,
  status TEXT NOT NULL DEFAULT 'active' CHECK(status IN('active','completed','cancelled','error')),
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
  FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);
CREATE TABLE agent_messages(
  id INTEGER PRIMARY KEY AUTOINCREMENT, conversation_id TEXT NOT NULL,
  role TEXT NOT NULL CHECK(role IN('system','user','assistant','tool')),
  content TEXT, tool_call_json TEXT, created_at TEXT NOT NULL,
  CHECK(content IS NOT NULL OR tool_call_json IS NOT NULL),
  FOREIGN KEY(conversation_id) REFERENCES agent_conversations(id) ON DELETE CASCADE
);
```

### `command_approvals`、`execution_records`

审批票据一次性使用，并绑定主机、用户、cwd、命令哈希和策略版本。

```sql
CREATE TABLE command_approvals(
  id TEXT PRIMARY KEY, session_id TEXT NOT NULL, policy_id TEXT NOT NULL,
  argv_json TEXT NOT NULL, cwd TEXT, command_hash TEXT NOT NULL,
  risk TEXT NOT NULL CHECK(risk IN('low','medium','high','critical','blocked')),
  policy_version INTEGER NOT NULL, status TEXT NOT NULL CHECK(status IN('pending','approved','rejected','expired','consumed')),
  created_at TEXT NOT NULL, decided_at TEXT, expires_at TEXT NOT NULL,
  FOREIGN KEY(session_id) REFERENCES sessions(id), FOREIGN KEY(policy_id) REFERENCES security_policies(id)
);
CREATE TABLE execution_records(
  id TEXT PRIMARY KEY, session_id TEXT NOT NULL, approval_id TEXT,
  authorization_type TEXT NOT NULL CHECK(authorization_type IN('approval','policy_allowlist','dry_run')),
  allow_rule_id TEXT, policy_version INTEGER, command_hash TEXT NOT NULL,
  started_at TEXT NOT NULL, ended_at TEXT,
  status TEXT NOT NULL CHECK(status IN('running','succeeded','failed','cancelled','blocked')),
  exit_code INTEGER, stdout_hash TEXT, stderr_hash TEXT, output_bytes INTEGER NOT NULL DEFAULT 0,
  FOREIGN KEY(session_id) REFERENCES sessions(id), FOREIGN KEY(approval_id) REFERENCES command_approvals(id)
);
```

### `sftp_operations`

```sql
CREATE TABLE sftp_operations(
  id TEXT PRIMARY KEY, session_id TEXT NOT NULL,
  operation TEXT NOT NULL CHECK(operation IN('list','read','upload','download','delete','rename','mkdir')),
  source_path TEXT, destination_path TEXT, size_bytes INTEGER,
  transferred_bytes INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL CHECK(status IN('queued','running','paused','completed','failed','cancelled')),
  overwrite_confirmed INTEGER NOT NULL DEFAULT 0 CHECK(overwrite_confirmed IN(0,1)),
  content_hash TEXT, error_code TEXT, created_at TEXT NOT NULL, started_at TEXT, ended_at TEXT,
  FOREIGN KEY(session_id) REFERENCES sessions(id)
);
```

### `audit_logs`、`audit_exports`

```sql
CREATE TABLE audit_logs(
  id INTEGER PRIMARY KEY AUTOINCREMENT, event_id TEXT NOT NULL UNIQUE,
  event_type TEXT NOT NULL, severity TEXT NOT NULL CHECK(severity IN('info','warning','error','critical')),
  actor TEXT NOT NULL, target_host_id TEXT, session_id TEXT, correlation_id TEXT,
  payload_json TEXT NOT NULL, prev_hash TEXT, hash TEXT NOT NULL, created_at TEXT NOT NULL,
  FOREIGN KEY(target_host_id) REFERENCES hosts(id), FOREIGN KEY(session_id) REFERENCES sessions(id)
);
CREATE TABLE audit_exports(
  id TEXT PRIMARY KEY, format TEXT NOT NULL CHECK(format='jsonl'), filter_json TEXT NOT NULL DEFAULT '{}',
  event_count INTEGER NOT NULL DEFAULT 0, file_hash TEXT, manifest_hash TEXT, output_path TEXT,
  status TEXT NOT NULL CHECK(status IN('running','succeeded','failed')), created_at TEXT NOT NULL, completed_at TEXT
);
```

## 3. 迁移和验收

首版迁移创建以上表和默认 `ask_before_execute` 策略。删除或损坏数据库时，应用只允许查看诊断和重新初始化，不得自动执行远程命令。验收重点是秘密零落盘、指纹变化阻断、审批不可重放、审计链可验证和 SFTP 路径不越界。
