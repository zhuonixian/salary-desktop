# 第十一阶段进度：提醒触达——OS 通知与托盘常驻

spec：`docs/superpowers/specs/2026-10-12-stage11-reminder-delivery-design.md`
plan：`docs/superpowers/plans/2026-10-12-stage11-reminder-delivery.md`
基线：v0.9.0 + 第十阶段（后端 372 测试）

## Task 状态

| # | 任务 | 状态 |
|---|---|---|
| 1 | Cargo（tray-icon feature + tauri-plugin-notification）+ capabilities（notification:default）+ npm 包 | ✅ |
| 2 | reminders.rs：四源聚合（征期/账期/月结/备份）+ 稳定 key + level 分级 | ✅ |
| 3 | 设置（local_notify_enabled/close_to_tray_enabled 默认双开）+ 提醒记忆（按日重置/合并幂等） | ✅ |
| 4 | 单测 8 个（四源命中/空账套零打扰/月结口径/备份阈值/排序/记忆跨日） | ✅（380 全绿） |
| 5 | 命令 6 条注册（get_due_notifications 等 + update_tray_badge） | ✅ |
| 6 | lib.rs：notification 插件 + 托盘（菜单/tooltip/左键）+ CloseRequested 关窗到托盘 | ✅ |
| 7 | 前端 ReminderChecker（+5s/30min/tray-check-reminders；权限会话仅请求一次；≤4 条/轮） | ✅（subagent 产出，主 agent 修 2 处 lint + 1 处 prefer-const） |
| 8 | 通知设置页「本地系统通知」Tab（双开关即时保存） | ✅ |
| 9 | 集成回归（cargo 380 / tsc 0 / lint 0）+ 文档四件套 | ✅ |

## 实现决策与偏差记录

- **征期窗口从上月起算（spec 偏差）**：spec 原写"当月起 2 个月"——实现时发现次月 15 日申报期次
  从当月起算时 days_left 恒 >15，整月漏报（如 11 月 1-14 日永远收不到 10 月申报提醒）。
  改为"上月起 2 个月"：每月 1 日起上期进入 15 天窗，16 日起本期逾期（overdue）持续提醒。
- **借款提醒要求 status='settled'**：仪表盘提醒卡口径为"已放款未收回"，approved 未放款不提醒（测试已对齐）。
- **月结活动口径**：上月存在 locked 工资行或任意凭证（belong_month 命中）即提醒；正式月结（month_closes.month status='closed'）后静默。
- **备份阈值**：last_data_backup_at 超 30 天或从未备份（含时间戳损坏）→ 提醒；要求非空账套（is_empty_ledger 复用第十阶段 16 表判定）。
- **空账套零打扰额外加码**：征期源也跳过空账套（全新安装无申报义务），超出 spec 假设（spec 原以为空账套天然无征期，实际征期规则不看工资数据）。
- **聚合函数注入 today**：collect_* 内部接收 NaiveDate 参数（get_due_notifications 统一取 Local::now），保证可测性。
- **关窗到托盘 fail-safe**：读设置失败（锁毒化等）保守驻留托盘；托盘「退出」是唯一退出入口。
- **subagent 并发额度超限**：前端 subagent 中途失败但产出基本完整（tsc 已过），主 agent 修 3 处 lint 后复验通过。

## 挂账

- 托盘与 OS 通知手工验收（Windows exe + macOS dmg）：通知弹出与每日一次去重、托盘菜单/tooltip/左键、关窗到托盘后仍提醒、macOS 首次授权弹窗、Linux 无托盘环境静默降级
- 手册补第十一章说明（下一发版前统一更新手册）
