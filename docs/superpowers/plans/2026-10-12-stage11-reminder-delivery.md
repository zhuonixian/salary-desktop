# 第十一阶段执行计划：提醒触达——OS 通知与托盘常驻

日期：2026-10-12
spec：`docs/superpowers/specs/2026-10-12-stage11-reminder-delivery-design.md`
基线：v0.9.0 + 第十阶段（后端 372 测试）

## 批次与任务

- **11A 通知聚合**（Task 1-3）：reminders.rs 新模块（四源聚合 + DueNotification + 稳定 key）→ 设置/记忆命令 → 单测
- **11B OS 通知**（Task 4-6）：tauri-plugin-notification（Cargo + capability + npm）→ 前端 ReminderChecker（启动/30 分钟/托盘事件）→ 通知设置页「本地系统通知」区块
- **11C 托盘常驻**（Task 7-8）：tray-icon feature + 托盘菜单/tooltip + 关窗到托盘（CloseRequested 拦截）+ update_tray_badge
- 收尾（Task 9）：集成回归、文档四件套

## 文件归属（防冲突）

| 文件 | 负责 |
|---|---|
| src-tauri/src/reminders.rs（新建）+ Cargo.toml + capabilities/default.json + lib.rs + commands.rs | 主 agent |
| src/components/ReminderChecker.tsx（新建）+ src/api/index.ts + src/types/index.ts + src/pages/NotificationSettings.tsx | subagent |

## 后端命令契约（已定，前后端共同遵守）

```text
get_due_notifications() -> DueNotification[]
  // { key, kind, title, body, due_date: string|null, level: 'overdue'|'soon'|'info' }
  // kind: tax_filing / advance_due / instrument_due / payable_stuck / month_close / backup
get_local_notify_settings() -> { enabled: bool, close_to_tray: bool }   // 默认均 true
save_local_notify_settings({ data: { enabled?: bool, close_to_tray?: bool } }) -> bool
get_notify_memory() -> { date: 'YYYY-MM-DD', keys: string[] }           // 跨日自动重置
mark_notified({ keys: string[] }) -> bool                               // 合并进今日集合
update_tray_badge({ count: number }) -> ()                              // 托盘 tooltip
```

托盘菜单「立即检查」→ Rust emit `tray-check-reminders` → 前端 `listen('tray-check-reminders')`。

## 实现要点

- 聚合四源复用既有查询：onboarding::get_tax_calendar（当月起 2 月，overdue 或 ≤15 天）、
  db::get_dashboard_reminders（内部已按提前天数过滤）、is_month_closed + 上月活动判定、
  last_data_backup_at 30 天阈值（要求非空账套，空账套零打扰）
- **通知 title/body 不含金额等敏感数字**（对齐 stage9 留痕口径）；锁屏/未初始化时前端跳过检查
- 每条提醒每天至多弹一次：memory JSON 按日重置；单次至多 4 条 toast，超出并入末条"…等 N 项"
- 关窗到托盘默认开启：CloseRequested → hide + prevent_close；托盘「退出」为唯一退出入口（行为变化已写入 spec）
- 通知权限：仅启动时请求一次，拒绝后静默降级（仅托盘 badge 触达）
- Cargo：tauri features 增 `tray-icon`；新增 `tauri-plugin-notification = "2"`；capability 增 `notification:default`；npm 增 `@tauri-apps/plugin-notification`

## 测试与验收

- cargo test --lib：四源命中/不命中、key 稳定、level 分级、空账套零打扰、月结活动口径、备份阈值、memory 跨日重置/合并幂等
- npx tsc -b + npm run lint（subagent 自验 + 主 agent 复验）
- 手工验收挂账：通知弹出与每日去重、托盘菜单/tooltip、关窗到托盘后仍提醒、macOS 授权弹窗
