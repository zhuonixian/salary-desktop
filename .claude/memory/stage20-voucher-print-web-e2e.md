# 第二十阶段：凭证打印与 Web E2E 基建（已交付）

spec：`docs/superpowers/specs/2026-10-25-stage20-voucher-print-web-e2e-design.md`
路线图：`docs/superpowers/plans/2026-10-25-roadmap-v0.11-iterations.md`（迭代 6）
progress：`docs/superpowers/plans/2026-10-25-stage20-progress.md`

## 模块地图

行级中式凭证打印 + 项目首个 Web E2E 运行器，后端零改动、零新增 npm 依赖。

- **打印组件**：`src/components/VoucherPrintModal.tsx`（props: open/vouchers/onClose）——中式记账凭证版式（凭证字号/中文日期/第 N 期、摘要/科目/借贷四列表、不足 5 行补空行、合计行借贷平衡判定、附件/制单/审核/记账/出纳署名区）；科目列 getGlAccounts 编码→名称映射失败回退仅编码；金额走 SensitiveText 脱敏口径
- **打印入口**：`src/pages/Vouchers.tsx` 行级操作列「打印」按钮（openPrintModal，当前单张；props 天然支持多张连续打印，每张独立分页）
- **打印 CSS**：`src/styles/global.css:353-493` voucher-print-* 前缀（与工资条 payslip-* 完全不重叠）——屏显 60vh 滚动纸张卡片；@media print 仅打印区可见 + `.voucher-print-sheet` page-break-after: always（:last-child auto 防末尾空白页）+ 表头去底色
- **E2E 运行器**：`scripts/web_e2e.mjs`（自包含单文件）——Node 22 原生 fetch/WebSocket + /usr/bin/google-chrome 无头 CDP + vite dev 随机端口（20000-40000，进程内 Set 互斥）；`npm run e2e:web` 触发
- **mock 演示凭证**：`src/api/index.ts:1079-1119` 两张 active 演示凭证（记-YYYYMM-001 工资计提 4 行 84220.00 / 记-YYYYMM-002 购办公用品 3 行 4055.14，均借贷平衡、归属当前月），浏览器预览与 E2E 场景 C 直接可见

## 关键设计

- **CDP 环境净化坑（必须记录）**：spawn chrome / vite 必须 `delete DISPLAY` 与 `delete DBUS_SESSION_BUS_ADDRESS`（cleanEnv()），否则无头 Chrome 无输出挂起——桌面会话的显示/总线变量会让 headless Chrome 尝试连接并停止产生任何输出，探活永远超时
- **打印门禁**：未解锁敏感数据（isSensitiveRevealed）时禁用「打印 / 另存 PDF」+ Alert 提示（与工资条打印同模式），避免打出脱敏废纸；打印本体 `window.print()`
- **CDP 客户端自制**：原生 WebSocket + JSON-RPC 按 id 匹配应答；双层看门狗（open 10s / 单调用 30s）防「端口可达但 WS 挂起」
- **antd v6 + React 19 实测坑**（旧类名保留兜底）：受控输入须原生 value setter + input 事件；两字中文按钮渲染带空格（「解 锁」）匹配前压空白；Select 选中值在 `.ant-select-content-has-value`、开合事件挂 `.ant-select` 根节点（无 .ant-select-selector）；Modal 正文在 `.ant-modal`（无 .ant-modal-content）；弹层关闭判定用可见性而非 DOM 存在性（.ant-modal-wrap 留 DOM）
- **E2E 断言强度**：打印按钮只认表格行内按钮；预览弹层用「打印预览」+「凭证字号」双标识唯一锁定（区分手工凭证 Modal）；标题断言限定 `.app-main` 内容区防侧边栏同名菜单假通过
- **进程清理**：finally SIGKILL + exit/SIGINT(130)/SIGTERM(143) 三处理器（SIGTERM 无处理器时 Node 直接终止、finally 不执行，vite/chrome 成孤儿——实测发生过）

## 已知边界

- **E2E 走预览 mock 层，非真实 SQLite 后端**：运行器打的是 vite dev + src/api/index.ts 内存 mock（锁屏任意密码可解、演示凭证为静态数据），不覆盖 Tauri 命令、rusqlite 与真实状态机；后端仍靠 418 测试 + 真机手工验收
- Tauri WebView 打印对话框、纸质打印效果（版式/分页）需真机目验，挂账 v0.10 验收清单第十一节
- E2E 未接 CI（本地手动 `npm run e2e:web`）；多张批量打印入口与打印模板配置留池（spec 第 4 节）
