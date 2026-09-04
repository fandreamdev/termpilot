# P8 本地验证记录

日期：2026-09-03

## 自动化结果

- `npm run typecheck`：通过
- `npm test`：4/4 通过
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`：通过
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`：通过
- `cargo test --manifest-path src-tauri/Cargo.toml`：14/14 通过
- `npm run tauri:debug:build`：通过
- `npm run desktop:smoke`：通过，无残留进程

## 已覆盖的本地能力

- SQLite WAL、迁移校验、主机 CRUD 和软删除
- Windows Credential Manager/DPAPI 引用、凭据包字段校验和过期清理
- 结构化 SSH 参数、主机指纹确认、PTY 输入/输出/resize
- SFTP 工作区路径校验、传输控制和审计哈希链导出/校验
- Agent 结构化工具入口、策略阻断、审批过期检查和急停持久化
- Agent 页面、设置页和 `Ctrl+Shift+Escape` 快捷键急停入口
- 用户级 `.termpilot` Profile/项目配置读取；运行时不读取 `.codex`

真实 OpenSSH、堡垒机 8022、100 MB 断点续传、Ollama/公网模型和安装包升级回滚仍需在具备授权端点和 Windows 实机环境中执行。
