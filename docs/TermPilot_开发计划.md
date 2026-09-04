# TermPilot 详细开发计划（个人部署版）

版本：1.0（2026-09-03）  
依据文档：

- [需求分析与系统设计](./TermPilot_需求分析与系统设计.md)
- [数据库表设计](./TermPilot_数据库表设计.md)
- [API 规范](./TermPilot_API规范.md)

## 1. 计划目标

本计划面向 1 名个人开发者，目标是交付一个可在 Windows 10 22H2/Windows 11 x64 上本地启动的 TermPilot 个人部署版。开发过程采用“阶段性交付”：每个阶段结束都必须能够启动应用、看到可操作界面或运行结果，并完成一组可重复的本地实机测试；不把所有功能堆到最后一次联调。

计划周次是全职投入基线。业余开发时按实际投入顺延，但不得为了压缩周期而删除安全、审计和恢复测试。

## 2. 最终范围与明确不做事项

### 2.1 MVP 必须完成

- Windows 单用户桌面应用（Tauri 2 + React/TypeScript + Rust）。
- 主机新增、编辑、分组、软删除；支持直连 SSH 和 SSH 堡垒机端点。
- 结构化填写 `host`、`port`、`username` 和认证方式；示例端点为 `jtdcblj2.zhenergy.com.cn:8022`，用户名 `u890374m2`。
- 密码、私钥、SSH Agent；密码可按会话、期限或永久保存，但秘密只进入 Windows Credential Manager/DPAPI。
- xterm.js 终端、PTY、resize、输入、输出、断线和重连提示。
- SFTP 列表、上传、下载、删除、重命名、新目录、进度、暂停、取消、重试、续传和覆盖确认。
- SQLite 业务数据、本地哈希链审计、JSONL/CSV 导出和离线校验。
- Agent 只通过结构化工具调用；默认逐条审批；允许列表中的低风险只读命令可按模式免逐条审批。
- OpenAI-compatible、Ollama、Windows 系统 HTTP 代理和公网模型外发预览。
- 用户级 `%USERPROFILE%\\.termpilot` 模型配置；项目 `.termpilot` 只能引用 Profile；运行时绝不读取 `.codex`。
- 急停、安装包、升级/回滚和本地备份。

### 2.2 当前明确不做

SIEM、企业 CA/mTLS、企业审计签名、集中审计上传、多用户共享、云同步、多级跳板链、Telnet/串口、企业 SSO，以及 AI 无审批执行修改操作。

## 3. 开发和测试基线

### 3.1 开发机

| 项目 | 要求 |
|---|---|
| 操作系统 | Windows 10 22H2 或 Windows 11 x64 |
| 必备工具 | Git、Node.js LTS、pnpm/npm、Rust stable、Visual Studio C++ Build Tools、WebView2 |
| 推荐工具 | VS Code、Windows Terminal、OpenSSH Client、SQLite CLI |
| 可选工具 | Ollama、Docker Desktop（用于隔离测试服务）、WinAppDriver/Playwright |
| 本地端口 | 前端开发端口和 Mock 服务端口不得写死在生产配置 |

### 3.2 测试目标分层

1. **纯本地 Mock**：无网络也能启动，用于 UI、命令封装、策略和审计测试。
2. **本机实机**：连接 `localhost` OpenSSH Server 或隔离虚拟机，验证真实 SSH/PTY/SFTP。
3. **隔离堡垒机**：使用获授权的测试端点验证端口 8022、指纹、后端映射和 SFTP 能力。
4. **模型测试**：优先 Ollama；再使用脱敏的 OpenAI-compatible 测试 Profile。禁止把真实 API Key 写入仓库、测试快照或日志。

### 3.3 必须先准备的资料

- 已获授权的 SSH 测试主机或本机 OpenSSH Server。
- 堡垒机地址、端口、用户名、认证方式、SSH 指纹和后端目标映射说明。
- 不含真实密码的测试配置；真实凭据只通过本地表单或 Credential Manager 注入。
- 一个可选的 Ollama 模型或 OpenAI-compatible 测试 Profile。
- 本地审计保留期限、导出目录和备份目录。

若堡垒机资料尚未确认，先完成本地 Mock 和 localhost 实机阶段；不得用未授权生产账号替代测试账号。

## 4. 统一阶段交付规则

每个阶段必须提交以下内容：

- 可启动版本（Debug 版本也可以）；
- 本阶段新增的 UI/Command/Event 或内部模块；
- 自动化测试和一份手工测试记录；
- “已知问题/未完成项”清单；
- 数据库迁移或配置迁移说明（若有）；
- 下一阶段开始前的回滚点（Git tag 或安装包）。

阶段完成不等于代码写完，而是满足该阶段的退出条件，并能由开发者在同一台 Windows 机器上重复演示。

## 5. 阶段计划总览

| 阶段 | 周次 | 交付重点 | 阶段结束可见效果 |
|---|---:|---|---|
| P0 环境与技术验证 | W1 | 工具链、Tauri 壳、SSH/PTY/SFTP 可行性 | 窗口启动；可用命令行验证 8022 和 localhost SSH |
| P1 可运行骨架 | W1–W2 | React 页面、Rust command、事件总线、CI | 能打开主窗口、切换页面、运行 Mock command |
| P2 数据库与主机管理 | W3 | SQLite 迁移、主机 CRUD、项目目录 | 可新增主机并重启后保留；可看到数据库记录 |
| P3 凭据与真实 SSH | W3–W5 | Credential Manager、结构化连接、指纹 | 可连接 localhost；条件具备时可连接堡垒机 8022 |
| P4 终端与会话 | W5–W7 | PTY、xterm.js、输入输出、重连 | 可在应用窗口使用真实远程终端 |
| P5 SFTP 与审计 | W6–W9 | 文件传输、续传、本地哈希链审计 | 可浏览、上传/下载文件并导出校验审计 |
| P6 策略与审批 | W7–W10 | argv、AST、风险、允许列表、审批票据 | 高危命令被阻断，安全只读命令按策略执行 |
| P7 模型 Profile 与 Agent | W9–W12 | `.termpilot`、模型流式、工具编排 | 输入自然语言可获得诊断；工具调用受控 |
| P8 UI 集成与急停 | W11–W13 | 主界面、SFTP、Agent、审批和急停 | 完整关键用户旅程可操作，功能冻结 |
| P9 系统/安全/性能测试 | W13–W15 | Windows、堡垒机、注入、恢复、长稳 | 形成测试报告，阻断缺陷清零或有豁免 |
| P10 发布验收 | W16 | 安装包、升级回滚、个人验收 | 可安装、升级、回滚并交付使用文档 |

## 6. 分阶段详细计划

当前实现进度：P5 已完成 SQLite 迁移基线、主机 CRUD/软删除、Credential Manager 凭据引用、`ssh2` 真实握手/认证/HostKey 指纹与 PTY worker、真实 SSH SFTP channel（列表/上传/下载/删除/重命名/建目录）、Workspace 离线映射和本地审计哈希链；真实端点上的长时间输出、断线重连、续传恢复仍需获授权测试账号或本机 `sshd` 证据。详见 [P3–P5 验证记录](./验证记录-P3-P5.md)。

### P0：环境与技术验证（W1）

**目标**：先证明开发环境和 SSH 技术路径成立，不开始复杂 UI。

**开发任务**：

1. 初始化 Tauri 2 + React/TypeScript + Rust workspace。
2. 固定 Node/Rust 版本和依赖锁文件，配置 lint、format、单元测试和构建脚本。
3. 编写独立 SSH PoC：结构化传入 `address`、`port`、`username`，不得接受命令字符串。
4. 分别验证 localhost SSH 和获授权堡垒机：TCP、认证、HostKey、PTY、resize、SFTP。
5. 记录端点是否需要登录后选择目标、是否支持 SFTP、断线重连和大文件。

**阶段演示**：

```text
ssh -p 8022 u890374m2@jtdcblj2.zhenergy.com.cn
```

仅在账号明确获授权时执行；无授权时使用 localhost/隔离虚拟机。

**实机测试**：

- `Test-NetConnection <host> -Port 8022` 成功或明确记录不可达原因；
- 连接 `localhost` OpenSSH Server，执行 `printf`、`uname` 等无副作用命令；
- 打开 PTY，调整行列，验证输出和断开；
- SFTP 列目录并传输一个合成文本文件；
- 验证 `ssh user@host 8022` 不会被应用采用，端口始终单独传递。

**退出条件**：PoC 可重复运行；SSH/PTY/SFTP 结论记录在 `docs/验证记录-P0.md`；CI 能通过格式和单元测试；若堡垒机不可用，localhost 路径仍必须通过。

### P1：可运行骨架（W1–W2）

**目标**：任何后续阶段都基于一个可启动桌面应用开发。

**开发任务**：

- 建立 `src/`、`src-tauri/`、`tests/`、`migrations/`、`docs/` 目录约定；
- 实现窗口、导航、错误边界、加载/空状态和统一通知组件；
- 实现 `invoke(command, request)` 封装、统一成功/错误响应、`request_id`、`correlation_id`；
- 建立 Tauri event 总线，支持 `seq` 检查和断线后重放接口；
- 增加 Mock command：host、session、transfer、agent、audit；
- 配置 GitHub Actions 或本地等价 CI：TypeScript、Rust、Markdown 检查和测试。

**阶段演示**：运行 `npm run tauri dev`（或项目实际包管理器对应命令），窗口显示主机、终端、SFTP、Agent、审计五个空状态页面；点击“测试连接”可看到 Mock 的 `session.status` 和 `session.output`。

**退出条件**：Debug 应用可启动；关闭/重启不残留后台进程；Mock command/event 有自动化测试；错误响应不会显示 Rust 堆栈或秘密字段。

### P2：数据库与主机管理（W3）

**目标**：实现数据库基线和第一个可持久化的真实功能。

**开发任务**：

1. 按数据库设计创建 SQLite WAL、迁移表和 17 张业务表。
2. 实现迁移版本、校验和、失败回滚和数据库健康检查。
3. 实现 `host_list/filter`、`host_upsert`、`host_delete`。
4. 实现主机表单：名称、连接类型、地址、端口、用户名、认证方式、分组、工作区、生产标记。
5. 加入地址/端口/用户名/路径校验；拒绝 URL、Shell 元字符、控制字符和越界目录。

**阶段演示**：在应用中新增 `jtdcblj2.zhenergy.com.cn:8022` 测试主机，关闭应用再启动，主机仍存在；打开诊断页查看迁移版本和数据库路径。

**实机测试**：新增、编辑、筛选、软删除、重复名称冲突；数据库重启恢复；另一个 Windows 标准用户不能读取当前用户数据目录。

**退出条件**：主机 CRUD 可用，数据库迁移通过，秘密字段不存在于 SQLite；`host_delete` 只软删除，不删除审计历史。

### P3：凭据与真实 SSH（W3–W5）

**目标**：让应用可以安全取得凭据并建立真实 SSH 连接。

**开发任务**：

- 封装 Windows Credential Manager/DPAPI 和三种解锁策略；
- 实现 `credential_store`、`credential_bundle_import`，解析后立即清零原文；
- 实现 `SshTransport`、HostKey 指纹确认和可选后端身份 HMAC；
- 实现 `session_connect`、`session_cancel` 的连接状态机；
- 为密码、私钥、SSH Agent 分别实现认证适配器；
- 增加认证失败次数、冷却、过期清理和错误脱敏。

**阶段演示**：从表单选择“仅本次运行”密码，点击连接 localhost；应用显示 `connecting → ready`。若已获授权，切换到堡垒机 Host 后使用端口 8022 连接。密码不出现在日志、数据库、React 持久化状态或 DevTools。

**实机测试**：错误密码、过期凭据、指纹首次确认、指纹变化阻断、断网超时、Hello/PIN（可用时）、Credential Manager 条目删除；验证同一 `request_id` 重试不会重复创建会话。

**退出条件**：三种认证至少有密码和 SSH Agent 可用；私钥路径引用可用；连接参数完全结构化；指纹变化默认阻断；秘密扫描无命中。

### P4：终端与会话（W5–W7）

**目标**：完成第一个真正可用的远程终端体验。

**开发任务**：

- 将 SSH PTY channel 接入 xterm.js；
- 实现 `session_send_input`、`session_resize`、`session_disconnect`；
- 实现 `session.output`、`session.status`、认证事件和 seq 重放；
- 支持多标签会话、复制、粘贴、搜索、滚动和有限缓冲；
- 处理网络断开、重连、远端主动关闭和 ConPTY 降级；
- 输入按 base64 bytes 传输，不把密码输入写入终端审计。

**阶段演示**：应用窗口中打开两个 SSH 标签，执行无副作用命令，调整窗口大小，复制输出，断网后看到断线提示并可手工重连。

**实机测试**：ANSI/Unicode、持续输出、1 MB/s 输出限速、resize 合并（500 ms）、8 个并发会话、强制断开后资源回收。

**退出条件**：单会话和多会话稳定；输入响应 ≤100 ms（本机基线）；事件无重复/乱序；关闭窗口后 SSH channel 全部释放。

### P5：SFTP 与本地审计（W6–W9）

**目标**：实现文件操作和可验证的本地证据链。

**开发任务**：

1. 实现 `sftp_transfer_start`、`transfer_pause/resume/cancel` 和 `transfer.progress`。
2. 实现远端目录列表、上传、下载、删除、重命名和新目录。
3. 实现本地 workspace 限制、远端 realpath、符号链接复核、覆盖确认、临时文件和原子替换。
4. 支持 100 MB 可恢复传输，避免把 20 GB 文件一次性读入内存。
5. 实现 `LocalAuditStore`、追加式 SHA-256 哈希链、`audit_export` 和 `audit_export_verify`。
6. 审计事件覆盖连接、指纹、凭据结果、SFTP、策略、审批、Agent 和急停；只保存脱敏摘要和哈希。

**阶段演示**：浏览 `/tmp`，上传一个 100 MB 合成文件，手工暂停、断网、恢复并下载；在审计页面导出 JSONL/CSV，修改一个字节后校验失败。

**实机测试**：路径 `..`、NUL、UNC、符号链接逃逸、覆盖冲突、权限不足、磁盘满、传输中断、应用强杀后续传；审计数据库损坏时 Agent 自动执行必须阻断。

**退出条件**：基本 SFTP CRUD 和续传可用；导出 manifest、文件哈希和事件链离线校验通过；任何秘密扫描无命中。

### P6：策略与审批（W7–W10）

**目标**：先建立安全闸门，再开放 Agent。

**开发任务**：

- 实现 `PolicyEngine` 和 `policy_allow_rule_upsert`；
- 将命令固定为 `program + args[]`，实现 AST/等价变体分析；
- 实现 `readonly`、`ask_before_execute`、`allow_safe_commands`、`manual_only`；
- 实现风险等级、主机/用户/目录/策略版本绑定和执行限额；
- 实现 `propose_command`、`approval_decide`、一次性票据和二次确认；
- 实现 Dry Run、超时、输出上限、连续失败停止和 TOCTOU 复核。

**阶段演示**：允许列表加入 `df -h`，在安全模式下执行并看到审计；输入 `rm -rf /`、`curl … | bash`、增加未知参数或改变 cwd 时，UI 显示阻断且远端无执行数据包。

**实机测试**：多空格、引号、换行、Unicode、变量、管道、重定向、`sh -c`、审批过期、策略版本变化、并发审批、急停竞态。

**退出条件**：高危命令阻断率 100%；安全只读命令每次都有规则 ID、策略版本、命令哈希和审计 ID；审批不能重放；审计不可用时执行拒绝。

### P7：模型 Profile 与 Agent（W9–W12）

**目标**：接入模型，但模型永远不能越过策略和执行代理。

**开发任务**：

1. 实现用户级 `%USERPROFILE%\\.termpilot` 配置读取、Profile 校验和 SQLite cache。
2. 支持 `model_profile_list`、`model_profile_validate`、`model_egress_preview`。
3. 支持 OpenAI-compatible、Ollama、流式事件、超时、限流和最多两次退避。
4. 实现上下文构建、按行截断、敏感检测和公网外发预览。
5. 实现 `agent_message_send`、`agent_cancel` 和八个 AI 工具。
6. Agent 状态机限制最大 12 步、单步 30 秒、总时长 5 分钟、连续失败 3 次停止。

**阶段演示**：在 `%USERPROFILE%\\.termpilot\\profiles\\ollama.toml` 配置本地模型，输入“检查磁盘空间”，看到流式回答和 `run_read_only_command` 工具调用；切换到公网 Profile 时先展示脱敏预览。`.codex` 不存在也不影响启动。

**实机测试**：Ollama 不可用、模型超时、非法 JSON、Prompt Injection、伪造 approval、凭据/Token/私钥出现在终端输出、工具参数越权；验证模型请求捕获结果中无秘密。

**退出条件**：模型只能生成结构化工具调用；工具 schema 额外字段被拒绝；策略、审批、审计和急停任一失败均安全阻断；模型不可用不影响 SSH/SFTP/终端。

### P8：UI 集成与急停（W11–W13）

**目标**：把已验证的后端能力整合成完整可操作产品，并冻结 MVP 功能。

**开发任务**：

- 完成主窗口布局：主机列表、标签终端、SFTP、Agent 侧栏、审批弹窗、审计页、设置页；
- 接入所有 API command/event，统一 loading、空、错误、重试、取消状态；
- 实现 `emergency_stop`/`emergency_stop_clear`，快捷键和 UI 均可触发；
- 实现生产主机标记、危险操作提示、二次确认和数据外发预览；
- 完成键盘操作、中文/英文资源、125%/200% DPI 布局；
- 关闭发布构建 DevTools，检查日志和崩溃脱敏。

**阶段演示**：从新建主机开始，连接、打开终端、浏览 SFTP、发送 Agent 问题、审批安全命令、导出审计、点击急停，再清除急停；整个流程不需要开发者工具。

**退出条件**：W13 功能冻结；P0 功能均有 UI 入口；关键旅程可在同一台 Windows 实机重复完成；阻断级 UI 缺陷为 0。

### P9：系统、安全、性能与恢复测试（W13–W15）

**目标**：以发布候选版本验证非功能指标和高风险边界。

**测试矩阵**：

| 类别 | 必测内容 | 达标线 |
|---|---|---|
| 兼容性 | Win10 22H2、Win11、WebView2、x64 安装/卸载 | 关键旅程通过 |
| SSH | localhost、隔离主机、堡垒机 8022、指纹变化、重连 | 8 会话稳定 |
| SFTP | 100 MB 续传、20 GB 流式、覆盖/权限/路径逃逸 | 不越界、不丢数据 |
| 安全 | Prompt Injection、Shell fuzz、审批重放、急停竞态、秘密扫描 | 高危阻断 100% |
| 审计 | WAL 恢复、磁盘满、导出篡改、链校验 | 篡改可检出 |
| 性能 | 冷启动、AI 首字节、持续输出、内存 | 按需求成功标准 |
| 恢复 | 断网、强杀、模型不可用、Credential 到期 | 可恢复且安全失败 |

**阶段演示**：使用发布候选安装包执行一遍从安装到卸载的完整脚本，生成测试报告、缺陷清单、秘密扫描报告、性能报告和审计验证报告。

**退出条件**：P0 阻断缺陷为 0；P1 缺陷已修复或有书面豁免；需求文档中的成功标准均有数据证据；不得用未授权生产账号做压测。

### P10：个人验收与发布（W16）

**目标**：形成可恢复、可更新、可交付的个人安装版本。

**开发任务**：

- 生成 NSIS 安装包；有证书则签名，无证书则发布 SHA-256；
- 配置 Tauri updater、签名清单、失败回滚和版本迁移；
- 编写安装、首次配置、SSH 堡垒机、凭据保存、模型 Profile、审计导出和故障排查手册；
- 备份 SQLite、策略和主机元数据，不备份 Credential Manager 秘密；
- 验证卸载时“保留/删除当前用户数据”选项，不触碰其他 Windows 用户数据；
- 创建发布 tag、变更日志、已知限制和回滚包。

**个人验收清单**：

1. 新安装后可启动，首次启动不依赖 `.codex`。
2. 可新增 localhost 或获授权堡垒机主机；端口 8022 语义正确。
3. 可建立终端、执行安全只读命令、完成 SFTP 上传/下载。
4. AI 请求经过脱敏、策略、审批和本地审计；高危命令被阻断。
5. 导出 JSONL/CSV 后可离线验证；篡改文件能报错。
6. 急停能阻断新任务，清除急停需要当前用户确认。
7. 升级失败可以回滚；卸载选项和数据隔离符合预期。

**退出条件**：发布候选包在目标 Windows 实机完成一次全流程验收；安全检查、依赖扫描、许可证清单和 SHA-256 已归档；仅在获得明确授权后连接正式服务器。

## 7. 每阶段推荐启动和测试命令

项目脚本名称可按实际 package manager 调整，但应保持等价能力：

```powershell
# 安装依赖并启动桌面开发版
npm ci
npm run tauri dev

# 前端和 Rust 单元测试
npm run test
cargo test --manifest-path src-tauri/Cargo.toml

# 静态检查
npm run lint
npm run typecheck
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings

# 生成安装包
npm run tauri build
```

P0–P2 只要求 Mock/localhost；P3–P5 增加真实 SSH/SFTP；P7 增加 Ollama 或脱敏模型 Profile；P9–P10 使用安装包而不是开发服务器验证。

## 8. 里程碑、依赖和延期处理

关键路径为：`P0 → P1 → P2 → P3 → P4/P5 → P6 → P7 → P8 → P9 → P10`。

- P0 未确认 SSH/PTY/SFTP 可行性，不得承诺堡垒机功能完成时间；先继续 localhost。
- P3 凭据保护未通过秘密扫描，不得进入真实堡垒机测试。
- P5 审计链未通过，不得开启 Agent 自动执行。
- P6 策略和审批未通过，不得接入会产生远端副作用的工具。
- P7 模型服务不可用时，P8 仍必须能演示 SSH、SFTP、审计和手工命令。
- P9 若性能或安全指标不达标，回退到最近 tag 修复；不得删减安全测试来赶 P10。

## 9. 风险和验收证据归档

每个阶段在 `docs/evidence/<stage>/` 保存：运行截图、命令输出、测试报告、版本号、配置摘要（不得含秘密）、错误复现步骤和结论。建议至少归档：

- `P0`：SSH/PTY/SFTP 能力矩阵和端点说明；
- `P3`：Credential Manager、指纹和秘密扫描报告；
- `P5`：100 MB 续传、审计导出和篡改校验报告；
- `P6`：Shell fuzz、审批重放和高危阻断报告；
- `P7`：模型请求脱敏捕获和 Prompt Injection 回归报告；
- `P9`：兼容性、性能、恢复和长稳报告；
- `P10`：安装包哈希、升级/回滚和最终验收签字记录。

所有证据必须脱敏；不得把密码、私钥、Token、Cookie、API Key、凭据包原文或其普通哈希写入证据目录。
