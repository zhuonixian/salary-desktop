# 第十四阶段：安全债——找回凭据换绑与自动备份（已交付）

spec：`docs/superpowers/specs/2026-10-21-stage14-security-debt-design.md`
progress：`docs/superpowers/plans/2026-10-21-stage14-progress.md`

## 模块地图

- `security.rs`：verify_password_and_load_dek 公共前置（change_password 重构复用）/ gen_recovery_code（32 字符表去 O/0/I/1，24 位 4 连字符，与向导 generateRecoveryCode 同格式）/ rotate_recovery_code / update_security_question
- `security_commands.rs`：rotate_recovery_code（返回明文仅一次）/ update_security_question，留痕 security_recovery_code_rotated(_failed) / security_question_updated(_failed)，detail 无密钥材料
- `data_safety.rs`：backup_database_with_prefix（手动路径零改动）/ get+update_auto_backup_settings / run_auto_backup_if_due（幂等）/ prune_auto_backups（auto-daily-*.enc 按修改时间滚动保留）
- `commands.rs`：get_auto_backup_settings / update_auto_backup_settings / run_auto_backup_if_due 三命令
- 前端：SecurityCenter「找回凭据维护」卡（两 Modal：换绑恢复码一次性展示+复制+确认抄写；修改安全问题）；DataSafety「自动备份」卡（开关+保留份数+最近执行）；SecurityContext unlock 成功与 MonthClose 月结成功 fire-and-forget 触发

## 关键设计

- 换绑密钥语义：验证密码 → set_dek → 仅重包裹目标 KEK，其余两条 wrap 不动；成功清零 failed_attempts/lock_until，失败不累计（对齐 change_password）
- 自动备份触发=前端解锁成功/月结成功（无 Rust 定时器；托盘常驻使退出语义模糊）；条件链 enabled → 非空账套 → DEK 已加载 → 当日未备（Local 日历日）
- 产物复用手动加密路径：auto-daily-*.enc（BACKUP_MAGIC + DEK AES-GCM），恢复走既有"解锁后选加密备份包"；成功后写 last_data_backup_at（30 天提醒静默）+ last_auto_backup_at（当日去重）
- 保留份数 3-20 默认 5，超出按文件修改时间删最旧；salary-backup-（手动）与 auto-before-restore-（恢复快照）不动
- 空账套零打扰（is_empty_ledger 门禁）；失败仅 console.warn + 不弹窗，绝不阻塞解锁/月结

## 关联

- v0.10.0 发版验收清单第十节（stage14 手工验收）
- 安全债来源：stage13 spec 第 5 节"找回密码命令补全与自动备份（独立排期）"
