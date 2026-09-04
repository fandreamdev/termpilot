# P10 发布构建记录

日期：2026-09-03

## 构建结果

- 命令：`npm run tauri:build`
- 目标：Windows x64 NSIS
- 安装包：[TermPilot_0.1.0_x64-setup.exe](../../../src-tauri/target/release/bundle/nsis/TermPilot_0.1.0_x64-setup.exe)
- SHA-256：`A34B97597EA3E733EBD5222C421DF61F8FD6B6D986B02E189DB30DDD71C4170B`
- 安装包大小：3,469,308 bytes

当前构建启用 current-user 安装模式；未配置代码签名证书，因此以 SHA-256 作为完整性校验值。升级签名清单和回滚实机验证仍需发布环境证书与第二个版本包。
