# 第十五阶段进度：上手体验——导航收敛与页面引导

spec：`docs/superpowers/specs/2026-10-25-stage15-ux-navigation-tour-design.md`
路线图：`docs/superpowers/plans/2026-10-25-roadmap-v0.11-iterations.md`（迭代 1）
基线：第十四阶段已交付（纯前端迭代，无后端变更）

## Task 状态

| # | 任务 | 状态 |
|---|---|---|
| 1 | 15A 导航重组：资金出纳 9 项拆为资金收付/资金台账，票据台账移入票据与报销；路由 key 集合不变 | ✅（31 叶子路由清点一致，9 组每组 ≤4） |
| 2 | 15B PageTour 共享组件（首启自动弹/关闭记忆/reopenTick 复看） | ✅（+tourTarget 独立文件：antd v6 target 不收字符串、react-refresh 单一导出） |
| 3 | 15B 四页接入（银行对账/收付款单/票据台账/记账凭证）+ data-tour 锚点 | ✅ |
| 4 | 回归：tsc + build + lint 全绿；菜单条目清点 | ✅ |
| 5 | 文档：手册菜单速查重写 + memory + CLAUDE.md + 路线图勾销 + push | ✅（含手册正文 16 处路径引用与 Reimbursements.tsx UI 文案同步） |

## 实现决策与偏差记录

- antd v6 Tour `target` 类型只接受元素 getter（`HTMLElement | (() => HTMLElement) | (() => null)`），不接受字符串选择器也不接受返回联合类型的函数 → `tourTarget` 以 `as HTMLElement` 收窄，运行时找不到元素返回 null 走 antd 居中展示兜底。
- BankTransactions 首版 tourTick 误加进子组件（ReconciliationWorkbench 的 state 区）——tsc 报 Cannot find name 后定位到页面主组件重新声明。
- 导航重组的连锁引用比预期多：手册正文 16 处「资金出纳 → X」路径、菜单速查表、Reimbursements.tsx Tooltip 文案、stage8 memory/progress——全部同步，避免文档指路错误。
- 首启 Tour 与 Dashboard 检查清单卡并存不冲突：Tour 只在 4 个复杂流程页首入弹出，Dashboard 引导职责不变。

## 挂账

- Tour 视觉效果真机目验（遮罩定位/复看按钮），并入 v0.10 验收清单 UI 项（B 级抽验）
- 命令面板 / 角色视图 / 首页 Tour：spec 第 5 节不做范围，未来增强池
