# TermPilot 使用与恢复指南

## 安装

运行 `TermPilot_0.1.0_x64-setup.exe`，安装范围为当前 Windows 用户。安装包发布前请先核对随包记录的 SHA-256。

## 首次配置

1. 在“主机”页面填写名称、SSH 地址、端口、用户名和认证方式；堡垒机端口单独填写为 `8022`。
2. 凭据只通过表单保存到 Windows Credential Manager，不要写入 SQLite 或配置文件。
3. 模型 Profile 放在 `%USERPROFILE%\\.termpilot\\profiles\\`，或放在用户级 `config.toml` 的 `[profiles.<name>]` 节；项目 `.termpilot` 只引用 Profile 名称。

## 安全操作

- Agent 只生成结构化工具调用；高风险命令必须审批。
- `Ctrl+Shift+Escape` 或界面“急停”会阻断新任务。清除急停需要当前用户确认。
- 审计可导出 JSONL/CSV，并使用 manifest 离线校验；修改导出文件会导致校验失败。

## 备份与恢复

备份应用数据目录中的 SQLite 数据库、策略和主机元数据；不要备份 Credential Manager 内容。恢复前关闭 TermPilot，替换数据库后重新启动并检查“数据库健康”。

## 限制

真实堡垒机、断点续传、模型服务和升级回滚需要在已授权的 Windows 实机环境中验证；不得使用未授权生产账号测试。
