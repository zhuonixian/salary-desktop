# 第十一阶段设计（草案）：提醒触达——OS 通知与托盘常驻

日期：2026-10-12
状态：草案（待审阅）
基线：v0.9.0 + 第十阶段（后端 372 测试）

## 1. 背景与目标

账期提醒卡、征期提醒卡都只在打开应用时可见——"提醒要打开应用才看得到"是本地工具的触达短板（第十阶段 spec 明确留作独立立项）。第十一阶段补齐触达链路，**零外联原则不变**（OS 本地通知不联网）：

1. **运行中触达**：应用运行期间（含最小化/关窗到托盘）定时聚合四类提醒，经系统通知弹出，免盯着页面
2. **常驻锚点**：托盘图标常驻，tooltip 显示待办数；关窗不再等于退出（可配）
3. **克制不打扰**：每条提醒每天最多弹一次（指纹去重），通知正文不带金额等敏感数字

**明确不做**（未来增强池）：邮件摘要推送（应用关闭时无后台进程可发送，价值有限且引入定时外联）、企业微信/钉钉媒介、点击通知深链跳转（各平台行为不一，保持系统默认聚焦窗口）。

## 2. 范围

| 模块 | 深度 | 新键/依赖 |
|---|---|---|
| 通知聚合（reminders.rs 新模块） | 四源聚合 + 稳定去重键 | 无新表 |
| 本地通知设置 | 总开关 + 关窗到托盘开关 | app_settings.local_notify_enabled / close_to_tray_enabled |
| 提醒记忆（去重指纹） | 按日自动重置 | app_settings.local_notify_memory |
| OS 通知通道 | tauri-plugin-notification | Cargo + capabilities + npm 包 |
| 托盘常驻 | 图标/tooltip 待办数/菜单（打开/立即检查/退出）+ 关窗隐藏 | tauri feature tray-icon |
| 前端检查器 | 启动 + 每 30 分钟 + 托盘事件触发 | ReminderChecker 组件 |

## 3. 提醒聚合（reminders.rs）

### 3.1 统一条目

```rust
pub struct DueNotification {
    pub key: String,        // 稳定键："{kind}:{唯一标识}:{关键日期}"
    pub kind: String,       // tax_filing / advance_due / instrument_due / payable_stuck / month_close / backup
    pub title: String,      // 不含金额（复用 stage8 提醒卡 title 口径）
    pub body: String,       // 不含金额等敏感数字（对齐 stage9 OperationLogs detail 口径）
    pub due_date: Option<String>,
    pub level: String,      // overdue | soon(≤3天) | info
}
```

### 3.2 四源口径

| 源 | 取数 | 纳入条件 | key 示例 |
|---|---|---|---|
| 征期 | 复用 onboarding::get_tax_calendar（当月起 2 个月） | overdue 或 days_left ≤ 15，且状态 pending | `tax_filing:eit_withholding:2026-10` |
| 账期 | 复用 db::get_dashboard_reminders（内部已按提前天数过滤） | 全部返回项 | `advance_due:{ref_id}:{due_date}` 等 |
| 月结 | is_month_closed（上月） | 上月未正式月结 且 上月有锁定工资或凭证 | `month_close:2026-09` |
| 备份 | app_settings.last_data_backup_at | 有业务数据（非空账套）且从未备份或超 30 天 | `backup:{today}` |

空账套只可能命中备份源，而备份源要求非空账套——全新安装零打扰。

### 3.3 设置与记忆

- `local_notify_enabled`（'0'/'1'，默认 '1'）、`close_to_tray_enabled`（默认 '1'）
- `local_notify_memory`：JSON `{ "date": "YYYY-MM-DD", "keys": [...] }`；读取时 date ≠ 今天自动重置为空（跨日重新提醒）；`mark_notified(keys)` 合并写入今日集合

### 3.4 命令契约（后端 ↔ 前端）

```text
get_due_notifications() -> DueNotification[]
get_local_notify_settings() -> { enabled: bool, close_to_tray: bool }
save_local_notify_settings({ data: { enabled: Option<bool>, close_to_tray: Option<bool> } }) -> bool
get_notify_memory() -> { date: string, keys: string[] }        // 跨日自动重置
mark_notified({ keys: string[] }) -> bool                      // 合并进今日集合
update_tray_badge({ count: number }) -> ()                     // 托盘 tooltip
```

托盘菜单「立即检查」→ Rust `app.emit("tray-check-reminders")` → 前端监听执行一次检查。

## 4. OS 通知通道

- 依赖：`tauri-plugin-notification = "2"`（Cargo）+ `@tauri-apps/plugin-notification`（npm）+ capability `"notification:default"`
- 发送前 `isPermissionGranted` / `requestPermission`（macOS 首次授权），失败静默降级（不报错弹窗）
- 单次检查最多弹 4 条，超出部分并入第 4 条正文"…等 N 项"；标题固定「工资核算助手」
- 点击行为：系统默认（聚焦窗口），不做深链

## 5. 托盘常驻（tauri feature tray-icon）

- 菜单：打开主窗口 / 立即检查提醒 / 退出
- tooltip：`工资核算助手（N 项提醒待处理）`，无待办为 `工资核算助手`；前端每次检查后调 update_tray_badge 刷新
- 关窗行为：CloseRequested 时若 close_to_tray_enabled → 隐藏窗口并 prevent_close（应用继续运行、检查器继续工作）；托盘「退出」才真正退出
- **行为变化显式声明**：默认关窗到托盘，托盘菜单提供唯一退出入口；单测不覆盖托盘 UI（挂账手工验收）
- 锁屏联动：isLocked 或未初始化时检查器跳过（通知不带敏感内容，但避免锁屏态打扰）

## 6. 前端检查器（ReminderChecker）

- 挂载于 AppLayout；触发：挂载后延迟 5s、每 30 分钟、监听 `tray-check-reminders`
- 流程：settings.enabled？→ get_due_notifications → get_notify_memory → 差集 = 未提醒项 → 逐条 toast（≤4 条）→ mark_notified → update_tray_badge(当前待办总数)
- 使用 SecurityContext 的 isInitialized/isLocked 守卫

## 7. 错误处理与边界

- 通知权限被拒 → 本次会话静默跳过（每次检查重试请求一次权限？否——仅启动时请求一次，拒绝后只留托盘 badge 触达）
- DB 锁竞争：聚合命令走常规锁内查询（查询轻量）；托盘菜单事件不查 DB
- memory JSON 损坏 → 视为空集合重建
- Linux 无系统托盘服务/通知守护（无头环境）→ 插件调用失败静默，不影响主流程

## 8. 测试策略

- reminders.rs 单测（内存库造数）：四源各自命中/不命中、key 稳定性、level 分级、空账套零打扰、月结有活动/无活动口径、备份 30 天阈值、memory 跨日重置与合并幂等
- 前端：npx tsc -b + npm run lint
- 挂账手工验收（Windows exe）：通知弹出与每日一次去重、托盘 tooltip/菜单、关窗到托盘后仍收到提醒、macOS 首次授权弹窗

## 9. 交付批次

- **11A**（Task 1-3）：reminders.rs 聚合 + 设置/记忆命令 + 单测
- **11B**（Task 4-6）：notification 插件接入（Cargo/capability/npm）+ 前端检查器 + 通知设置区块
- **11C**（Task 7-8）：托盘常驻 + 关窗到托盘 + badge 刷新
- 收尾：集成回归、导航核对、文档四件套
