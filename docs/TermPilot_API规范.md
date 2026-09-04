# TermPilot 个人版 API 规范

版本：2.0  
日期：2026-09-04  
范围：单用户 Windows 桌面应用，仅供本人使用

本文只定义 React 与 Rust Core 之间的 Tauri command/event。不提供公网 HTTP 服务，不做 REST 兼容，不做多用户权限和长期版本兼容。

## 1. 通用约定

调用：`invoke(command, request)`。响应统一为：

```json
{"ok":true,"request_id":"r1","data":{},"error":null}
```

错误：`{"ok":false,"request_id":"r1","data":null,"error":{"code":"...","message":"..."}}`。

ID 使用 UUID/ULID；时间使用 UTC RFC3339；长任务返回任务 ID；所有任务支持取消。密码、私钥、Token、API Key、凭据包原文不得出现在响应、日志、审计或模型上下文。

## 2. 主机和凭据

### `host_list`

请求 `{query?,group?,page_size?:number}`，返回当前用户未软删除主机。`page_size` 1–200。

### `host_upsert`

请求：

```json
{"id?":"h1","name":"测试机","connection_type":"direct_ssh",
 "address":"192.168.1.10","port":22,"username":"ops",
 "auth_method":"password","group_name":"test","is_production":false,
 "policy_id":"p1"}
```

`connection_type` 为 `direct_ssh|bastion_endpoint`；地址不得含 URL、路径或 Shell 字符；端口 1–65535；名称同一用户唯一。生产标记不会因编辑连接参数自动清除。

### `host_delete`

软删除。存在活动会话时返回 `HOST_IN_USE`，不会静默断开。

### `credential_store`

请求为 `{host_id,kind,target_name?,secret?,retention_mode}`，其中 `kind` 为 `password|private_key|ssh_agent`，`retention_mode` 为 `never|app_session`。个人版只实现“不保存”和“本次应用运行”两种模式：`app_session` 密码写入当前 Windows 用户的 Generic Credential，应用主窗口关闭时删除；`never` 密码仅保留在进程内存直到退出。私钥仅保存用户选取的绝对文件路径，SSH Agent 不保存秘密。响应仅返回非秘密的 `credential_ref`、类型和保存模式；密码、私钥正文不会回显，也不会写入 SQLite、日志、审计或模型上下文。Hello/PIN 和凭据包导入不实现。

## 3. SSH 会话和终端

### `session_connect`

请求 `{host_id,credential_ref?,fingerprint_confirmation?,pty?:{rows,cols}}`。首次或指纹变化必须有用户确认。返回 `session_id`，后续通过 `session.status` 通知 `ready` 或错误。

### `session_send_input`

请求 `{session_id,bytes_base64}`。只发送用户在终端输入的字节，不作为 Agent 权限。

### `session_resize`

请求 `{session_id,rows,cols}`，范围 1–1000。

### `session_disconnect`

请求 `{session_id,reason?}`。释放 PTY、SFTP 和后台线程。

### `session_cancel`

取消尚未完成的连接或后台操作。

状态：`connecting|ready|reconnecting|disconnected|closed|error`。

## 4. SFTP

### `sftp_list`

请求 `{session_id,path:"~",limit?:number,cursor?}`。连接后默认路径为远端用户 `~`；不允许 NUL、`..` 越界或符号链接逃逸。

响应 `data` 为 `{path,entries,next_cursor,count}`；`entries` 中每项为
`{name,kind}`，其中 `kind` 为 `file|directory`。目录列表授权事件必须先于
实际 SFTP 请求落盘，返回内容不包含文件正文。

### `sftp_transfer_start`

请求 `{session_id,op,src?,dst?,overwrite?:boolean,resume?:boolean}`。`op` 为 `upload|download|delete|rename|mkdir`；单文件上限 20 GiB；本地路径必须由用户按次选择并通过路径校验。生产文件覆盖、删除和上传默认需要人工确认。上传续传只有在服务端确认安全 append 且用户明确确认时启用，否则自动从头传输。

返回 `{transfer_id,status:"queued"|"running"|"completed"}`。`transfer_id` 与数据库 `sftp_operations.id` 相同；长任务通过 `transfer.progress` 事件报告进度。

### `transfer_pause|transfer_resume|transfer_cancel`

请求 `{transfer_id,reason?}`。取消和暂停不删除临时文件；完成状态为 `completed|failed|cancelled`。下载支持按大小和 SHA-256 续传；上传只有在服务端确认安全 append 且用户确认时续传，否则恢复时从头传输。

### `transfer_retry`

请求 `{transfer_id,confirmed?}`。仅对失败或取消的任务重新建立传输，并重新执行路径、覆盖和生产确认校验，返回新的 `transfer_id`。

### `list_remote_directory`

请求 `{session_id,path?:string,limit?:number}`，`path` 缺省为 `~`。返回目录项名称和 `file|directory` 类型，与 SFTP 使用同一越界校验。

### `read_remote_file`

请求 `{session_id,path,max_bytes?}`。响应仅返回经脱敏和大小限制的文本、SHA-256 与截断标记；文件正文不写入数据库、日志或模型上下文。

### `upload_file|download_file`

分别是 `sftp_transfer_start` 的受限封装，强制 `op=upload|download`；本地路径必须为用户按次选择的绝对路径，下载使用随机临时文件并在同一目录原子替换。

## 5. 策略、Agent 和审批

### `policy_get`

返回当前用户的活动策略。个人版只维护一个活动策略。

### `policy_allow_rule_upsert`

请求 `{policy_id,rule,reauth_token}`。规则必须绑定程序、精确参数、主机 ID、远程用户、cwd、超时和输出上限。首版只允许内置 `df -h`、`pwd`、`whoami` 等固定只读模板。

### `agent_message_send`

请求 `{session_id,text,mode?,conversation_id?,client_request_id}`，立即返回 `{task_id,status:"active",conversation_id}`；最终回答通过 `agent.delta` 事件返回，事件 `data` 包含 `task_id`、`conversation_id`、`status` 和 `delta`。模式为 `readonly|ask_before_execute|allow_safe_commands|manual_only`，默认 `ask_before_execute`。模型需要工具时只返回一个 `{tool,arguments}` JSON 对象；桌面端仅接受本文定义的 8 个工具并重新生成 `request_id`、`policy_version` 和 `deadline`，不会信任模型携带的元数据或破坏性确认字段。文件上传、下载、删除、重命名和新目录等 Agent 文件操作必须由用户在 SFTP 页面明确确认，不能由模型自动确认；最终执行仍由 Rust 策略、审批、路径校验和急停状态裁决。

### `agent_cancel`

请求 `{task_id,reason?}`。取消模型流和未执行工具；远程命令尽力停止，不自动重放。

### `approval_decide`

请求 `{approval_id,decision,phrase?}`，`decision` 为 `approve|reject`。票据一次性，策略版本变化或过期后失效。

Agent 工具仅保留：`get_terminal_context`、`run_read_only_command`、`propose_command`、`execute_approved_command`、`list_remote_directory`、`read_remote_file`、`upload_file`、`download_file`。工具请求必须带 `request_id`、`session_id`、`policy_version` 和 `deadline`。

不实现模型 Profile 列表、项目配置、双 Provider、通用 Shell 执行和公网工具扩展。

## 6. 审计和急停

### `audit_export`

个人版只要求 JSONL 导出和 `manifest.json` 校验；CSV、复杂筛选和远程上传不实现。

### `audit_export_verify`

离线验证文件 SHA-256 和审计链。

### `audit_list`

请求 `{limit?:number}`，返回本地脱敏审计事件列表，仅供当前用户界面展示。

### `app_settings_get|app_settings_set`

读写 `app_settings` 中的非秘密设置。写入请求为 `{key,value,value_type}`，禁止保存密码、Token、私钥或 API Key。

### `database_backup|database_restore`

备份请求 `{path}`，使用 SQLite `VACUUM INTO` 生成一致性 `.db` 文件。恢复请求 `{path,confirmed:true}`，恢复前必须由当前用户明确确认；两者均写入审计。

### `emergency_stop`

请求 `{scope:"all"|"session"|"agent",session_id?,reason}`。立即阻断新 Agent、SFTP 和命令。

### `emergency_stop_clear`

需要当前 Windows 用户重新确认。

## 7. 事件

事件外层：`{event,version,seq,session_id?,correlation_id,occurred_at,data}`。

保留事件：`session.output`、`session.status`、`transfer.progress`、`agent.delta`、`approval.created`、`audit.appended`、`system.emergency_stop`。事件块 ≤64 KiB，敏感内容已脱敏，不实现跨重启事件恢复。

## 8. 错误码

保留：`VALIDATION`、`NOT_FOUND`、`CONFLICT`、`HOST_INVALID_ADDRESS`、`SSH_HOSTKEY_CHANGED`、`SSH_AUTH_FAILED`、`SSH_TIMEOUT`、`SESSION_CLOSED`、`PATH_ESCAPE`、`SFTP_CONFLICT`、`POLICY_BLOCKED`、`POLICY_CONTEXT_CHANGED`、`APPROVAL_REQUIRED`、`APPROVAL_EXPIRED`、`AUDIT_UNAVAILABLE`、`MODEL_UNAVAILABLE`、`EMERGENCY_STOP_ACTIVE`、`CANCELLED`、`INTERNAL`。

所有未识别错误安全失败，不执行远程写操作。
