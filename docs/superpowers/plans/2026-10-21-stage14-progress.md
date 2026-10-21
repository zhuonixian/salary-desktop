# 第十四阶段进度：安全债——找回凭据换绑与自动备份

spec：`docs/superpowers/specs/2026-10-21-stage14-security-debt-design.md`
基线：v0.10.0 发版准备完成（e98ef95，后端 397 测试 → 本阶段 410）

## Task 状态

| # | 任务 | 状态 |
|---|---|---|
| 1 | 14A 后端：verify_password_and_load_dek 重构 + gen_recovery_code + rotate/update_security_question + 命令注册 | ✅ |
| 2 | 14A 测试：rotation_tests 9 项（格式一致性/新旧码互斥/另两条 wrap 不动/失败不累计/清锁/未初始化/问题校验） | ✅ |
| 3 | 14B 后端：models + backup_database_with_prefix + 设置/执行/清理 + 三命令注册 | ✅ |
| 4 | 14B 测试：自动备份 4 项（产物 .enc+当日去重/三种跳过/默认与裁剪/滚动清理不动手动与恢复快照） | ✅ |
| 5 | 前端：安全中心找回凭据维护卡（两 Modal）+ api 封装 | ✅ |
| 6 | 前端：数据安全自动备份卡 + SecurityContext 解锁触发 + MonthClose 月结触发 | ✅ |
| 7 | 全量回归 + 文档四件套 + push | ✅（cargo 410 绿 / tsc+build+lint 绿；memory/spec/progress/CLAUDE/手册/验收清单十节齐） |

## 实现决策与偏差记录

- **reset_password_by_question 只收答案不收问题**（问题仅展示、答案即凭据）——测试初版按"问题+答案"双参写错，按真实签名修正。
- **change_password 重构**：验证段抽出为 verify_password_and_load_dek 供三处换绑复用，行为不变（既有测试 change_password_does_not_touch_failed_attempts 仍绿）。
- **自动备份默认开启**：数据安全优先；空账套与未解锁天然不执行，数据安全页有显式开关与份数配置。
- **触发放前端不放 Rust 定时器**：解锁/月结两个动作天然携带"该备份了"的语义，且锁屏跳过逻辑免实现；后端 run_auto_backup_if_due 自身幂等兜底。
- **手动备份命令零改动**：backup_database 委托 backup_database_with_prefix("salary-backup")，auto 路径用 "auto-daily" 前缀。

## 挂账

- Windows exe 手工验收（已并入 v0.10 验收清单第十节）：换绑全流程 / 自动备份解锁触发与月结触发 / 滚动清理
- 自动备份外置目录/网盘目标、备份失败 OS 通知（spec 不做范围）
