# 第二十阶段设计：凭证打印与 Web E2E 基建

日期：2026-10-25
状态：待审阅
基线：第十九阶段已交付（后端 418 测试，本阶段零后端改动）
路线图：`docs/superpowers/plans/2026-10-25-roadmap-v0.11-iterations.md`（迭代 6）

## 1. 背景与目标

两个互相成就的缺口：

- **凭证打印**：记账凭证（stage5 自动凭证 + stage13 手工凭证）只有屏显列表，纸质归档需要打印。工资条打印（payslip-*）已验证「屏显预览卡片 + @media print 只显示打印区」模式，凭证打印复用该模式做中式记账凭证版式（摘要/科目/借贷金额/合计/署名区）。
- **Web E2E 基建**：stage19 纯搬家重构时"行为保持靠类型门禁与人工走查，无 E2E"是显式短板；项目至今没有任何浏览器级 E2E。本阶段建立首个可重复跑的 Web E2E 运行器，并把凭证打印作为第一个被 E2E 覆盖的功能（打印按钮 → 预览弹层断言）。

约束：零后端改动、零新增 npm 依赖（E2E 用 Node 22 原生能力 + 系统 Chrome）。

## 2. 功能设计：行级凭证打印

### 2.1 入口与组件

- 入口在凭证页每行操作列加「打印」link 按钮（`src/pages/Vouchers.tsx:323-331`），点击 `openPrintModal([record])` 打开打印预览 Modal（当前单张；props 即 `vouchers: Voucher[]`，天然支持多张连续打印）。
- 组件 `src/components/VoucherPrintModal.tsx`（props: open/vouchers/onClose），Modal 挂载在凭证页（`src/pages/Vouchers.tsx:679-684`）。

### 2.2 中式记账凭证版式（每张一个 `.voucher-print-sheet`）

- **抬头**：居中标题「记账凭证」（已作废凭证随标题红字标注「（已作废）」防误用）；meta 行三段——凭证字号（如 记-202608-001）/ 凭证日期（中文格式 2026年08月05日）/ 第 N 期（belong_month）。
- **分录表**：四列 摘要 / 科目 / 借方金额 / 贷方金额；分录按 line_order 升序（与凭证页展开行同口径）；金额为零的格子留空（中式凭证习惯：借方行贷方列空白）。
- **科目列**：打开时异步拉 `getGlAccounts()` 建 编码→名称 映射，桌面模式展示「编码 名称」；拉取失败不影响打印，回退仅显示编码（预览 mock 无科目数据即此回退）。
- **补空行**：实际分录不足 5 行（MIN_PRINT_ROWS）时补占位空行，保持中式凭证固定行数的纸张观感。
- **合计行**：借/贷合计金额 fmtMoney（千分位 + 两位小数，消除浮点尾巴）；差额四舍五入到分后 < 0.005 判平衡——平衡标绿「（借贷平衡）」，不平衡标红「（借贷不平衡，请先核对）」。
- **署名区**：附件______张 / 制单 / 审核 / 记账 / 出纳 留空横线（纸质凭证手签）。
- **金额脱敏**：行金额与合计均经 `SensitiveText type="amount"`，与凭证页列表同口径。

### 2.3 打印门禁

与工资条打印一致：`useSecurity().isSensitiveRevealed` 未解锁时**禁用**「打印 / 另存 PDF」按钮，并在 Modal 顶部出 Alert「金额已脱敏。请先解锁敏感数据…」——避免打印出整页脱敏「¥ ****」废纸。解锁后点击调 `window.print()`（浏览器/Tauri WebView 原生打印对话框，支持另存 PDF）。

### 2.4 打印 CSS 与分页（src/styles/global.css:353-493）

- 屏显：`.voucher-print-area` 灰底滚动容器（max-height 60vh），每张凭证白底黑框卡片预览。
- `@media print`：`body *` visibility hidden → 仅 `.voucher-print-area` 及其后代可见，容器 position absolute 铺满页面；`.voucher-print-sheet` `page-break-after: always` 每张凭证独立分页，`:last-child` 改 auto 避免末尾多出空白页；表头底色置白适配黑白打印。
- 选择器全部 `voucher-print-*` 前缀，与工资条 `payslip-*` 完全不重叠，两个打印模式互不影响。

## 3. E2E 设计：无头 Chrome CDP 运行器

`scripts/web_e2e.mjs`（约 625 行）+ `package.json` scripts 加 `"e2e:web": "node scripts/web_e2e.mjs"`。自包含单文件、**零新增 npm 依赖**。

### 3.1 机制

- **栈**：Node 22 原生 fetch / WebSocket + `/usr/bin/google-chrome --headless` + CDP（`--remote-debugging-port`）+ 本地 vite dev server。
- **端口**：vite 与 chrome CDP 各从 20000-40000 随机端口池取（避开 5173 与常用段）；进程内 Set 互斥去重，防同池相撞导致 CDP 探活失败。
- **CDP 客户端**：原生 WebSocket + JSON-RPC，每条消息独立 id 按应答匹配；双层看门狗——WS open 10s、单条调用 30s 超时，防「端口可达但 WS 挂起」无限等待。
- **探活**：vite 以 `fetch /` 轮询（30s 上限，提前退出则带 stderr 尾部报错）；chrome 以 `fetch /json/version` + `/json` 取 page target 的 webSocketDebuggerUrl（20s 上限）。
- **页面操作**：`Runtime.evaluate`（returnByValue + awaitPromise）执行页面表达式；`waitFor` 轮询 `{ ok, info }` 断言，超时报错携带最后状态与页面可见文本片段；hash 路由跳转不整页刷新（保留应用内存态如已选操作人）。
- **清理**：finally 中 SIGKILL chrome/vite + 删除临时 user-data-dir；`exit`/SIGINT(130)/SIGTERM(143) 三处理器兜底杀子进程（SIGTERM 无处理器时 Node 直接终止、finally 不执行，vite/chrome 会成孤儿进程——实测发生过）。

### 3.2 环境净化坑（必须记录的实测坑）

**spawn chrome / vite 必须净化 env：`delete DISPLAY` 与 `delete DBUS_SESSION_BUS_ADDRESS`（cleanEnv()），否则无头 Chrome 无输出挂起**——桌面 Linux 会话的显示/总线变量会让 headless Chrome 尝试连接并不再产生任何输出，探活永远超时。所有 spawn（vite 与 chrome）统一走 cleanEnv()。

其它实测坑：

- **React 受控输入**：直接赋值 `input.value` 不触发 React onChange，必须原生 `HTMLInputElement.prototype.value` setter + `dispatchEvent(new Event('input', { bubbles: true }))`。
- **antd v6 渲染差异**（旧类名全部保留兜底）：两字中文按钮插入空格（「解锁」渲染为「解 锁」，匹配前压掉空白）；Select 无 `.ant-select-selection-item`/`.ant-select-selector`，选中值取 `.ant-select-content-has-value`、开合事件挂在 `.ant-select` 根节点；Modal 无 `.ant-modal-content`，正文在 `.ant-modal`；关闭后 `.ant-modal-wrap` 保留 DOM（display:none），弹层存在性以可见性判定而非 DOM 存在性。

### 3.3 场景（三组）

前置（非场景，失败不中断）：预关各页 PageTour（localStorage 写 `tour:<key>=done`）+ 页头下拉选择第一个操作人（重试 2 次）；B/C 前置失败时各自带原因 FAIL。

| 场景 | 步骤与断言 |
|---|---|
| A 启动与解锁 | 锁屏出现（「请输入启动密码」）→ 填入任意密码（预览 mock）点「解锁」→ 侧边菜单（工资核算助手 / 首页仪表盘）可见 |
| B 五页导航冒烟（stage19 重构回归） | bank-transactions / fund-documents / notes-instruments / fund-accounts / salary 五页逐页断言标题渲染；断言限定内容区 `.app-main`（防侧边栏同名菜单项假通过）并侦测操作人门禁拦截（「尚未选择当前操作人」） |
| C 凭证打印 | /#/vouchers → 断言**表格行内**「打印」按钮（不做「页面任意打印按钮」兜底——那会点中弹层内「打印 / 另存 PDF」削弱断言）→ 点击首行 → 预览弹层以「打印预览」+「凭证字号」双标识唯一锁定（只查「合计」无法区分手工凭证 Modal）→ 断言凭证字号回显、借贷金额列、金额数字（或脱敏 ****）→ 关闭并确认弹层消失 |

### 3.4 输出契约

```
E2E|<场景名>|OK|<耗时ms>|<一句话说明>
E2E|<场景名>|FAIL|<耗时ms>|<失败原因>
E2E_SUMMARY|pass=<n>|fail=<m>
```

全过退出码 0，否则 1；单场景失败不中断后续场景；基建失败（vite/chrome 起不来）三场景全部记 FAIL 仍满足契约。单行输出压掉竖线/换行并限长 300 字符，便于 CI 日志逐行 grep。

### 3.5 预览 mock：演示凭证

`src/api/index.ts:1079-1119` 新增两张 active 演示凭证（归属当前月，字段严格对齐前端 Voucher/VoucherLine 类型）：记-YYYYMM-001 工资计提（4 行分录，84,220.00 借贷平衡）与 记-YYYYMM-002 购办公用品（3 行分录，4,055.14 借贷平衡）。浏览器预览与 E2E 场景 C 均直接可见。

## 4. 不做范围

- 不做多张批量勾选打印 UI（组件 props 已支持，入口暂只接行级单张）
- 不做打印模板配置（纸张规格/边距/套打），固定 A4 纵向默认边距
- E2E 不覆盖 Tauri 桌面壳（WebView 打印对话框、托盘等仍靠真机手工验收，见 v0.10 验收清单第十一节补项）
- E2E 不接入 CI（本地 `npm run e2e:web` 手动跑；接 CI 需先解决 runner 的 Chrome 供给）

## 5. 验收与测试策略

- 类型与构建门禁：`npx tsc -b` + `npm run lint` + `npm run build` 全绿
- E2E 门禁（本阶段最强门禁）：`node scripts/web_e2e.mjs` 三场景全过（`E2E_SUMMARY|pass=3|fail=0`、exit 0），可重复跑通
- 后端零改动：`git status --porcelain src-tauri` 空输出（沙箱 cargo 冷编译超时，代替全量回归；418 存量测试主会话本地验证）
- 凭证打印真机目验（版式/分页/门禁）挂账 v0.10 验收清单第十一节补项

## 6. 关联

- 路线图迭代 6（`docs/superpowers/plans/2026-10-25-roadmap-v0.11-iterations.md`）
- 进度：`docs/superpowers/plans/2026-10-25-stage20-progress.md`；memory：`.claude/memory/stage20-voucher-print-web-e2e.md`
- 打印模式参照：工资条打印（stage6）；五页冒烟回归对象为 stage19 重构页面；凭证业务见 stage5/stage13
