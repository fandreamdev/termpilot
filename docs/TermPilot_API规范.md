# TermPilot API 规范（个人部署版）

版本：1.0（2026-09-03）  
适用范围：Windows 单用户、个人部署、Tauri 2 + React/TypeScript + Rust Core。本文档依据《TermPilot_需求分析与系统设计》和《TermPilot_数据库表设计》编写。

## 1. 范围与边界

本文档定义 React ↔ Tauri command/event、Rust 内部 trait、AI 工具以及本地配置接口。个人版通过 SSH/SFTP 访问直连主机或 SSH 堡垒机；不包含 SIEM、企业 CA/mTLS、集中审计、组织策略或多用户共享。

示例堡垒机连接必须结构化为：

```text
host = jtdcblj2.zhenergy.com.cn
port = 8022
username = u890374m2
```

等价 OpenSSH 命令是 `ssh -p 8022 u890374m2@jtdcblj2.zhenergy.com.cn`。`8022` 始终是端口字段，禁止作为远程命令拼接。`https://jtdcblj.zhenergy.com.cn` 仅作为网页管理入口，不能自动作为 SSH 地址。

## 2. 通用协议约定

### 2.1 调用封装

Tauri command 使用 `invoke(command, request)`；请求可选携带 `request_id`，响应统一为：

```json
{"ok":true,"request_id":"01J...","data":{},"error":null}
```

失败响应：

```json
{"ok":false,"request_id":"01J...","data":null,
 "error":{"code":"SSH_TIMEOUT","message":"连接超时","retryable":true,
           "details":{"phase":"handshake"}}}
```

`request_id` 为 UUIDv4 或 ULID，1–64 个 ASCII 字符；重试必须复用同一 ID。所有时间为 RFC3339 UTC（如 `2026-09-03T08:00:00Z`），金额/大小使用整数，ID 使用 UUID/ULID 字符串。

逻辑 URL 的 HTTP 语义（仅用于适配器和测试）：成功查询为 `200`，创建/异步任务为 `201` 或 `202`，参数错误 `400`，未认证 `401`，当前用户无权 `403`，资源不存在 `404`，幂等冲突/状态冲突 `409`，超时 `408`，策略阻断仍使用 `403` 并在错误码中说明原因，内部故障 `500`。Tauri invoke 不返回 HTTP 状态，而是在统一封装的 `error.code` 中表达同样语义。

### 2.2 安全和校验总则

* 密码、私钥正文、私钥口令、Token、Cookie、API Key、凭据包原文及其普通哈希不得出现在响应、日志、SQLite、审计或模型请求中。只能返回随机 `credential_ref`、`bundle_handle` 或脱敏摘要。
* SSH 地址采用主机名或 IP（1–253 字符）；禁止 URL、路径、控制字符和 shell 元字符。端口为 1–65535，默认 SSH 端口 22，堡垒机示例为 8022。
* 所有 SSH/SFTP 参数以结构化字段传给 Rust；禁止拼接 PowerShell/cmd/bash 字符串。远端命令必须是 `program + args[]`。
* 文本字段使用 UTF-8；除特别说明外最大 4096 字节；JSON 深度 ≤20、单请求 ≤1 MiB。路径必须规范化并限制在用户选择的工作区；拒绝 `..` 越界、UNC 路径和 NUL。
* 长任务返回任务 ID，并支持 cancellation token。取消是尽力而为，服务端仍需写入最终状态和审计事件。网络重试最多 2 次指数退避（1s、2s、4s），不得重复执行非幂等命令。
* 本地审计写入失败时，Agent 自动执行、策略变更和凭据持久化必须阻断（`AUDIT_UNAVAILABLE`）。

### 2.3 错误对象和错误码

错误码格式为 `DOMAIN_REASON`。通用码：`VALIDATION`、`NOT_FOUND`、`CONFLICT`、`FORBIDDEN`、`UNAUTHORIZED`、`TIMEOUT`、`CANCELLED`、`INTERNAL`。领域码包括：`HOST_INVALID_*`、`BUNDLE_MISSING/DUPLICATE/INVALID_*`、`SSH_HOSTKEY_CHANGED`、`SSH_AUTH_FAILED`、`SSH_TIMEOUT`、`CREDENTIAL_EXPIRED/UNAVAILABLE`、`SESSION_CLOSED`、`PATH_ESCAPE`、`SFTP_CONFLICT`、`POLICY_BLOCKED/POLICY_INVALID/REAUTH_REQUIRED`、`APPROVAL_EXPIRED`、`MODEL_CONFIG_INVALID/MODEL_UNAVAILABLE/MODEL_RATE_LIMIT`、`EGRESS_POLICY_BLOCKED`、`AUDIT_IO`、`VERIFY_HASH_MISMATCH`、`EMERGENCY_STOP_ACTIVE`。`message` 面向用户，`details` 只能包含脱敏诊断。

## 3. 公共数据类型

### 3.1 Host

```json
{"id":"host-uuid","name":"生产堡垒机","connection_type":"bastion_endpoint",
 "address":"jtdcblj2.zhenergy.com.cn","port":8022,"username":"u890374m2",
 "auth_method":"password","group_name":"zhenergy","is_production":true,
 "workspace_root":"D:/TermPilot/workspace","endpoint_fingerprint":"SHA256:…",
 "policy_id":"policy-uuid","created_at":"2026-09-03T08:00:00Z","updated_at":"…"}
```

`name` 1–128 字符且同一用户唯一；`connection_type` 为 `direct_ssh|bastion_endpoint`；`auth_method` 为 `password|private_key|ssh_agent`；`workspace_root` 为本地绝对路径；`is_production` 只能由用户明确设置，编辑连接参数不能自动清除。

### 3.2 Session、Transfer、Approval

Session 状态：`connecting|ready|reconnecting|disconnected|closed|error`；PTY 行列均为 1–1000。Transfer 操作：`upload|download|delete|rename|mkdir`，状态：`queued|running|paused|completed|cancelled|failed`。Approval 决策：`approve|reject`，票据只能使用一次且有过期时间。

## 4. React ↔ Tauri Command API

以下请求/响应中的 `data` 均位于通用成功封装内。桌面版实际调用是 `invoke(command, request)`，不启动公网 HTTP 监听；为便于接口评审、抓包模拟和未来本地适配器，定义统一逻辑 URL：`tauri://localhost/api/v1/{path}`。逻辑 URL 不应被浏览器或远程客户端直接访问。

### 4.0 URL、方法和数据方向总览

| Command | 方法 | 逻辑请求 URL | 数据方向 | 成功 data |
|---|---|---|---|---|
| `host_list/filter` | GET | `tauri://localhost/api/v1/hosts` | React → Rust | `{items,next_cursor}` |
| `host_upsert` | POST/PUT | `tauri://localhost/api/v1/hosts` | React → Rust | `Host` |
| `host_delete` | DELETE | `tauri://localhost/api/v1/hosts/{host_id}` | React → Rust/SQLite | `{id,deleted_at}` |
| `credential_bundle_import` | POST | `tauri://localhost/api/v1/credentials/bundles:import` | React → Rust | `{host_id,bundle_handle,masked_summary}` |
| `credential_store` | POST/DELETE | `tauri://localhost/api/v1/credentials` | React → Rust/Windows Credential Manager | `{credential_ref,…}` |
| `session_connect` | POST | `tauri://localhost/api/v1/sessions` | React → Rust → SSH | `{session_id,status}` |
| `session_cancel` | POST | `tauri://localhost/api/v1/sessions/{session_id}:cancel` | React → Rust | `{session_id,status}` |
| `session_send_input` | POST | `tauri://localhost/api/v1/sessions/{session_id}/input` | React → Rust → SSH | `{accepted_bytes,seq}` |
| `session_resize` | POST | `tauri://localhost/api/v1/sessions/{session_id}/resize` | React → Rust → SSH | `{rows,cols}` |
| `session_disconnect` | POST | `tauri://localhost/api/v1/sessions/{session_id}:disconnect` | React → Rust → SSH | `{status,ended_at}` |
| `sftp_transfer_start` | POST | `tauri://localhost/api/v1/transfers` | React → Rust → SFTP | `{transfer_id,status}` |
| `transfer_cancel` | POST | `tauri://localhost/api/v1/transfers/{transfer_id}:cancel` | React → Rust → SFTP | `Transfer` |
| `transfer_pause` | POST | `tauri://localhost/api/v1/transfers/{transfer_id}:pause` | React → Rust → SFTP | `Transfer` |
| `transfer_resume` | POST | `tauri://localhost/api/v1/transfers/{transfer_id}:resume` | React → Rust → SFTP | `Transfer` |
| `model_profile_list` | GET | `tauri://localhost/api/v1/model-profiles` | React → Rust → `.termpilot` | `{items}` |
| `model_profile_validate` | POST | `tauri://localhost/api/v1/model-profiles/{profile}:validate` | React → Rust → Model endpoint | `ValidationReport` |
| `model_egress_preview` | POST | `tauri://localhost/api/v1/model-egress/preview` | React → Rust | `{scope,redacted_preview,blocked_items}` |
| `agent_message_send` | POST | `tauri://localhost/api/v1/agent/tasks` | React → Agent | `{conversation_id,task_id,status}` |
| `agent_cancel` | POST | `tauri://localhost/api/v1/agent/tasks/{task_id}:cancel` | React → Agent | `{task_id,status}` |
| `policy_allow_rule_upsert` | POST/PUT | `tauri://localhost/api/v1/policies/allow-rules` | React → Policy Engine | `{rule_id,policy_version}` |
| `approval_decide` | POST | `tauri://localhost/api/v1/approvals/{approval_id}:decide` | React → Policy Engine | `Approval` |
| `audit_export` | POST | `tauri://localhost/api/v1/audit/exports` | React → Local SQLite | `{export_id,status}` |
| `audit_export_verify` | POST | `tauri://localhost/api/v1/audit/exports:verify` | React → Local SQLite | `VerificationReport` |
| `emergency_stop` | POST | `tauri://localhost/api/v1/system/emergency-stop` | React → Rust 全局开关 | `{stop_id,stopped_tasks}` |
| `emergency_stop_clear` | POST | `tauri://localhost/api/v1/system/emergency-stop:clear` | React → Rust 全局开关 | `{stop_id,cleared_at}` |
| `project_config_load` | GET | `tauri://localhost/api/v1/projects/config` | React → `.termpilot` reader | `{model_profile,settings}` |
| `project_config_validate` | POST | `tauri://localhost/api/v1/projects/config:validate` | React → `.termpilot` reader | `{valid,errors,warnings}` |

URL 路径参数必须先按 UUID/名称规则校验再解析；查询参数不得改变权限或绕过策略。表中“成功 data”均不包含密码、私钥、Token、Cookie、API Key 或凭据包原文。

下文每个接口的“请求（Request）”就是发送到 4.0 表中逻辑 URL 的 JSON body（GET 的字段为 query/path 参数），“响应（Response）”就是通用封装中的 `data`；错误时返回第 2.3 节的 `error` 对象。这样既可直接映射 Tauri `invoke`，也可由本地测试适配器映射为 HTTP 请求。

### 4.1 主机和凭据

#### `host_list/filter`（只读）

请求：`{ "group"?: string, "query"?: string, "include_deleted"?: false }`。字段长度分别 ≤128、≤256；`query` 按名称/地址/用户名模糊匹配；分页 `page_size` 1–200（默认 50）、`cursor` ≤128。

响应：`{ "items":[Host], "next_cursor":null }`。不存在的分组返回空数组，不报错。超时 5 秒，幂等。

成功案例：`{"group":"zhenergy","query":"jtdcblj2"}` → `{"items":[{"id":"h1","address":"jtdcblj2.zhenergy.com.cn","port":8022,…}],"next_cursor":null}`。

#### `host_upsert`

请求字段：`id?`（更新时必填，UUID）、`name`、`connection_type`、`address`、`port`、`username`、`auth_method`、`group_name?`、`is_production`、`workspace_root?`、`policy_id?`、`notes?`。名称 1–128；用户名 1–128；备注 ≤2000；端口和地址按 2.2 校验；`https://` 地址直接返回 `HOST_INVALID_ADDRESS`。

响应：保存后的 `Host`（不含秘密）。按 `id` 幂等，10 秒超时，不可取消。名称冲突返回 `CONFLICT`。

#### `host_delete`

请求：路径参数 `host_id`（UUID），Request 可带 `{reason?:string,reauth_token?:string}`；reason ≤512 字符，生产主机或存在活动会话时必须二次确认。响应：`{"id":"h1","deleted_at":"2026-09-03T08:10:00Z"}`。执行软删除，不物理删除审计和历史会话；重复删除幂等，活动会话不会被静默中断，返回 `HOST_IN_USE` 并要求先断开。

#### `credential_bundle_import`

请求：

```json
{"request_id":"r1","raw_bundle":"<仅在内存中传输的四字段文本>",
 "host_name":"生产堡垒机","is_production":true,
 "retention_mode":"app_session","unlock_policy":"current_user"}
```

`raw_bundle` 仅接受 `credential_bundle_v1`，≤16 KiB、CRLF/LF 均可；字段必须恰好四个、无重复/未知字段、值非空，解析后立即清零。`retention_mode`：`never|app_session|expires_at|persistent`；`expires_at` 模式必须另传合法未来 UTC 时间。响应不回显原文：

```json
{"host_id":"h1","bundle_handle":"bh_随机值","masked_summary":{"username":"u***2","host":"jtdc***cn","port":8022,"expires_at":null}}
```

错误：`BUNDLE_MISSING`、`BUNDLE_DUPLICATE`、`BUNDLE_INVALID_FORMAT/FIELD/HOST/PORT`。10 秒超时；相同 `request_id` 幂等；前端提交后清空剪贴板/表单。

#### `credential_store`

请求：`{host_id, kind, secret, retention_mode, unlock_policy, expires_at?}`。`kind=password|private_key|ssh_agent`；`secret` 仅进安全内存，最大 64 KiB，不能出现在 tracing；私钥可改为 `user_file` 路径引用。`ssh_agent` 不接受 secret 正文。需要当前 Windows 用户，Hello 策略需重新认证。

响应：`{"credential_ref":"cr_…","kind":"password","retention_mode":"app_session","expires_at":null}`。数据库仅保存 Credential Manager/DPAPI 引用；15 秒超时，按 `host_id+kind` 覆盖更新。删除可传 `retention_mode:"never"` 或调用同名撤销操作。

### 4.2 SSH 会话和终端

#### `session_connect`

请求：`{host_id, credential_ref?, bundle_handle?, fingerprint_confirmation?, pty?:{rows,cols}, request_id}`。`credential_ref` 与 `bundle_handle` 二选一；首次连接必须回传用户确认的 `fingerprint_confirmation`，rows/cols 1–1000（默认 24×80）。连接配置从 Host 读取，不能在此传 shell 命令。

响应：`{"session_id":"s1","status":"connecting","endpoint":{"address":"jtdcblj2.zhenergy.com.cn","port":8022},"correlation_id":"c1"}`，随后通过 `session.status` 通知 `ready` 或错误。45 秒超时；相同 request_id 复用；`session_cancel` 可取消握手。首次/变化指纹必须用户确认。

#### `session_cancel`

请求 `{session_id, reason?}`（reason ≤256）。响应 `{session_id,status:"closed",cancelled:true}`；仅 `connecting|reconnecting` 可取消，幂等。

#### `session_send_input`

请求 `{session_id, bytes}`，`bytes` 为 base64，单次 ≤64 KiB；禁止把凭据作为输入日志。响应 `{session_id,accepted_bytes,seq}`。仅 ready 会话，5 秒超时，不幂等。

#### `session_resize`

请求 `{session_id,rows,cols}`，均 1–1000；500 ms 合并旧事件。响应 `{session_id,rows,cols}`。ready 会话可调用，幂等。

#### `session_disconnect`

请求 `{session_id,reason?}`；响应 `{session_id,status:"closed",ended_at}`。重复调用成功返回当前状态；终端输出停止并写审计。

### 4.3 SFTP 传输

#### `sftp_transfer_start`

请求：

```json
{"session_id":"s1","op":"upload","src":"D:/TermPilot/a.txt",
 "dst":"/tmp/a.txt","overwrite":false,"resume":true,"transfer_id":"t1"}
```

`op` 为 `upload|download|delete|rename|mkdir`；本地 `src/dst` 必须位于用户批准的 workspace，远端路径 ≤4096 字符、规范化后不得越界；单文件默认 ≤20 GiB；`overwrite`、`resume` 为布尔。响应 `{ "transfer_id":"t1", "status":"queued", "correlation_id":"c1" }`。60 秒建任务，按 transfer_id 幂等；进度由事件推送。冲突返回 `SFTP_CONFLICT`，越界返回 `PATH_ESCAPE`。

#### `transfer_cancel` / `transfer_pause` / `transfer_resume`

请求均为 `{transfer_id,reason?}`。响应 `{transfer_id,status,bytes_done,total_bytes}`。取消幂等；暂停仅 `running`，恢复仅 `paused`，状态不符返回 `CONFLICT`。取消/暂停不删除可续传临时文件，最终状态必须审计。

### 4.4 模型 Profile 与外发策略

#### `model_profile_list`

请求 `{include_invalid?:false}`。响应仅公开非秘密字段：

```json
{"items":[{"name":"openai","provider":"openai-compatible","model":"gpt-4.1",
 "base_url":"https://api.example.com/v1","endpoint_scope":"Public",
 "validated_at":"2026-09-03T08:00:00Z","capabilities":{"stream":true}}]}
```

配置只从 `%USERPROFILE%\\.termpilot\\config.toml`、`profiles\\*.toml`、`auth.toml` 读取；`.codex` 永不读取。API Key 只显示 `auth_ref` 是否存在。5 秒、幂等。

#### `model_profile_validate`

请求 `{profile,probe?:["config","model","stream"]}`，`profile` 1–128 字符；可选探测不得超过 30 秒。响应 `{name,valid,errors:[],warnings:[],capabilities,normalized}`，`normalized` 不含认证值。非法 provider/model、Base URL、temperature（0–2）、timeout（5–600 秒）返回 `MODEL_CONFIG_INVALID`。

#### `model_egress_preview`

请求：`{profile,context_refs:[{type,id}],include_terminal:boolean,include_files:boolean}`；引用最多 100 个，文件片段单项 ≤64 KiB、总预览 ≤256 KiB。响应：

```json
{"scope":"Public","redacted_preview":{"terminal":"user=***\\nstatus=ok","files":[]},
 "blocked_items":[{"category":"credential","reason":"never_egress"}],"policy_id":"egress-default"}
```

密码、私钥、Token、Cookie、凭据包、Credential Manager 内容和命中禁止规则的数据始终阻断。10 秒、幂等。

### 4.5 Agent、策略和审批

#### `agent_message_send`

请求 `{conversation_id?,session_id,text,mode?,client_request_id}`；`text` 1–32 KiB；mode 为 `readonly|ask_before_execute|allow_safe_commands|manual_only`，默认读取主机策略。响应 `{conversation_id,task_id,status:"queued"}`；建任务 10 秒，client_request_id 幂等，`agent_cancel` 取消。模型不可用只返回手工建议，不执行命令。

#### `agent_cancel`

请求 `{task_id,reason?}`；响应 `{task_id,status:"cancelled"}`。取消模型流和未执行工具，正在运行的远程命令发送停止信号并等待安全收尾。

#### `policy_allow_rule_upsert`

请求：`{rule:{id?,program,args_schema,working_dir?,risk:"low",description},reauth_token}`。程序名只能是白名单 basename；参数 schema ≤32 个参数、禁止 shell 元字符和重定向；`working_dir` 必须为已批准路径。`reauth_token` 为当前 Windows 身份二次确认产生的一次性引用，不是密码。响应 `{rule_id,policy_version,updated_at}`。15 秒、按 rule id 幂等；校验失败 `POLICY_INVALID`。

#### `approval_decide`

请求 `{approval_id,decision,phrase?,authn?}`；decision `approve|reject`；高风险审批要求用户输入界面展示的短语（1–64 字符）并可要求 Hello/PIN。响应 `{approval_id,status:"approved"|"rejected",decided_at,policy_version}`。票据一次性、过期或策略版本变化返回 `APPROVAL_EXPIRED`/`POLICY_DENIED`。

### 4.6 审计和急停

#### `audit_export`

请求 `{filter:{from?,to?,host_id?,event_types?,risk?},format:"jsonl"|"csv",destination_dir}`。时间范围 ≤365 天；event_types ≤50；目标目录必须由用户选择且可写。响应 `{export_id,status:"queued",file_uri:null,manifest_uri:null}`，完成由 `audit.export_status` 通知。120 秒建任务；每次生成新 ID；不上传 SIEM。

#### `audit_export_verify`

请求 `{file_uri,manifest_uri}`，仅允许本地文件且路径 ≤4096。响应：

```json
{"valid":true,"event_count":128,"file_sha256":"hex64","chain":{"valid":true,"first_event_id":"e1","last_event_id":"e128"},"errors":[]}
```

任意文件或链修改返回 `VERIFY_HASH_MISMATCH`；60 秒，可取消，幂等。

#### `emergency_stop` / `emergency_stop_clear`

`emergency_stop` 请求 `{scope:"all"|"session"|"agent",session_id?,reason}`，reason 1–512 字符；响应 `{stop_id,scope,stopped_tasks,created_at}`，立即阻断新 Agent/SFTP/命令并尝试终止活动任务。`emergency_stop_clear` 请求 `{stop_id,authn}`，需要当前用户重新确认；响应 `{stop_id,cleared_at}`。急停期间执行接口返回 `EMERGENCY_STOP_ACTIVE`。

### 4.7 项目 `.termpilot` 配置

#### `project_config_load`

请求 `{project_root}`；根目录必须是用户选择的本地目录，禁止 UNC/越界。响应 `{project_root,model_profile,settings,policy_refs,source_path}`。只读取 `<project-root>\\.termpilot\\config.toml`，允许 `model_profile` 和非模型项目设置；不返回任何认证信息。

#### `project_config_validate`

请求 `{project_root,content_hash?}`；响应 `{valid,errors:[],warnings:[],model_profile,unknown_fields:[]}`。`model_profile` 1–128 字符且必须能在用户级 Profile 找到；项目文件不得包含 `provider`、`model`、`base_url`、`api_key`、`token`、`password`、私钥或 `auth_ref` 定义，发现即 `PROJECT_MODEL_FIELD_FORBIDDEN`。`.codex` 不作为回退来源。

## 5. AI 工具接口

Agent 只能通过以下 JSON Schema 风格工具调用，执行代理再次校验会话、策略版本、审批票据和急停状态。每个工具请求都必须包含公共字段：

```json
{"request_id":"01J…","session_id":"s1","policy_version":7,
 "deadline":"2026-09-03T08:05:00Z","arguments":{}}
```

`request_id` 1–64 字符且在任务内幂等；`session_id` 必须属于当前用户；`policy_version` 必须是当前活动策略；`deadline` 必须为未来 UTC 时间且不超过任务总时长 5 分钟。工具结果统一包含 `{status, risk, audit_id, truncated}`，命令类另含 `stdout/stderr` 摘要。工具结果会脱敏后返回模型；每次调用均写审计。严格 JSON Schema `additionalProperties=false`，禁止模型自定义工具。

工具逻辑 URL（仅 Agent 内部路由，不对前端开放）：

| 工具 | 方法 | 逻辑 URL | Request 主体 | Response 主体 |
|---|---|---|---|---|
| `get_terminal_context` | POST | `tauri://localhost/api/v1/tools/terminal-context` | 公共字段 + `{max_bytes,include_history}` | `{status,terminal_tail,redactions,truncated,audit_id}` |
| `run_read_only_command` | POST | `tauri://localhost/api/v1/tools/commands:read-only` | 公共字段 + `{program,args,cwd,timeout_seconds}` | `{status,exit_code,stdout,stderr,risk,audit_id}` |
| `propose_command` | POST | `tauri://localhost/api/v1/tools/commands:propose` | 公共字段 + `{argv,shell_preview,rationale,expected_impact}` | `{status,proposal_id,normalized_argv,risk,command_hash}` |
| `execute_approved_command` | POST | `tauri://localhost/api/v1/tools/commands:execute-approved` | 公共字段 + `{approval_id,command_hash,argv}` | `{status,execution_id,exit_code,stdout,stderr,risk,audit_id}` |
| `list_remote_directory` | POST | `tauri://localhost/api/v1/tools/sftp:list` | 公共字段 + `{path,page_token,recursive,limit}` | `{status,items,next_cursor,risk,audit_id}` |
| `read_remote_file` | POST | `tauri://localhost/api/v1/tools/sftp:read-file` | 公共字段 + `{path,max_bytes,offset}` | `{status,content,encoding,sha256,truncated,risk,audit_id}` |
| `upload_file` | POST | `tauri://localhost/api/v1/tools/sftp:upload` | 公共字段 + `{local_uri,remote_path,overwrite,resume}` | `{status,transfer_id,risk,audit_id}` |
| `download_file` | POST | `tauri://localhost/api/v1/tools/sftp:download` | 公共字段 + `{remote_path,local_uri,resume}` | `{status,transfer_id,risk,audit_id}` |

### `get_terminal_context`

请求参数：`{max_bytes?:number,include_history?:boolean,lines?:number}`；`max_bytes` 1–65536（默认 65536），`lines` 1–2000，`include_history` 默认 false。原始输出最多 64 KiB，按行截断并保留头尾，自动移除密码提示、Token、Cookie、环境变量秘密。响应 `{status:"ok",risk:"low",audit_id,session_id,terminal_tail,redactions:[{category,count}],captured_at,truncated}`。

### `run_read_only_command`

请求参数：`{program,args,cwd?,timeout_seconds?:number}`；仅允许策略白名单命令，`program` basename 1–128，args ≤32、单项 ≤1024，`cwd` 必须是批准的远端目录，timeout 1–60 秒；禁止 `;|&>$<`、换行、`sh -c`/动态脚本。响应 `{status:"completed"|"failed",risk:"low",audit_id,execution_id,exit_code,stdout,stderr,truncated,command_hash}`，输出各 ≤64 KiB，脱敏后才返回。请求示例：`{"request_id":"r1","session_id":"s1","policy_version":7,"deadline":"2026-09-03T08:01:00Z","arguments":{"program":"df","args":["-h"],"cwd":"/var","timeout_seconds":15}}`。

### `propose_command`

请求参数：`{argv,shell_preview?,rationale,expected_impact}`；`argv` 必须为非空数组（程序名 + ≤32 参数），同样执行 argv 校验；`shell_preview` 仅展示、≤4096 且不得作为执行输入；`rationale`、`expected_impact` 各 ≤2000。响应 `{status:"proposed",risk,requires_approval,audit_id,proposal_id,normalized_argv,policy_version,command_hash}`；只生成计划，不执行。`shell_preview` 含 shell 元字符时仍不得执行。

### `execute_approved_command`

请求参数：`{approval_id,command_hash,argv,timeout_seconds?}`；proposal、approval、policy 必须匹配且未过期；`argv` 必须与审批时规范化结果字节级一致；timeout 1–600 秒。响应 `{status:"completed"|"failed",risk,audit_id,execution_id,exit_code,stdout,stderr,truncated,command_hash}`。审计落盘失败或急停时阻断，禁止模型自行伪造 approval。审批缺失返回 `APPROVAL_REQUIRED`。

### `list_remote_directory`

请求参数：`{path,page_token?,recursive?:boolean,limit?:number}`；path ≤4096 且不得越界，`page_token` ≤256，limit 1–1000，recursive 深度由策略限制（默认 2）。响应 `{status:"completed",risk:"low",audit_id,path,items:[{name,type,size,mtime,mode}],next_cursor,truncated}`；文件名 UTF-8 ≤255，不回传内容。

### `read_remote_file`

请求参数：`{path,max_bytes?:number,offset?:number}`；`max_bytes` 1–32768（默认 32768），offset ≥0；二进制按 base64 返回。响应 `{status:"completed",risk:"low",audit_id,path,offset,length,content,encoding:"utf8"|"base64",truncated,sha256}`，敏感内容脱敏，不能把凭据文件发送给公网模型。

### `upload_file` / `download_file`

请求：`{local_uri,remote_path,overwrite?:boolean,resume?:boolean}`（`upload_file`）或 `{remote_path,local_uri,resume?:boolean}`（`download_file`）；本地路径必须在 workspace，远端路径按 SFTP 规则，文件默认 ≤20 GiB，覆盖生产主机需审批。响应 `{status:"queued",risk,audit_id,transfer_id}`，后续复用传输命令和进度事件。`local_uri` 仅允许用户批准的 `file:///` 路径，不接受任意 URL 或命令字符串。

## 6. Rust 内部接口

接口只在 Rust Core 内部可调用，没有 URL，不得通过 WebView 直接访问；React 只能调用第 4 节白名单 command。所有实现必须返回结构化错误并接受 cancellation token。

```rust
trait SshTransport {
    async fn connect(&self, cfg: SshConfig, cancel: CancellationToken) -> Result<SessionHandle>;
    async fn open_pty(&self, session: &SessionHandle, rows: u16, cols: u16) -> Result<ChannelHandle>;
    async fn send_input(&self, channel: &ChannelHandle, bytes: &[u8]) -> Result<usize>;
    async fn resize_pty(&self, channel: &ChannelHandle, rows: u16, cols: u16) -> Result<()>;
    async fn exec_argv(&self, session: &SessionHandle, program: &str, args: &[String], timeout: Duration,
                       cancel: CancellationToken) -> Result<ExecResult>;
    async fn sftp(&self, session: &SessionHandle) -> Result<SftpHandle>;
    async fn disconnect(&self, session: SessionHandle, reason: DisconnectReason) -> Result<()>;
}

trait PolicyEngine {
    fn validate_tool(&self, request: &ToolRequest) -> Result<NormalizedToolRequest>;
    fn evaluate(&self, request: &NormalizedToolRequest, ctx: &PolicyContext) -> Decision;
}

trait ModelClient {
    async fn validate_profile(&self, profile: &ModelProfile, cancel: CancellationToken) -> Result<Capabilities>;
    async fn stream(&self, req: ChatRequest, cancel: CancellationToken) -> Result<Receiver<ModelEvent>>;
}

trait LocalAuditStore {
    async fn append(&self, event: SanitizedAuditEvent) -> Result<AuditEventId>;
    async fn export(&self, filter: AuditFilter, format: ExportFormat, cancel: CancellationToken) -> Result<ExportResult>;
    async fn verify(&self, files: VerifyInput) -> Result<VerificationReport>;
}
```

`SshConfig` 字段必须为已校验的 host/port/username/auth handle/fingerprint；不得接受 command 字符串。`Decision` 只能是 `allow|ask_approval|deny`，默认拒绝。`ModelClient` 仅接收脱敏上下文。`SanitizedAuditEvent` 的 payload 不得含秘密或完整敏感输出。

## 7. 事件接口（Tauri event）

事件统一外层：

```json
{"event":"session.output","version":1,"seq":42,"session_id":"s1",
 "correlation_id":"c1","occurred_at":"2026-09-03T08:00:01Z","data":{}}
```

`seq` 在每个 session/任务内单调递增；前端发现跳号可请求重放。事件载荷 ≤256 KiB，敏感事件永不含原文。

| 事件 | data 结构与限制 |
|---|---|
| `session.output` | `{stream:"stdout"|"stderr"|"pty",bytes_base64, truncated}`；块 ≤64 KiB；不记录密码输入。 |
| `session.status` | `{status,reason?,endpoint_fingerprint?,remote_identity_changed?}`；reason ≤512。 |
| `credential.authentication_required` | `{session_id,method,expires_at?,prompt_id}`；不含密码/Token。 |
| `credential.authentication_failed` | `{session_id,method,code,attempts_remaining?}`；不说明账户存在性。 |
| `transfer.progress` | `{transfer_id,status,bytes_done,total_bytes?,speed_bps?,eta_seconds?}`；数值非负，进度单调。 |
| `agent.delta` | `{task_id,kind:"text_delta"|"tool_call_delta"|"usage"|"done"|"error",text?,tool?,usage?}`；text 已脱敏。 |
| `approval.created` | `{approval_id,task_id,risk,summary,expires_at,policy_version}`；summary 不含秘密。 |
| `audit.appended` | `{event_id,event_type,hash,prev_hash,created_at}`；只发哈希和元数据。 |
| `audit.export_status` | `{export_id,status,file_uri?,manifest_uri?,event_count?,error_code?}`；URI 为本地路径摘要或用户选择路径。 |
| `system.emergency_stop` | `{stop_id,scope,active,stopped_tasks,reason_hash}`；reason 原文不广播。 |
 
事件错误统一通过 `error` 字段或对应终态事件表达，不能抛出未脱敏堆栈。

## 8. 请求/响应校验清单

| 类别 | 必须校验 |
|---|---|
| 身份与权限 | 当前 Windows 用户、Hello/PIN 二次确认、会话归属、策略版本、审批票据、急停状态 |
| 网络 | 主机名/IP、端口范围、SSH 指纹/远端身份 HMAC、TLS 仅用于模型 HTTPS；禁止把网页 URL 当 SSH 地址 |
| 凭据 | 四字段格式严格解析、无重复/未知字段、有效期未来、内存清零、只返回引用 |
| 命令 | argv 结构、白名单、AST/等价变体、参数范围、超时、工作目录；禁止 shell 元字符和动态脚本 |
| 路径 | 本地 workspace、远端根目录、规范化后无 `..` 越界、NUL/UNC/控制字符拒绝 |
| 模型外发 | endpoint scope、数据分类、脱敏预览、禁止凭据类别；Profile 来源必须 `user_termpilot` |
| 审计 | 先写授权/审批和审计再执行；哈希链连续；导出 manifest 文件哈希可离线验证 |
| 资源 | 文本、数组、文件、并发会话、超时和重试上限；超限返回 `VALIDATION`，不得静默截断安全字段 |

## 9. 典型端到端案例

### 9.1 通过堡垒机建立终端

1. `host_upsert` 传入 `address=jtdcblj2.zhenergy.com.cn`、`port=8022`、`username=u890374m2`、`connection_type=bastion_endpoint`。
2. `credential_store` 返回 `credential_ref`（真实密码只进入 Credential Manager）。
3. `session_connect` 返回 `session_id`，收到 `session.status(ready)` 后渲染终端。
4. 用户输入通过 `session_send_input` 的 base64 bytes 发送；输出通过 `session.output` 返回。

### 9.2 Agent 只读诊断

`agent_message_send` → `agent.delta(tool_call_delta)` → `get_terminal_context`/`run_read_only_command` → `agent.delta(text_delta)` → `agent.delta(done)`。任何工具参数越权、审计失败、指纹变化或急停均转为阻断错误，不自动重放。

### 9.3 SFTP 上传

`sftp_transfer_start` 返回 `transfer_id` → `transfer.progress` 多次推送 → `completed` 或用户调用 `transfer_pause/resume/cancel`。覆盖生产文件必须先 `approval.created` 和 `approval_decide`。

### 9.4 失败响应案例

端口被误填为 URL 或超出范围：

```json
{"ok":false,"request_id":"r2","data":null,
 "error":{"code":"HOST_INVALID_ADDRESS","message":"SSH 地址不能包含协议或路径","retryable":false,
           "details":{"field":"address"}}}
```

主机指纹变化：

```json
{"ok":false,"request_id":"r3","data":null,
 "error":{"code":"SSH_HOSTKEY_CHANGED","message":"访问端点身份发生变化，连接已阻断","retryable":false,
           "details":{"host_id":"h1","confirmation_required":true}}}
```

Agent 缺少审批票据或本地审计不可用：

```json
{"ok":false,"request_id":"r4","data":null,
 "error":{"code":"APPROVAL_REQUIRED","message":"该操作需要人工审批","retryable":false,
           "details":{"proposal_id":"p1","risk":"high"}}}
```

```json
{"ok":false,"request_id":"r5","data":null,
 "error":{"code":"AUDIT_UNAVAILABLE","message":"本地审计不可用，已阻止自动执行","retryable":true,
           "details":{"correlation_id":"c1"}}}
```

## 10. 兼容性与版本

API 版本通过事件 `version` 和应用诊断暴露；新增字段向后兼容，删除/改变枚举需升主版本。未知请求字段默认拒绝于安全敏感接口、对配置读取则忽略并写诊断。客户端必须处理未知事件和错误码，并以安全失败为默认行为。

## 11. 配置文件接口（TOML）

配置读取器不是远程 HTTP 接口，但属于启动和 Profile API 的输入边界。读取顺序为“内置默认值 → 用户级 `.termpilot/config.toml` → 用户级 Profile → 进程环境变量临时覆盖”。项目级文件只引用 Profile。

用户级目录（与 `.codex` 同级）：

```text
%USERPROFILE%\\.termpilot\\
├── config.toml
├── profiles\\openai.toml
├── profiles\\ollama.toml
└── auth.toml
```

用户级 `config.toml` 示例：

```toml
default_profile = "openai"

[profiles.openai]
provider = "openai-compatible"
model = "gpt-4.1"
base_url = "https://api.example.com/v1"
temperature = 0.2
timeout_seconds = 60
auth_ref = "wincred://TermPilot/openai"
stream = true
```

Profile 名称 1–128 字符（`[A-Za-z0-9._-]`），`provider` 只能是 `openai-compatible|ollama`；`model` 1–256；`base_url` 必须 HTTPS，Ollama 回环地址允许 HTTP；公网/未知域名按 `Public` 处理，用户不能手工降级为 `Local`；temperature 0–2；timeout 5–600；能力字段只能是布尔值。未知字段忽略并在 `model_profile_validate.warnings` 返回路径。

`auth.toml` 只保存引用，例如：

```toml
[refs.openai]
target = "windows-credential-manager:TermPilot/openai"
```

`target` 1–512 字符，不得包含 `secret`、`key` 的实际值。进程环境变量（如 `TERMPILOT_OPENAI_API_KEY`）仅在内存中使用，不写文件、日志、审计或模型上下文。

项目级 `<project-root>\\.termpilot\\config.toml` 示例：

```toml
model_profile = "openai"
```

项目配置允许 `model_profile` 和非模型项目设置（工作区、策略引用等），禁止定义 provider、model、base_url、认证或密钥字段；该文件可以纳入版本控制。`.codex` 不存在、损坏或权限不足均不得阻止 TermPilot 启动。
