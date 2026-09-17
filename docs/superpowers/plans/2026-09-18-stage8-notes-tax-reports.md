# 第八阶段票据台账、账期提醒与申报导出 Implementation Plan

> 实施时按 Task 顺序推进；每个 Task 独立测试、提交并更新 progress。涉及多模块开发时按 CLAUDE.md 使用 subagent 划分互不重叠的文件范围，由主 agent 统一集成、测试、提交和推送。

**Goal:** 补齐出纳票据管理（承兑/支票登记、背书、贴现、托收）、账期提醒、现金盘点、资金日报、个税扣缴申报表导出与增值税进项台账，并消化第七阶段 12 项 Minor。

**Architecture:** 新增 `notes.rs`（票据状态机与凭证联动）与 `cash_count.rs`（现金盘点）两个领域模块，复用 cashier.rs 已验证的模式（审批留痕、`insert_voucher` + `ensure_fund_voucher_lines`、红字冲正、`ensure_month_open`、`require_current_operator`）；提醒/日报/进项台账为纯查询（db.rs / cashier.rs）+ excel.rs 导出；四张新表走既有迁移事务框架。

**Tech Stack:** Rust + Tauri 2 + rusqlite、React 19 + TypeScript + Ant Design 6、rust_xlsxwriter。

**Spec:** `docs/superpowers/specs/2026-09-18-stage8-notes-tax-reports-design.md`（表结构/状态机/凭证分录以 spec 第 3、4 节为准，实施时逐字对照）

## Global Constraints

- 票据/盘点所有写命令调用 `db::ensure_month_open`（票据按登记月与操作月双查，与第七阶段冲正双月口径一致）。
- 已生成凭证的流转只能红字冲正（原凭证 active + 反向凭证，净影响 0），不直接作废；已月结月份须先反月结。
- 禁止绕过状态机直接 UPDATE 状态列。
- 金额容差 0.005；数据库保存正数。
- 资金科目凭证分录必带 `fund_account_id`，对方科目必空（`ensure_fund_voucher_lines` 既有约束）。
- 不把本地操作人包装成多用户权限。
- 中文 UI、中文错误、snake_case Tauri 命令、RFC3339 时间戳。
- 全量回归：`npx tsc -b`（勿用 `--noEmit`，本仓库为空检查）、`npm run lint`、`npm run build`、`cd src-tauri && cargo fmt --check`、`cargo check`、`cargo test --lib`（基线 255）。
- `docs/user-guide-v2.html` 为用户未跟踪文件，不得覆盖、删除或纳入提交。
- 个税申报表仅导出 Excel 格式，不做在线申报；进项台账明示「发票登记 ≠ 进项认证」。

---

## 8A：票据台账与账期提醒

### Task 1: 8A DDL、模型与迁移

**Files:**
- Modify: `src-tauri/src/db.rs`
- Modify: `src-tauri/src/models.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/db.rs`

**Tables:** `negotiable_instruments`、`instrument_endorsements`、`cash_count_sheets`、`cash_count_denominations`（DDL 逐字取 spec 3.1-3.3）；app_settings 键 `reminder_advance_days`（缺省 7）。

**Interfaces（Produces）:**
- `pub fn migrate_stage8_schema(conn: &Connection) -> AppResult<()>`（挂入既有迁移调用链，事务内执行 + `PRAGMA foreign_key_check`）
- models.rs：`NegotiableInstrument`、`InstrumentEndorsement`、`CashCountSheet`、`CashCountDenomination` 及 `InstrumentRegisterInput`、`CashCountCreateInput`；状态常量（`holding/endorsed_out/discounted/collecting/collected/issued_outstanding/paid/void`）用 `pub const` 集中定义供 notes.rs 引用
- 票据号唯一：`UNIQUE (instrument_type, instrument_no) WHERE status != 'void'` partial index

- [ ] 先写迁移失败测试：部分建表注入错误应整体回滚；坏外键拒绝。
- [ ] 写幂等测试：空库初始化、v0.7.0"旧库"（先跑 stage7 迁移）二次执行 stage8 迁移均无变化。
- [ ] 增加四表 DDL、唯一索引、FK；`reminder_advance_days` 缺省写入。
- [ ] 为每张表补 model/input 类型与状态常量。

**Acceptance:** 新旧库迁移幂等；异常不留半成品；`cargo test --lib` 基线 255 + 新增。

### Task 2: 票据状态机与凭证联动领域层

**Files:**
- Add: `src-tauri/src/notes.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/notes.rs`

**Interfaces:**
- Consumes: Task 1 表与模型；`accounting::insert_voucher` / `VoucherLineDraft`（fund_account_id 贯通）；`cashier::require_current_operator`、`db::ensure_month_open`、AMOUNT_TOLERANCE
- Produces: `register_instrument`、`endorse_instrument`、`discount_instrument`、`start_collection`、`confirm_collection`、`settle_issued_instrument`、`void_instrument`、`reverse_instrument_flow`（全部 `(&mut Connection, ..., operator: &str) -> AppResult<...>`，凭证与状态同事务）

**凭证分录（spec 4.1 逐字对照，缺省科目可被 counter_account_code 覆盖）：**
- 收到承兑：借 1121 / 贷 1122；开出承兑：借 2201 / 贷 2202；收到支票：借所选账户科目 / 贷 1122；开出支票：借 2201 / 贷所选账户科目
- 背书：借 2201 / 贷 1121；贴现：借 1002 实收 + 借 6603（票面−实收，差额 0 免腿）/ 贷 1121 票面；托收启动无凭证；到账确认：借 1002 / 贷 1121；开出兑付：借 2202 / 贷 1002

- [ ] 状态机门禁测试：托收中禁背书/贴现；终态不可再流转；void 仅限未流转；支票限定登记即终态（collected/paid）。
- [ ] 凭证断言测试：每个流转的借贷科目/金额/方向断言；贴现实收>票面拒绝；差额 0 免财务费用腿；背书金额=票面校验。
- [ ] `reverse_instrument_flow` 红字冲正测试：净影响 0、approval_events 留痕、已月结月份拒绝。
- [ ] 月结保护测试：登记月与操作月双查（跨月流转允许、已月结月拒绝）。
- [ ] 全部写命令 operator 署名 + log 埋点（operation_logs 由命令层 Task 3 接）。

**Acceptance:** 状态机全分支测试通过；凭证分录与 spec 4.1 完全一致。

### Task 3: 票据命令、API 与台账页面

**Files:**
- Modify: `src-tauri/src/commands.rs`、`src-tauri/src/lib.rs`
- Modify: `src/types/index.ts`、`src/api/index.ts`（含 mock）
- Add: `src/pages/NotesInstruments.tsx`
- Modify: `src/App.tsx`、`src/pages/OperationLogs.tsx`

**Interfaces:**
- Consumes: Task 2 全部领域函数
- Produces: Tauri 命令 `get_negotiable_instruments(query)`、`get_instrument_detail(id)`（含背书链）、`register_instrument`、`endorse_instrument`、`discount_instrument`、`start_collection`、`confirm_collection`、`settle_issued_instrument`、`void_instrument`、`reverse_instrument_flow`；写命令记 operation_logs

- [ ] 命令层薄封装 + 中文错误透出 + 操作日志（get 类不记）。
- [ ] 台账页：收到/开出/全部三 Tab + 状态/类型/月份/关键字筛选；行操作按状态驱动（持有→背书/贴现/托收/作废；托收中→到账确认；已开出→兑付）。
- [ ] 登记/背书/贴现弹窗（背书显示被背书人+事由+全额提示；贴现实收默认=票面可改，实时显示贴现息）。
- [ ] 详情 Drawer：票面信息 + 背书链时间线 + 关联凭证号。
- [ ] 红字冲正弹窗（原因必填）；OperationLogs 补命令中文映射；App.tsx 资金出纳组 +「票据台账」。
- [ ] mock 覆盖新命令（含状态机流转，default 分流规则沿用）。

**Acceptance:** 浏览器 mock 可走完 收到承兑→背书/贴现/托收到账 与 开出→兑付 全流程；桌面端命令清单注册齐全。

### Task 4: 账期提醒

**Files:**
- Modify: `src-tauri/src/db.rs`（`get_dashboard_reminders`）
- Modify: `src-tauri/src/commands.rs`、`src-tauri/src/lib.rs`
- Modify: `src/types/index.ts`、`src/api/index.ts`、`src/pages/Dashboard.tsx`

**Interfaces:**
- Produces: `pub fn get_dashboard_reminders(conn: &Connection, today: &str) -> AppResult<Vec<ReminderItem>>`；`ReminderItem { category: 'advance_due'|'instrument_due'|'payable_stuck', title, due_date, days_left: i64, amount: Option<f64>, ref_id: i64 }`

- [ ] 提醒查询测试：提前天数边界（当天/前 N 天/逾期）、N 从 app_settings 读取缺省 7、借款未清余额=借款额−累计核销（复用 Task 14 台账口径函数）、票据仅 holding/collecting/issued_outstanding 状态、滞留应付≥N 天。
- [ ] 命令 `get_dashboard_reminders`（只读不记日志）。
- [ ] Dashboard 提醒卡：三类分组、天数徽标（逾期红/临近橙）、金额 SensitiveText 脱敏、卡头可改提前天数（Select 写 app_settings）。
- [ ] mock 数据三类别各一条。

**Acceptance:** 仪表盘展示三类提醒；配置提前天数即时生效。

### Task 5: 8A 批次收尾

**Files:**
- Modify: `docs/superpowers/plans/2026-09-18-stage8-progress.md`（新建，见步骤）
- Modify: `.claude/memory/stage8-notes-tax-reports.md`（新建骨架）

- [ ] 新建 `docs/superpowers/plans/2026-09-18-stage8-progress.md`（当前基线/目标/批次表/记录模板，格式照 stage7-progress）。
- [ ] 全量回归六命令过；测试数记入 progress。
- [ ] 批次记录：完成项/关键决策/未完成风险/commit 区间。

**Acceptance:** 批次可从 progress 文件完整恢复上下文。

---

## 8B：现金盘点与资金日报

### Task 6: 现金盘点单

**Files:**
- Add: `src-tauri/src/cash_count.rs`
- Modify: `src-tauri/src/lib.rs`、`src-tauri/src/commands.rs`
- Modify: `src/types/index.ts`、`src/api/index.ts`（含 mock）
- Add: `src/pages/CashCount.tsx`
- Modify: `src/App.tsx`、`src/pages/OperationLogs.tsx`

**Interfaces:**
- Consumes: Task 1 表；`accounting::insert_voucher`；`db::ensure_month_open`；账面余额派生复用 cashier.rs 资金日记账余额函数（公开或包一层）
- Produces: `create_count_sheet`（draft）、`update_count_sheet`（draft 可改，面额明细整表替换）、`confirm_count_sheet`（生成差异凭证）、`void_count_sheet`（draft 直接作废 / confirmed 走红字冲正）、`get_count_sheets`、`get_count_sheet_detail`；凭证：盘亏 借 1901/贷 1001，盘盈 借 1001/贷 1901，差异 0 免凭证

- [ ] 领域测试：面额合计=实存校验；差异自动算；差异≠0 原因必填；差异 0 免凭证；盘亏/盘盈凭证双向断言；confirmed 后 update 拒绝；confirmed 作废走红字冲正（净影响 0）；账户限 cash 类型；`ensure_month_open(belong_month)`。
- [ ] 命令层 + 操作日志中文映射。
- [ ] 页面：盘点单列表（月份筛选+状态）+ 新建弹窗（账户限现金类、账面余额自动带出、快速模式填总额/面额模式 100/50/20/10/5/1/0.5/0.1 张数自动合计）+ 差异醒目显示 + 详情 Drawer（含凭证号）。
- [ ] App.tsx 资金出纳组 +「现金盘点」；mock 覆盖。

**Acceptance:** 盘点全流程（新建→录实存→确认→凭证）测试与 mock 走通。

### Task 7: 资金日报

**Files:**
- Modify: `src-tauri/src/cashier.rs`（`get_fund_daily_report`，与日记账同源）
- Modify: `src-tauri/src/excel.rs`（`export_fund_daily_report`）
- Modify: `src-tauri/src/commands.rs`、`src-tauri/src/lib.rs`
- Modify: `src/types/index.ts`、`src/api/index.ts`（含 mock）
- Modify: `src/pages/FundJournals.tsx`、`src/pages/OperationLogs.tsx`

**Interfaces:**
- Produces: `pub struct FundDailyReport { date: String, accounts: Vec<FundDailyAccountRow>, entries: Vec<FundDailyEntryRow>, trend: Vec<FundDailyTrendPoint> }`；`FundDailyAccountRow { account_id, account_name, opening, income, expense, closing }`；命令 `get_fund_daily_report(date)`、`export_fund_daily_report(date, path)`

- [ ] 查询测试：期初+收入−支出=期末勾稽（多账户）；当日无业务时 opening=closing；跨月边界（月初日期取上月期末）；trend 7 点含当日。
- [ ] excel 导出测试：两 sheet（账户汇总+明细）行数与合计断言；文件名 `资金日报_YYYYMMDD.xlsx` 由前端 save 对话框定，后端只收 path。
- [ ] FundJournals 页头加「导出日报」按钮（日期取当前所选月份的今天或用户选日，save 对话框 + 敏感导出门禁与日记账导出一致）。
- [ ] 命令注册 + 日志映射（get 不记、export 记）+ mock。

**Acceptance:** 日报勾稽测试通过；导出 Excel 结构正确。

### Task 8: 8B 批次收尾

**Files:**
- Modify: `docs/superpowers/plans/2026-09-18-stage8-progress.md`

- [ ] 全量回归六命令过；批次记录追加 progress。

**Acceptance:** 同 Task 5。

---

## 8C：申报导出、进项台账与收尾

### Task 9: 个税扣缴申报表导出

**Files:**
- Modify: `src-tauri/src/excel.rs`（`export_tax_withholding_declaration`）
- Modify: `src-tauri/src/commands.rs`、`src-tauri/src/lib.rs`
- Modify: `src/types/index.ts`、`src/api/index.ts`（含 mock）
- Modify: `src/pages/SalaryCalculate.tsx`、`src/pages/ExportCenter.tsx`、`src/pages/OperationLogs.tsx`

**Interfaces:**
- Produces: 命令 `export_tax_withholding_declaration(month: String, path: String) -> AppResult<()>`；列序：姓名/身份证号/收入额/基本养老保险/基本医疗保险/失业保险/住房公积金/减除费用/专项附加扣除/累计应纳税所得额/当期预扣率/速算扣除数/累计已预扣税额

- [ ] 三险拆列逻辑测试：有台账员工按台账各险种个人费率拆 `salary_results.social_insurance`；无台账按全局规则各险种个人比例拆；比例和≠100% 时三列退回合并展示（养老列=汇总、医疗/失业列空）并整表尾注。
- [ ] 门禁测试：仅 `status='已锁定'` 月份可导出，草稿/无数据月份返回中文错误。
- [ ] Excel 测试：行数=锁定员工数、合计行断言（收入额/三险/公积金/已预扣列合计）。
- [ ] SalaryCalculate 工具栏「扣缴申报表」按钮（锁定态可用）+ ExportCenter 增类型 + 命令注册 + 日志映射 + mock。

**Acceptance:** 锁定月份导出列齐合计对；未锁定被拒。

### Task 10: 增值税进项台账

**Files:**
- Modify: `src-tauri/src/db.rs` 或 `src-tauri/src/cashier.rs`（`get_input_tax_ledger`，发票维度放 db.rs 与 invoices 查询同域）
- Modify: `src-tauri/src/excel.rs`（`export_input_tax_ledger`）
- Modify: `src-tauri/src/commands.rs`、`src-tauri/src/lib.rs`
- Modify: `src/types/index.ts`、`src/api/index.ts`（含 mock）
- Add: `src/pages/InputTaxLedger.tsx`
- Modify: `src/App.tsx`、`src/pages/OperationLogs.tsx`

**Interfaces:**
- Produces: `pub fn get_input_tax_ledger(conn, from_month, to_month) -> AppResult<InputTaxLedgerReport>`（rows + 月度小计 + 区间合计）；行含发票代码/号码/开票日期/销方名称/销方税号/不含税/税额/价税合计/费用类型/关联报销单号/所属月份；排除 `status='void'`；报销单号经 `reimbursement_claim_invoices` 反查

- [ ] 查询测试：void 排除、区间过滤、月度小计与合计断言、关联报销单号反查（多报销关联取首个+计数）。
- [ ] Excel 导出测试：`进项台账_YYYYMM-YYYYMM.xlsx` 行数/合计。
- [ ] 页面：区间选择 + 表格（合计行）+ 顶部 Alert「发票登记 ≠ 进项认证，认证状态以税务系统为准」+ 导出按钮（敏感门禁）。
- [ ] App.tsx 票据报销组 +「进项台账」；命令注册 + 日志映射 + mock。

**Acceptance:** 台账口径测试与导出通过。

### Task 11: 第七阶段 12 项 Minor 打磨

**Files（按项定位，实施时以 stage7 memory「Minor 挂账」清单为准）:**
- `src/pages/FundDocuments.tsx`（项 1 核销草稿编辑透传 settlement_mode/due_date）
- `src/pages/BankTransactions.tsx` + `src-tauri/src/cashier.rs`（项 2 bank_manual 账户下拉收窄；项 3 幂等 contains 改结构化判断）
- `src-tauri/src/cashier.rs`（项 4 跨月冲正凭证口径注释+测试显式化；项 9 存量单 counter_account_code 回落说明与测试；项 10 同分平局断言语义）
- `src-tauri/src/data_safety.rs`（项 5 attachment_disk_stats O(n²)→单遍聚合）
- `src/pages/FinancialAnalysis.tsx` + `src-tauri/src/db.rs`（项 6 预算 expense_type 下拉化，选项来自 invoice_expense_types 表）
- `src/pages/Advances.tsx`（项 7 统计卡口径提示文案）
- `src-tauri/src/db.rs` + `src/api/index.ts`（项 8 报销 save 报错中文化；mock 状态机 default 不恒 true 改按命令语义）
- `src/pages/BankTransactions.tsx`（项 11 旧匹配伪未达提示与两步恢复引导文案）

- [ ] 每项：定位 → 修复 → 对应测试（项 1/2/6/7/11 前端为主验证 tsc/lint/build；项 3/4/5/8/9/10 后端补测试）。
- [ ] 项 12：unmatched_paid_batch_count 口径注释补文档（已切 allocation，写明新旧"或"关系）。
- [ ] 逐项提交或按文件分组提交，commit message 标注 `minor(stage7-N)`。

**Acceptance:** 12 项全部闭环；无新增回归。

### Task 12: 全量回归、文档四件套与发版评估

**Files:**
- Modify: `CLAUDE.md`（第八阶段段落 + Memory References + 架构摘要 notes.rs/cash_count.rs）
- Add: `.claude/memory/stage8-notes-tax-reports.md`（Task 5 骨架完善为已交付能力+已知边界+Windows 验收清单）
- Modify: `docs/superpowers/plans/2026-09-18-stage8-progress.md`（8C 完成记录）
- Modify: `docs/user-guide.html`（第八章新增票据台账/现金盘点/资金日报/申报导出/进项台账卡片，菜单速查表与版本历史同步）
- Modify: `src/pages/OperationLogs.tsx`（如仍有映射缺漏）

- [ ] 全量回归六命令 + graphify update。
- [ ] 导航核对（资金出纳 8 项/票据报销 3 项与 App.tsx 一致）。
- [ ] 文档四件套更新；Windows 验收清单固化（票据全流程+冲正、盘点差异凭证、提醒卡、三导出、既有 v0.7 项）。
- [ ] 向用户汇报发版评估（版本号建议 v0.8.0），经确认后走发版流程。

**Acceptance:** 阶段闭环；文档与代码一致；发版决策留痕。
