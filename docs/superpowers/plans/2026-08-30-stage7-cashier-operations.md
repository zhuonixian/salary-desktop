# 第七阶段出纳运营闭环 Implementation Plan

> 实施时按 Task 顺序推进；每个 Task 独立测试、提交并更新 progress。涉及多模块开发时按 CLAUDE.md 使用 subagent 划分互不重叠的文件范围，由主 agent 统一集成、测试、提交和推送。

**Goal:** 将现有工资/报销付款和银行流水扩展为账户级收付款、账面日记账、多对多银行对账、审批留痕及借款核销闭环，同时保持本地单机定位和旧数据兼容。

**Architecture:** 新增 `cashier.rs` 作为出纳领域层；账面日记账从带 `fund_account_id` 的凭证分录派生；通用资金单通过状态机驱动付款、凭证和冲正；银行流水与资金分录通过 allocation 多对多核销；React Context 统一业务月份和当前操作人。

**Tech Stack:** Rust + Tauri 2 + rusqlite、React 19 + TypeScript + Ant Design 6、rust_xlsxwriter、现有 AES-GCM/DEK 安全模块。

**Spec:** `docs/superpowers/specs/2026-08-30-stage7-cashier-operations-design.md`

## Global Constraints

- 不把本地操作人包装成真正多用户权限；UI 必须明确其为业务署名。
- 禁止通过直接 UPDATE 绕过状态机；状态变化和 approval_events 必须同事务。
- 已结算单据只能冲正，不直接作废或删除原凭证。
- 所有资金写操作调用 `db::ensure_month_open`。
- 金额比较容差 0.005；数据库保存正数，方向由业务类型/借贷字段表达。
- 新增资金账户辅助核算后，现金/银行凭证分录不得缺 `fund_account_id`。
- 迁移不猜测多账户归属；不确定数据保留 NULL 并进入待归集。
- DDL 重建必须事务化、保留 ID/索引/外键并运行 `PRAGMA foreign_key_check`。
- 中文 UI、中文错误、snake_case Tauri 命令、RFC3339 时间戳。
- 使用 `apply_patch` 编辑；不覆盖用户现有未提交文件。
- 全量回归：`npx tsc --noEmit`、`npm run lint`、`npm run build`、`cd src-tauri && cargo fmt --check`、`cargo check`、`cargo test --lib`。

## Gate 0：上线前基线与旧库样本

### Task 0: 第六阶段 Windows 验收与迁移样本固化

**Files:**
- Modify: `docs/superpowers/plans/2026-08-22-stage6-progress.md`
- Add: `src-tauri/tests/fixtures/` 下脱敏旧库样本（若仓库不接受二进制，则增加构造脚本/测试 helper）
- Modify: `docs/superpowers/plans/2026-08-30-stage7-progress.md`

- [ ] 使用 v0.6.1 Windows exe 验收第六阶段待验项：余额表、年结/反结、社保、年度个税、工资条、同期列。
- [ ] 执行一次加密备份、恢复、数据库体检，记录结果。
- [ ] 构造至少三类旧库：无流水、单账户旧流水、有付款批次和旧匹配。
- [ ] 保存当前 141 个 Rust 测试及前端检查基线。
- [ ] 若 Gate 0 出现阻断缺陷，先修复并发布补丁，不带入第七阶段批量改造。

**Acceptance:** 第六阶段手工验收有记录，旧库迁移测试可重复执行。

## 7A：基础底座

### Task 1: 全局业务月份 Context

**Files:**
- Add: `src/contexts/BusinessMonthContext.tsx`
- Modify: `src/main.tsx` 或 `src/bootstrap.tsx`
- Modify: `src/App.tsx`
- Modify: `src/pages/{Dashboard,Attendance,OcrCenter,PunchCard,SalaryCalculate,Invoices,Reimbursements,Payments,BankTransactions,Vouchers,FinancialReports,FinancialAnalysis,MonthClose,ExportCenter}.tsx`

**Interfaces:** `useBusinessMonth(): { month: Dayjs; monthStr: string; setMonth(month: Dayjs): void }`。

- [x] 先写最小 Context 测试或可验证的 hook 使用样例。（项目无前端测试框架，采用 hook 接入 + 类型/lint/build + 静态审计验证）
- [x] Context 初始化读取 `salary-desktop.business-month` localStorage，非法值回退当前月。
- [x] 顶部 DatePicker 改写 Context，不再维护 `globalMonth` 局部状态。
- [x] 逐页移除重复的当月默认 state；特殊年度、区间选择不迁移。
- [x] 页面局部选择月份时同步全局 Context。
- [x] 验证跨页面切换月份保持一致，PunchCard/OCR 不再固定在首次渲染月份。（代码路径与构建验证完成，桌面 GUI 交互纳入 Windows 验收）

**Acceptance:** 顶部月份和全部月度业务页面一致；刷新后保留最近月份。

### Task 2: 7A DDL、模型与安全迁移框架

**Files:**
- Modify: `src-tauri/src/db.rs`
- Modify: `src-tauri/src/models.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/db.rs`

**Tables:** `fund_accounts`、`business_partners`、`operator_profiles`、`approval_events`、`business_attachments`；`voucher_lines.fund_account_id`、`payment_batches.fund_account_id`、`bank_transactions.fund_account_id`。

- [ ] 写旧库升级失败测试：重复默认账户、坏外键、部分建表均应回滚。
- [ ] 增加 DDL、索引、CHECK/UNIQUE 约束和 `ensure_column` 兼容迁移。
- [ ] 为每张表补 Rust model/input/query 类型。
- [ ] 增加 `migration_reports` 或 app_settings 迁移状态键，记录待归集数量。
- [ ] 迁移结束运行 `PRAGMA foreign_key_check` 并断言无错误。
- [ ] 保证空库初始化、v0.6.1 旧库升级重复执行幂等。

**Acceptance:** 新旧库均能初始化；迁移异常不留下半成品。

### Task 3: 出纳领域模块骨架与基础资料 CRUD

**Files:**
- Add: `src-tauri/src/cashier.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/models.rs`
- Test: `src-tauri/src/cashier.rs`

**Commands:**
- `get/save/set_active_fund_account`
- `get/save/set_active_business_partner`
- `get/save/set_active_operator_profile`
- `set_current_operator` / `get_current_operator`

- [ ] 基础资料保存校验编码、类型、会计科目、账号重复和引用保护。
- [ ] 同类型默认资金账户切换在事务中完成。
- [ ] 创建 Tauri `CurrentOperatorState`，锁屏不清历史署名，注销/停用当前操作人时要求重新选择。
- [ ] 提供统一 `require_current_operator` helper；业务命令日志使用真实姓名，安全命令仍用 security。
- [ ] 加 CRUD、停用、默认切换和当前操作人失效测试。

**Acceptance:** 可稳定维护资金账户、往来单位、本地操作人；业务日志不再固定为 system。

### Task 4: 基础资料前端与当前操作人

**Files:**
- Add: `src/pages/FundAccounts.tsx`
- Add: `src/contexts/OperatorContext.tsx`
- Modify: `src/types/index.ts`
- Modify: `src/api/index.ts`
- Modify: `src/App.tsx`
- Modify: `src/pages/OperationLogs.tsx`

- [ ] API 类型、invoke 封装和 mock case 先行。
- [ ] 资金账户页使用 Tabs 管理账户、往来单位、操作人。
- [ ] 账号和金额使用 SensitiveText/SensitiveStatistic。
- [ ] Header 显示当前操作人并支持切换；无有效操作人时只允许进入安全/基础资料页。
- [ ] 显示“本地署名，不是多用户权限”的明确提示。
- [ ] 操作日志增加 operator 筛选和新命令中文映射。

**Acceptance:** 浏览器 mock 可打开；桌面端可完成基础资料和操作人切换。

### Task 5: 通用加密附件底座

**Files:**
- Modify: `src-tauri/src/security.rs`
- Modify: `src-tauri/src/security_commands.rs`
- Modify: `src-tauri/src/data_safety.rs`
- Modify: `src-tauri/src/cashier.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src/types/index.ts`
- Modify: `src/api/index.ts`

**Commands:** `add_business_attachment`、`list_business_attachments`、`delete_business_attachment`、`get_decrypted_attachment_url`。

- [ ] 抽取发票资源加密可复用 helper，不改变既有发票密文格式。
- [ ] 附件先加密落临时文件，再原子 rename；DB 与文件失败执行补偿清理。
- [ ] 删除仅允许未提交实体；已提交附件只允许通过反审批后变更。
- [ ] 备份/恢复/体检覆盖 attachments 目录和 manifest。
- [ ] 测试加密往返、篡改失败、孤儿文件体检和备份恢复。

**Acceptance:** 通用业务附件加密、预览、备份恢复可用，发票功能无回归。

## 7B：业务单据与付款

### Task 6: 资金单、状态机和审批事件

**Files:**
- Modify: `src-tauri/src/db.rs`
- Modify: `src-tauri/src/models.rs`
- Modify: `src-tauri/src/cashier.rs`
- Test: `src-tauri/src/cashier.rs`

**Tables:** `fund_documents`。

**Functions:** create/update/query/detail、submit、approve、reject、withdraw、void、mark_batched、settle、reverse。

- [ ] 写各 document_type 必填字段和金额/账户方向测试。
- [ ] 写完整允许/禁止状态转移矩阵测试。
- [ ] 状态更新与 approval_events 同事务；事件表不提供更新删除函数。
- [ ] `maker_checker_enabled=true` 时审批人与提交人不得相同。
- [ ] settled 后禁止 void/edit，reverse 创建新单据且引用原单。
- [ ] 月结保护覆盖原月份和冲正月份。

**Acceptance:** 任一状态不可由任意字段更新绕过，详情可重放完整历史。

### Task 7: 收付款单命令、API 与页面

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/types/index.ts`
- Modify: `src/api/index.ts`
- Add: `src/pages/FundDocuments.tsx`
- Modify: `src/App.tsx`
- Modify: `src/pages/OperationLogs.tsx`

- [ ] 注册 CRUD 和状态命令，命令层统一获取当前操作人、写日志。
- [ ] 页面按收款/付款/转账 Tab，支持月份、状态、往来单位、账户筛选。
- [ ] 表单按类型动态约束 source/target account、往来对象和对方科目。
- [ ] 详情 Drawer 展示附件、凭证链接和审批时间线。
- [ ] 状态按钮完全由后端返回状态决定，不在编辑表单出现状态字段。
- [ ] maker-checker 冲突显示可操作的中文提示。

**Acceptance:** 单据从草稿到审批可完整操作，历史与日志一致。

### Task 8: 资金单自动凭证与事务冲正

**Files:**
- Modify: `src-tauri/src/db.rs`（安全重建 vouchers CHECK）
- Modify: `src-tauri/src/accounting.rs`
- Modify: `src-tauri/src/cashier.rs`
- Modify: `src-tauri/src/models.rs`
- Test: `src-tauri/src/accounting.rs`、`cashier.rs`

- [ ] 先写 v0.6.1 vouchers 表重建测试，断言 ID、分录、索引和旧查询不变。
- [ ] 增加 `fund_document` source type，禁止绕过 insert_voucher 平衡校验。
- [ ] 实现 receipt/payment/transfer/advance/settlement 的凭证生成规则。
- [ ] 每条资金分录写 `fund_account_id`，对方分录必须为空。
- [ ] 结算状态、凭证、日志同事务提交；任何失败全部回滚。
- [ ] 冲正生成反向凭证，原凭证保留 active 并建立追溯关系。

**Acceptance:** 五类单据凭证借贷平衡、辅助账户正确、重试不重复。

### Task 9: 通用付款批次

**Files:**
- Modify: `src-tauri/src/db.rs`
- Modify: `src-tauri/src/accounting.rs`
- Modify: `src-tauri/src/excel.rs`
- Modify: `src-tauri/src/models.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src/types/index.ts`
- Modify: `src/api/index.ts`
- Modify: `src/pages/Payments.tsx`

- [ ] payment batch 创建必须选择资金账户；旧批次允许 NULL 只读。
- [ ] `general` 批次仅纳入 approved payment/advance，批次 item 保存往来方银行信息快照。
- [ ] 标记付款时将资金单 settled、生成凭证和审批事件；全部同事务。
- [ ] 作废未付款批次时资金单恢复 approved；已付款批次禁止作废。
- [ ] Excel 增加通用付款格式和批次类型中文名称。
- [ ] 现有 salary/reimbursement 批次补 fund_account_id 并写凭证辅助账户。

**Acceptance:** 三种付款批次行为一致，旧批次不丢失且新批次具备账户维度。

## 7C：日记账与银行对账

### Task 10: 历史资金账户归集向导

**Files:**
- Modify: `src-tauri/src/cashier.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src/types/index.ts`
- Modify: `src/api/index.ts`
- Modify: `src/pages/FundAccounts.tsx`

**Commands:** `get_fund_migration_status`、`preview_fund_assignment`、`apply_fund_assignment`。

- [ ] 汇总旧 bank_transactions、payment_batches、资金科目 voucher_lines 的待归集数量。
- [ ] 单账户唯一映射可预览但仍需用户确认后写入。
- [ ] 多账户逐批/逐流水分配；禁止后台按金额或账号猜测。
- [ ] 写入前再次校验对象未被月结后修改；操作全量审计。
- [ ] 完成状态写 app_settings，保留可重复打开的迁移报告。

**Acceptance:** 历史数据归集过程透明、可审计、无静默错误。

### Task 11: 流水账户化与导入预览

**Files:**
- Modify: `src-tauri/src/db.rs`
- Modify: `src-tauri/src/excel.rs`
- Modify: `src-tauri/src/models.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src/types/index.ts`
- Modify: `src/api/index.ts`
- Modify: `src/pages/BankTransactions.tsx`

- [ ] 导入前选择 bank/third_party 账户；现金账户不可导入银行流水。
- [ ] 流水唯一索引加入 fund_account_id，迁移时安全重建索引。
- [ ] 增加字段识别预览、收入支出方向和余额列校验；确认后才入库。
- [ ] 待归集旧流水可查询但不能自动匹配。
- [ ] `bank_manual` 生成凭证时资金行写入流水账户 ID。
- [ ] 覆盖不同账户相同流水、重复导入、月结锁定测试。

**Acceptance:** 每条新流水都有明确账户，导入错误在落库前可见。

### Task 12: 多对多银行核销引擎与旧匹配迁移

**Files:**
- Modify: `src-tauri/src/db.rs`
- Modify: `src-tauri/src/models.rs`
- Modify: `src-tauri/src/cashier.rs`
- Modify: `src-tauri/src/commands.rs`
- Test: `src-tauri/src/cashier.rs`、`db.rs`

**Table:** `bank_reconciliation_allocations`。

**Commands:** candidate preview、confirm allocations、cancel allocation、auto-match preview、batch confirm。

- [ ] 写一对一、一对多、多对一、部分核销、超额、反方向、跨账户测试。
- [ ] 查询账面候选仅返回 active、同账户、方向相符且有未核销余额的资金分录。
- [ ] 自动匹配只返回候选和 score；批量确认只处理高置信且无冲突项目。
- [ ] allocation 写入/取消后实时计算两侧状态，不依赖可漂移的冗余 matched 布尔值。
- [ ] 迁移旧 bank_transaction_matches；无法定位的记录进入报告并保留旧表。
- [ ] 月结后禁止修改该月 allocation；跨月差异按银行流水月份控制。

**Acceptance:** 多对多核销金额守恒，旧匹配可追踪，取消不破坏原数据。

### Task 13: 资金日记账、对账页面与余额调节表

**Files:**
- Modify: `src-tauri/src/db.rs`
- Modify: `src-tauri/src/models.rs`
- Modify: `src-tauri/src/cashier.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/excel.rs`
- Modify: `src/types/index.ts`
- Modify: `src/api/index.ts`
- Add: `src/pages/FundJournals.tsx`
- Modify: `src/pages/BankTransactions.tsx`

**Table:** `bank_reconciliation_periods`。

- [ ] 实现按账户/区间的账面日记账和稳定滚动余额。
- [ ] 对账页展示流水与账面分录双栏、剩余金额、候选评分和 allocation 明细。
- [ ] 余额调节表计算期初衔接、账面期末、对账单期末、未达项、调节后差额。
- [ ] 差额超过 0.005 或有待归集流水时禁止确认。
- [ ] 导出现金/银行日记账和余额调节表，敏感导出要求 reveal。
- [ ] 前端增加不平衡红色提示和跳转到未核销项目。

**Acceptance:** 账面余额可复算、调节表可解释差额、导出与页面一致。

## 7D：借款、报销治理与收尾

### Task 14: 员工借款、备用金与核销

**Files:**
- Modify: `src-tauri/src/db.rs`
- Modify: `src-tauri/src/models.rs`
- Modify: `src-tauri/src/cashier.rs`
- Modify: `src-tauri/src/accounting.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/excel.rs`
- Modify: `src/types/index.ts`
- Modify: `src/api/index.ts`
- Add: `src/pages/Advances.tsx`

**Table:** `advance_settlement_links`。

- [ ] advance 必须绑定员工、预计归还日和其他应收款科目。
- [ ] 支持报销抵扣、现金/银行归还、工资扣回、其他核销四类来源。
- [ ] 多次核销累计不超过借款，取消核销恢复余额并联动作废/冲正凭证。
- [ ] 输出未核销余额、逾期天数、0-30/31-60/61-90/90+ 账龄。
- [ ] 页面支持员工筛选、核销时间线、来源跳转和 Excel 导出。

**Acceptance:** 借款发放、部分核销、完全核销和逾期统计可闭环。

### Task 15: 报销审批治理改造

**Files:**
- Modify: `src-tauri/src/db.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/accounting.rs`
- Modify: `src/types/index.ts`
- Modify: `src/api/index.ts`
- Modify: `src/pages/Reimbursements.tsx`

- [ ] ReimbursementClaimInput 移除可直接写 status/payment_status/payment_date 的普通保存路径。
- [ ] 保存只处理草稿业务字段；submit/approve/reject/withdraw 使用专用命令和 approval_events。
- [ ] 删除直接付款按钮；付款状态只允许付款批次事务更新。
- [ ] 已审批附件/发票变更必须反审批，填写原因并联动作废计提凭证。
- [ ] maker-checker 开启时阻止自提交自审批。
- [ ] 写兼容旧状态数据和完整凭证联动测试。

**Acceptance:** 报销状态不可从表单任意改写，审批和付款职责路径清晰。

### Task 16: 月结、仪表盘、预算与安全联动

**Files:**
- Modify: `src-tauri/src/db.rs`
- Modify: `src-tauri/src/cashier.rs`
- Modify: `src-tauri/src/models.rs`
- Modify: `src/pages/MonthClose.tsx`
- Modify: `src/pages/Dashboard.tsx`
- Modify: `src/pages/FinancialAnalysis.tsx`
- Modify: `src-tauri/src/data_safety.rs`
- Modify: `src/pages/DataSafety.tsx`

- [ ] 月结汇总和检查增加资金单、辅助凭证、待归集、余额调节、部分核销、逾期借款。
- [ ] 严格对账按账户配置；历史归集未完成时只 warning，不突然阻断旧用户。
- [ ] 仪表盘增加待审批、待付款、待归集、未核销、借款逾期入口。
- [ ] 预算实际发生统一纳入已审批资金单或有效凭证，明确避免与发票/报销重复计费。
- [ ] 数据安全状态增加附件、资金表、迁移状态和孤儿文件统计。
- [ ] 月结包增加日记账、余额调节表、通用付款和借款余额。

**Acceptance:** 出纳异常能在工作台发现，正式月结不会遗漏关键资金事项。

### Task 17: 导航整理、可访问性、全量回归与文档

**Files:**
- Modify: `src/App.tsx`、相关 CSS
- Modify: `src/pages/OperationLogs.tsx`
- Modify: `src/api/index.ts` mock
- Modify: `docs/user-guide.html`
- Modify: `CLAUDE.md`
- Modify: `.claude/memory/stage7-cashier-operations.md`
- Modify: `docs/superpowers/plans/2026-08-30-stage7-progress.md`

- [ ] 新建“资金出纳”菜单：资金账户、收付款单、付款批次、银行对账、资金日记账、借款备用金。
- [ ] 薪酬菜单移除付款批次/银行流水，保留工资与社保。
- [ ] 补齐所有 mock case、日志中文映射、空状态、键盘操作和错误提示。
- [ ] 运行全量前后端回归及旧库迁移测试。
- [ ] Windows exe 手工验收完整主流程和异常/冲正路径。
- [ ] 更新用户手册、CLAUDE、memory、progress；执行 `graphify update .`。

**Acceptance:** 自动测试全绿，Windows 验收清单无阻断问题，文档和知识图谱与代码一致。

## Release Checkpoints

| Checkpoint | 可发布内容 | 前置 |
|---|---|---|
| 7A | 全局月份、资金账户/往来单位/操作人、附件底座 | Gate 0 |
| 7B | 通用收付款、审批历史、通用付款批次、自动凭证/冲正 | 7A |
| 7C | 账户化流水、多对多核销、日记账、余额调节 | 7B + 历史归集向导 |
| 7D | 借款核销、报销治理、月结分析与正式收尾 | 7C |

任何 checkpoint 均不得以“功能已写完”替代旧库升级和 Windows exe 验收。
