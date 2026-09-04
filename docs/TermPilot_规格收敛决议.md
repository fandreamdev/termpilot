# TermPilot 规格收敛决议

版本：1.0  
日期：2026-09-04  
状态：个人部署实现基线

本文档用于解决《需求分析与系统设计》《数据库表设计》《API 规范》和《开发计划》之间的实现歧义。若其他文档与本文冲突，以本文为准。

## 1. 已冻结的个人版 MVP 边界

- 平台：仅支持当前实际使用的 Windows x64 版本；单一 Windows 用户；Tauri 2 + React/TypeScript + Rust。不做跨版本发布承诺。
- SSH：直连 SSH 或单级 SSH 堡垒机端点。MVP 不实现多级跳板、Telnet、串口、SSO。
- 认证：密码、私钥文件引用、SSH Agent。秘密只进入 Windows Credential Manager/DPAPI 或 SSH Agent，不进入 SQLite、日志、审计、模型上下文和导出文件。
- 终端：远程 SSH PTY；ConPTY 仅用于本地 Windows 终端/辅助进程，不作为远程 SSH PTY 的实现名称。
- SFTP：列表、上传、下载、删除、重命名、新目录、暂停、取消、重试、下载续传；上传续传仅在服务端确认支持安全 append 且用户明确确认时启用，否则从头传输。单文件上限统一为 20 GiB。
- Agent：默认 `ask_before_execute`；只有固定允许列表中、低风险、只读、无副作用的命令可在 `readonly` 或 `allow_safe_commands` 下自动执行。修改、删除、权限、服务、网络配置和无法解析的命令不得自动执行。
- 远端命令：MVP 只支持结构化 `program + args[]`，不接受任意 Shell 字符串；无法证明安全时直接阻断。
- 模型：个人版只实现一个实际使用的 provider（Ollama 或 OpenAI-compatible，二选一）；配置使用单一用户级 `.termpilot/config.toml`，不实现项目级 Profile 和 `.codex` 兼容层。
- 审计：本地 SQLite 追加式 SHA-256 哈希链，授权/审批事件必须先于远端执行落盘；个人版要求 JSONL 和 `manifest.json` 离线校验，CSV 为后置可选功能。

以下能力从个人版删除：安装包、自动更新、升级回滚、代码签名、多用户/跨用户共享、企业 SSO、SIEM、集中审计、企业证书、云同步、多级跳板、Telnet、串口、REST/HTTP 兼容层和长期 API 兼容承诺。

以下能力降级为可选或后置：Windows Hello/PIN 解锁、凭据包导入、密码多种保存期限、CSV 导出、项目级配置、通用 Shell AST 等价分析、跨 Windows 版本兼容测试、复杂事件重放和自动恢复。

## 2. 统一数据字典

### 2.1 模型 Profile

`endpoint_scope` 统一使用小写枚举：`local|public|custom`。API 展示层可首字母大写，但线上协议和数据库均使用小写。`custom` 表示无法安全判定为本机回环地址，按公网策略处理，不允许用户手工降级为 `local`。

超时统一使用 `timeout_seconds`，整数范围 5–600；数据库列名改为 `timeout_seconds`，禁止再使用 `timeout_ms`。

### 2.2 任务状态

传输和工具对外终态统一为 `completed|failed|cancelled`；数据库 `sftp_operations.status` 同样使用 `completed`。Agent 对话仍可使用 `active|completed|cancelled|error`。

### 2.3 策略

`policy_allow_rule_upsert` 请求必须包含 `policy_id`。规则只写入指定策略；策略版本递增，旧审批票据立即失效。不存在 `policy_id` 或不是当前用户的策略时返回 `NOT_FOUND`/`FORBIDDEN`。

### 2.4 凭据包

`credential_refs` 不保存 `credential_bundle_profile_id`；凭据包和解析器的关联只记录在 `session_credential_bundles.profile_id`。表说明不得再列出该字段。

## 3. 实现约束

- 远程 Shell：MVP 目标为 POSIX/OpenSSH；Windows 远端只提供终端连接，不承诺 Agent 命令自动执行。
- 允许列表：首版只提供内置命令模板（如精确的 `df -h`、`pwd`、`whoami`），用户规则必须绑定程序、精确参数模式、主机 ID、远程用户、cwd、超时和输出上限；不支持“任意命令 + 关键词黑名单”。
- 审计哈希：`hash = SHA256(canonical_json(event_without_hash_fields) || prev_hash)`；第一条事件使用固定全零 64 字符十六进制 genesis 值。写入通过单写事务串行化；归档前必须生成完整导出，不能截断活动链。
- 传输 ID：API `transfer_id` 与 `sftp_operations.id` 为同一应用生成的 UUID/ULID。临时文件使用应用专用目录和不可预测名称，完成后通过同一目录内 rename 实现原子替换。
- 事件序号：`seq` 按 `session_id + stream` 递增；重放 API 必须按 session、stream 和起始 seq 查询，不能复用 `sessions.last_seq` 表示所有任务的全局序号。
- 取消：取消为尽力而为；最终状态、错误码和审计事件必须写入。应用重启后不自动恢复远程命令，SFTP 仅保留可人工恢复的临时文件。
- Agent 文件工具：模型不得伪造用户确认；上传、下载、删除、重命名和新目录缺少用户确认时由 Core 阻断，需用户在 SFTP 页面重新确认。

## 4. 仍然阻塞实现的外部确认项

以下内容未确认前，不得连接正式服务器，也不得把相关能力标记为“已验收”：

1. 堡垒机登录后是否直接到目标，还是需要菜单选择/二次跳转。
2. 堡垒机是否支持远程 PTY、resize、SFTP、断线重连和大文件。
3. 凭据有效期、一次性使用、断线后复用和重新获取方式。
4. 后端目标身份的稳定标识及其可读取方式。
5. 测试 SSH 账号、主机指纹、隔离 SFTP 目录和测试模型 Profile。
6. Windows Hello/PIN 的目标 API 和最低系统能力；在确认前只实现“当前用户自动解锁”。

## 5. 实现顺序

先完成本地 Mock、localhost OpenSSH、数据库迁移和策略单元测试；再接入堡垒机和真实模型。外部确认项完成后，补充契约测试和端到端证据，再进入正式服务器验收。

数据库迁移从现有 1.0 基线升级为 1.1：将 `sftp_operations.status='succeeded'` 映射为 `completed`。个人版不创建 `model_profile_cache`，模型配置直接读取单一用户级 TOML；迁移脚本必须在事务中执行并记录 checksum。若数据库尚未发布，可直接采用收敛后的 DDL。

个人版首轮实现顺序调整为：主机/凭据 → localhost SSH/PTY/SFTP → SQLite 审计 → 固定只读白名单和审批 → 单一模型适配 → UI 整合。发布、更新、多用户和企业能力不进入开发计划。
