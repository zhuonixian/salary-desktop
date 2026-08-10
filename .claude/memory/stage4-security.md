---
name: stage4-security
description: 第四阶段安全配置能力摘要
---

# 第四阶段：安全配置

完整计划：`docs/superpowers/plans/2026-08-10-stage4-security-config.md`
spec：`docs/superpowers/specs/2026-08-10-stage4-security-config-design.md`
进度同步：`docs/superpowers/plans/2026-08-10-stage4-progress.md`

## 定位

在第三阶段本地财务管理基础上，加入应用访问安全、敏感数据加密、默认脱敏展示。

## 已交付能力

1. 启动密码 + 闲置自动锁 + 手动锁屏
2. 双层密钥（KEK + DEK）架构，发票图片 / 备份包 / OCR token 加密
3. 恢复码 + 安全问题双找回路径
4. SensitiveText / SensitiveStatistic 默认脱敏，二次密码解锁 5 分钟
5. 旧版迁移：发票图片与 OCR token 一次性加密
6. Tauri 安全配置收紧（CSP + assetProtocol scope + withGlobalTauri=false）

## 关键模块

- 后端：`src-tauri/src/security.rs` / `security_commands.rs` / `legacy_migration.rs`
- 前端：`src/contexts/SecurityContext.tsx`、`src/components/{LockScreen,SetupSecurity,SensitiveText,SensitiveStatistic,RevealPasswordModal}.tsx`、`src/pages/SecurityCenter.tsx`
- schema：`security_state` / `legacy_migration_state` 表、`invoices.image_encrypted` 字段
- 配置：`tauri.conf.json` 收紧 CSP / assetProtocol scope

## 测试门槛

```bash
npx tsc --noEmit
npm run lint
npm run build
cd src-tauri && cargo test --lib
cd src-tauri && cargo fmt --check
cd src-tauri && cargo check
```

## 不做范围（未来增强池）

- 整库加密（SQLCipher）
- 字段级加密
- 找回密码卡片完整功能（update_security_question / regenerate_recovery_code 命令）
- Employees 编辑 Modal 字段脱敏（用户主动编辑时已知内容，UX 取舍）
- SalaryRules 字段脱敏（InputNumber 编辑控件脱敏会破坏输入）
