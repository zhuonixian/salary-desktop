# 第十五阶段：上手体验——导航收敛与页面引导（已交付）

spec：`docs/superpowers/specs/2026-10-25-stage15-ux-navigation-tour-design.md`
progress：`docs/superpowers/plans/2026-10-25-stage15-progress.md`
路线图：`docs/superpowers/plans/2026-10-25-roadmap-v0.11-iterations.md`（迭代 1）

## 模块地图

- `App.tsx` menuItems 重组（纯前端，路由 key 全部不变）：票据报销(3) → **票据与报销**(4，+票据台账)；资金出纳(9) → **资金收付**(4：资金账户/收付款单/付款批次/银行对账) + **资金台账**(4：资金日记账/应收台账/借款备用金/现金盘点)；共 9 组每组 ≤4
- `components/PageTour.tsx`：antd Tour 封装——首启自动弹（localStorage `tour:{key}`=done 记忆）、reopenTick 递增复看；`components/tourTarget.ts`：CSS selector → 元素 getter（antd v6 target 不收字符串；找不到元素走 antd 居中兜底）
- 四页接入（页头「指引」按钮 + data-tour 锚点）：BankTransactions / FundDocuments / NotesInstruments / Vouchers，每页 4-5 步流程指引

## 关键设计

- 菜单联动全部分组无关：menuPathToGroupKey 动态生成、操作人门禁按路径、openKeys 按组 key 动态——重组零联动改动
- antd v6 Tour target 类型只收 `() => HTMLElement`（不接受 HTMLElement|null 联合），tourTarget 用 as HTMLElement 收窄，运行时 null 走居中兜底
- react-refresh 规则要求组件文件单一导出 → tourTarget 独立文件
- 文档连锁同步：手册正文 16 处「资金出纳 → X」路径 + 菜单速查表 + Reimbursements.tsx UI 文案 + stage8 memory/progress 历史路径引用

## 已知边界

- Tour 视觉效果（遮罩定位）需真机目验，并入 v0.10 验收（导航与指引为 UI 项，B 级抽验）
- 命令面板/角色视图/首页 Tour 不做（spec 第 5 节）
