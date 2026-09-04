# TermPilot 个人版需求分析与系统设计

版本：2.0  
日期：2026-09-04  
状态：个人实现基线

## 1. 产品目标

TermPilot 是本人使用的 Windows 桌面工具，用于连接本人已获授权的生产和测试 SSH 服务器，提供远程终端、SFTP 和受控 AI 诊断。应用不对外发布，不提供团队协作或企业部署。

## 2. 范围

### 2.1 必须实现

- 主机新增、编辑、分组、软删除。
- 直连 SSH 和单级 SSH 堡垒机。
- 密码、私钥文件引用、SSH Agent。
- SSH 主机指纹确认和变化阻断。
- 多标签远程终端、PTY、resize、复制、搜索、断线提示。
- SFTP 列表、上传、下载、删除、重命名、新目录、暂停、取消、重试和下载续传；连接后默认访问远端 `~`。
- 本地路径按次选择并校验，远端执行 realpath、符号链接复核和覆盖确认。
- SQLite 本地审计、SHA-256 哈希链和 JSONL 导出。
- 默认人工审批、固定只读命令白名单、高危命令阻断。
- 上下文脱敏、一个模型 Provider、Agent 取消和限额。
- 全局急停、本地备份和恢复。

### 2.2 明确不做

安装包、自动更新、升级回滚、代码签名、跨 Windows 版本兼容、ARM64、多用户共享、团队协作、云同步、企业 SSO、SIEM、集中审计、企业 CA/mTLS、多级跳板、Telnet、串口、公网 HTTP API、双模型 Provider、项目级模型 Profile、`.codex` 兼容、Hello/PIN、复杂凭据包、CSV 导出、后台自动恢复和通用 Shell 等价分析。

## 3. 用户和数据边界

只支持当前 Windows 用户。数据库位于 `%LOCALAPPDATA%\\TermPilot\\data`；配置位于 `%USERPROFILE%\\.termpilot\\config.toml`。密码、私钥、Token、API Key、完整终端输出和文件正文不得进入数据库、日志、崩溃信息、审计正文或模型请求。

## 4. 系统架构

```text
React/xterm.js
        │ Tauri command/event
Rust Core ── SQLite 审计/业务数据
   ├── Credential Manager
   ├── SSH/SFTP 传输层
   ├── 固定策略与审批引擎
   └── 单一模型客户端与 Agent
```

WebView 只能调用白名单 command；模型不能直接访问 SSH channel。所有远程操作都经过 Rust 参数校验、策略判断和审计。

## 5. SSH 和终端

主机连接参数必须分开保存：`address`、`port`、`username`、`auth_method`。堡垒机示例 `jtdcblj2.zhenergy.com.cn:8022` 中 8022 始终是 TCP 端口，不得作为远程命令。

MVP 只承诺 POSIX/OpenSSH 远端的 Agent 命令执行；Windows 远端可连接终端，但不启用自动命令。远程 PTY 由 SSH channel 提供，ConPTY 仅用于本地 Windows 辅助进程。

## 6. SFTP

单文件上限 20 GiB。下载按大小和 SHA-256 续传；上传只有在服务端确认安全 append 且用户明确确认时续传，否则从头传输。临时文件使用随机名称，完成后在同一目录内原子 rename。生产主机的删除、覆盖、重命名和上传默认需要确认。

## 7. Agent 和安全策略

默认模式为 `ask_before_execute`。自动执行仅限内置固定只读命令（首版：精确的 `df -h`、`pwd`、`whoami`），且必须同时满足：结构化 argv、白名单精确匹配、风险为 low、主机和 cwd 匹配、未使用解释器/管道/重定向、策略和审计可用。

不接受任意 Shell 字符串，不执行 `sh -c`、`bash -c`、`eval`、管道、重定向、二次 SSH、递归删除、权限修改或动态脚本。无法证明安全时直接阻断。生产环境修改操作必须人工审批。

终端和远程文件内容一律视为不可信数据，发送模型前按规则脱敏；公网模型禁止接收密码、私钥、Token、Cookie 和凭据文件。

## 8. 审计、急停和失败策略

授权/审批事件必须在远程执行前写入 SQLite。审计哈希为 `SHA256(canonical_json(event_without_hash_fields) || prev_hash)`，第一条事件使用全零 genesis 值。审计不可用、策略版本变化、审批过期、指纹变化、路径越界或急停时，远程写操作必须拒绝。

急停立即阻断新 Agent、SFTP 和命令，并尽力关闭活动 channel；解除需要当前 Windows 用户重新确认。应用重启后不自动恢复远程命令，SFTP 临时文件只能人工恢复。

## 9. 非功能要求

- 当前电脑冷启动目标 P95 ≤3 秒。
- 至少支持 8 个并发 SSH 会话。
- 终端和传输使用有界缓冲，不能因输出或大文件线性耗尽内存。
- 所有秘密扫描、路径逃逸、审批重放、指纹变化和高危命令测试必须通过。

## 10. 外部阻塞项

开始真实堡垒机验收前，必须确认登录后目标选择方式、PTY/resize/SFTP 能力、凭据有效期、后端目标身份、测试账号和隔离目录。模型只需在开发前选择 Ollama 或 OpenAI-compatible 其中一个。
