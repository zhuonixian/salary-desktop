# 第十一阶段：提醒触达——OS 通知与托盘常驻（进行中）

spec：`docs/superpowers/specs/2026-10-12-stage11-reminder-delivery-design.md`
plan：`docs/superpowers/plans/2026-10-12-stage11-reminder-delivery.md`
progress：`docs/superpowers/plans/2026-10-12-stage11-progress.md`

## 模块地图

- `reminders.rs`（新）：四源聚合（征期/账期/月结/备份）→ DueNotification（稳定 key，title/body 不含金额）；设置（local_notify_enabled / close_to_tray_enabled，默认均开）；提醒记忆（local_notify_memory 按日重置）
- `lib.rs`：tauri-plugin-notification 注册 + tray-icon（菜单：打开/立即检查/退出；tooltip 待办数）+ CloseRequested 拦截（关窗到托盘）
- 前端：`ReminderChecker`（启动 +5s / 每 30 分钟 / 监听 tray-check-reminders）挂 AppLayout；通知设置页「本地系统通知 + 关窗到托盘」区块

## 关键设计

- 触达分层：OS toast（每条每天一次，≤4 条/次）+ 托盘 tooltip 常驻待办数 + 应用内卡片（既有）
- 零外联不变：OS 通知为本地通道；邮件摘要/企业微信钉钉媒介留未来增强池
- 空账套零打扰：备份源要求非空账套，其余源空账套天然无数据
- 锁屏/未初始化跳过检查；通知权限仅启动请求一次，拒绝后静默降级为仅托盘 badge

## 已知边界

- 托盘 UI 与 OS 通知行为无法单测覆盖，Windows/macOS 手工验收挂账
- 点击通知为系统默认聚焦窗口，不做深链跳转
