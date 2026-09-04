# TermPilot

TermPilot 是面向单一 Windows 用户的 SSH/SFTP 个人工作台。当前仓库包含 Tauri 2 + React/TypeScript + Rust + SQLite 的可运行架构骨架，以及独立的 `frontend-demo/` 视觉案例。

## 目录

- `src/`：React 工作台、终端两栏布局和 Tauri command 客户端。
- `src-tauri/src/`：Rust Core；主机 CRUD、会话、策略、急停、配置和审计哈希模块。
- `src-tauri/migrations/`：SQLite 初始迁移，数据库位于 `%LOCALAPPDATA%\\TermPilot\\data`。
- `docs/`：需求、API、数据库设计、开发计划和规格收敛决议。
- `frontend-demo/`：不依赖 Tauri 的多页面 HTML 交互案例。

## 开发

```powershell
npm install
npm run dev       # 浏览器预览 React 工作台
npm run build     # TypeScript 检查和 Vite 构建
cargo check --manifest-path src-tauri/Cargo.toml
npm run tauri:dev # Windows Tauri 桌面应用
```

开发环境默认使用前端 Mock 数据；在 Tauri 窗口内会调用 Rust commands。远程文件浏览连接后默认从用户主目录 `~` 开始，本地上传下载路径按次选择并校验。真实 SSH、SFTP、Credential Manager 和唯一模型 Provider 通过适配器接入，未完成外部确认前不会自动连接正式服务器。

默认传输适配器为隔离 Mock。完成测试账号、指纹和隔离目录确认后，可在启动 Tauri 前设置 `TERMPILOT_TRANSPORT=openssh`，启用当前 Windows OpenSSH 的 `ssh`/`sftp` 适配器；该适配器要求系统 SSH Agent/密钥和 `known_hosts` 已配置，不会在命令行传递密码。

## 安全边界

Rust 是所有远程操作的唯一入口。命令只接受结构化 `program + args[]`，默认人工审批；固定只读模板和急停状态由 Core 校验。秘密不写入 SQLite、日志、审计正文或模型请求。真实堡垒机、凭据和模型验收前，请先完成规格文档列出的外部确认项。
