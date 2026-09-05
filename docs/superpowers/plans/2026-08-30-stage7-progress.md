# 第七阶段进度同步

本文件用于第七阶段出纳运营闭环开发接力。每轮开发结束必须追加记录，避免上下文压缩、分工或新会话接手造成信息丢失。

## 当前基线

- 分支：`master`
- 当前版本：`v0.6.1`（HEAD `219e41d`，规划时记录）
- 阶段计划：`docs/superpowers/plans/2026-08-30-stage7-cashier-operations.md`
- 设计说明：`docs/superpowers/specs/2026-08-30-stage7-cashier-operations-design.md`
- 长期摘要：`.claude/memory/stage7-cashier-operations.md`
- 自动测试基线：第六阶段收尾记录 Rust 141 个测试通过；开始实现前须重新运行确认。
- 工作区注意：规划时存在用户未跟踪文件 `docs/user-guide-v2.html`，不得覆盖、删除或纳入无关提交。

## 目标交付

资金账户与往来单位、全局月份、本地操作人署名、通用收付款及审批历史、通用付款批次、账户级自动凭证、银行流水多对多核销、资金日记账、余额调节表、员工借款核销、报销治理和月结联动。

## 当前状态

- 2026-08-30：设计与实施计划已建立。
- 2026-08-30 Gate 0 本机基线：`npx tsc --noEmit`、`npm run lint`、`npm run build`、`cargo fmt --check`、`cargo check`、`cargo test --lib` 全部通过；Rust 141 passed，保留既有 5 个 dead_code/unused warning，Vite 保留既有 chunk 体积提示。
- Gate 0 未完成：Windows exe 手工验收与真实加密备份恢复演练需要 Windows GUI 环境；旧库迁移样本将在 Task 2 新迁移落地前固化，当前尚无第七阶段 schema 可验证。
- 2026-08-30 Task 1：全局业务月份 Context 已实现并接入 15 个主要月度页面；自动化验证通过，桌面 GUI 联动待 Windows 验收。
- 下一步：Task 2 资金基础 DDL、模型与迁移框架；7A 发布仍受 Gate 0 Windows 验收约束。

## 批次状态

| 批次 | 状态 | 说明 |
|---|---|---|
| Gate 0 | 自动化完成 | 自动化基线通过；Windows 验收与真实备份恢复演练待 Windows GUI 环境 |
| 7A | 完成 | 全局月份 + 基础资料 + 操作人 + 加密附件（Task 1-5） |
| 7B | 完成 | 通用收付款、审批、付款批次、凭证冲正（Task 6-9） |
| 7C | 完成 | 历史归集、流水账户化、多对多对账、日记账、调节表（Task 10-13） |
| 7D | 完成 | 借款核销、报销治理、月结联动、收尾回归与文档（Task 14-17） |

## 协作规则

- 开发前读 `CLAUDE.md`、本文件、stage7 spec 与 plan。
- 多模块任务使用 subagent 时按互不重叠文件划分；主 agent 负责事务边界、类型契约、集成测试、commit 和 push。
- 每个 Task 独立提交并在本文件追加测试、风险、未完成项和 commit。
- 不把本地操作人署名描述成真正账号权限。
- 旧库迁移、状态机、资金金额守恒和月结保护是阻断级验收项。

## 本轮记录模板

```md
### YYYY-MM-DD HH:mm — Task N / 批次

- 目标：
- 完成：
- 关键决策/偏差：
- 修改文件：
- 测试：
- Windows 手工验收：
- 未完成/风险：
- 下轮入口：
- 提交：
```

### 2026-08-30 — Gate 0 自动化基线

- 目标：恢复第七阶段实施上下文，确认 v0.6.1 自动化基线和工作区安全。
- 完成：确认 HEAD `219e41d`；前端类型检查/lint/build 与 Rust fmt/check/141 tests 全部通过；未发现本项目遗留运行进程。
- 关键决策/偏差：当前环境不是 Windows GUI，不能冒充完成 Windows 手工验收；Task 1 仅改前端月份状态，不改变账务数据，允许先行，但 7A 发布门禁不放行。
- 修改文件：本进度文件。
- 测试：`npx tsc --noEmit`、`npm run lint`、`npm run build`、`cd src-tauri && cargo fmt --check && cargo check && cargo test --lib` 全部通过。
- Windows 手工验收：未执行；待 Windows 环境。
- 未完成/风险：真实加密备份恢复、旧库样本、stage6 Windows 待验项。
- 下轮入口：Task 1 BusinessMonthContext。
- 提交：未提交。

### 2026-08-30 — Task 1 / 7A 全局业务月份

- 目标：消除顶部月份与各业务页月份互不联动的问题，并持久化最近业务月。
- 完成：新增 `BusinessMonthContext`；Provider 接入 bootstrap；App 顶部 DatePicker 改用 Context；Dashboard、Attendance、OcrCenter、PunchCard、SalaryCalculate、Invoices、Reimbursements、Payments、BankTransactions、Vouchers、FinancialReports、FinancialAnalysis、MonthClose、ExportCenter 共 14 个页面接入（连同 App 顶部共 15 个使用点）。
- 关键决策/偏差：Invoices/Reimbursements 改为非空月度视图，不再通过清空月份查询全部历史；科目余额表区间、社保年度和个税年度保持独立。项目未配置前端单测框架，本 Task 使用类型检查、完整 lint/build 和残留状态静态审计替代新增测试框架。
- 修改文件：`src/contexts/BusinessMonthContext.tsx`、`src/bootstrap.tsx`、`src/App.tsx` 及上述 14 个页面。
- 测试：`npx tsc --noEmit`、`npm run lint`、`npm run build`、`git diff --check` 全部通过；Vite 仅既有 chunk 提示。
- Windows 手工验收：待验证顶部切月后跨页面一致、刷新保留、非法 localStorage 回退、OCR/打卡使用所选月份。
- 未完成/风险：localStorage 被禁用时只保持当前会话；Windows GUI 未验收。
- 下轮入口：Task 2 DDL、模型与安全迁移框架。
- 提交：未提交。

### 2026-09-05 — 7A 基础底座（Task 2-5）完成

- 目标：资金领域 DDL/迁移、cashier 骨架与基础资料 CRUD、基础资料前端与操作人、通用加密附件。
- 完成：Task 2 五表+三列迁移框架（147 测试）；Task 3 cashier.rs CRUD+11 命令+操作人会话（158，含 fix：停用操作人署名时序）；Task 4 基础资料页/OperatorContext/导航拦截（前端三验+浏览器实测）；Task 5 加密附件底座+发票原语复用重构（167 测试）。每任务经独立实现者+审查者双裁决。
- 关键决策：DDL 入迁移事务满足 spec 9.1；默认账户停用拦截；基础资料尽力署名；附件归档 attachments/{entity_type}/{belong_month}/。
- 修改文件：db.rs、models.rs、cashier.rs（新）、commands.rs、lib.rs、security.rs、invoice.rs、data_safety.rs、legacy_migration.rs；前端 FundAccounts.tsx（新）、OperatorContext.tsx（新）、types/api/App/OperationLogs/bootstrap。
- 测试：cargo 167 passed；tsc/lint/build 全过。
- Windows 手工验收：未执行（批次发布门禁继续挂账）。
- 未完成/风险：FK 门禁全库启动阻断待复议；operator 筛选前端过滤；附件 add 实体门禁待 Task 6；Task 0 Windows 验收。
- 下轮入口：Task 6 资金单、状态机和审批事件。
- 提交：8cf0039..c55ef83（7cb48c8/32a23d0/30d0823/2b05f1d/c55ef83）。

### 2026-09-05 — 7B Task 7：收付款单命令、API 与页面完成

- 目标：暴露 Task 6 资金单领域层为 Tauri 命令（含审批事件查询与 maker_checker 设置），新增收付款单前端页面。
- 完成：cashier.rs/models.rs 全部 TODO(Task 7) dead_code 标记销项（mark_document_batched 改挂 TODO(Task 9)，批次场景内用）；commands.rs 新增 14 命令（get_fund_documents/get_fund_document_detail/list_approval_events/get·set_maker_checker_enabled/create·update·submit·approve·reject·withdraw·void·settle·reverse_fund_document），get 类不记日志、写命令 log_operation 当前操作人署名；前端 FundDocuments.tsx（收款/付款/内部转账/全部单据 Tab + 月份(全局)/状态/往来对象/账户/关键字筛选 + 新建编辑弹窗按类型动态约束账户方向与往来对象（表单无状态字段）+ 行操作按钮完全由后端状态驱动 + 详情 Drawer（Descriptions + 审批时间线 + 附件上传/列表/删除/预览）+ 冲正弹窗原因必填 + 工具栏"审批设置" maker_checker 开关）；App.tsx 资金出纳组新增"收付款单"路由菜单；OperationLogs 补 10 个写命令中文映射（get 类不映射避免死键）。
- 关键决策：FundDocumentQuery 新增 account_id 筛选（brief 要求账户筛选而后端无此参数，SQL 拼接 source/target OR 命中，同时给非法类型/状态入参加 ensure_in_list 前置校验）；mock 内存态轻量状态机演示完整草稿→提交→审批→结算→冲正；浏览器 headless 实测通过（解锁→四 Tab→审批流→maker_checker 自审批拦截中文提示→冲正单生成+时间线+原因，无 JS 报错）。
- 修改文件：cashier.rs、models.rs、commands.rs、lib.rs；types/index.ts、api/index.ts（含 mock）、pages/FundDocuments.tsx（新）、App.tsx、pages/OperationLogs.tsx。
- 测试：cargo 180 passed（基线 179 + 新增 1：account_id 筛选与枚举前置校验）；cargo check 警告维持基线 5 个；tsc/lint/build 全过。
- 未完成/风险：付款类单据审批后仅显示"待付款批次"提示（批次动作归 Task 9）；结算/冲正凭证联动为占位文案（Task 8 落地后补跳转）；附件预览在浏览器 mock 返回空路径走"暂不支持"分支（桌面端走 convertFileSrc）；冲正弹窗月份预填依赖 antd setFieldsValue 先于挂载（实测有效，与 create 弹窗同模式）。
- 下轮入口：Task 8 资金单自动凭证与事务冲正（必须承接：冲正/结算凭证生成移入事务内）。
- 提交：a797ef7（feat）+ 9a5235d（test）。

### 2026-09-05 — 7B 业务单据（Task 6-9）完成

- 目标：资金单状态机、审批事件、收付款页面、自动凭证与事务冲正、通用付款批次。
- 完成：Task 6 fund_documents+全状态机+挂账三条承接（179）；Task 7 14 命令+四 Tab 页+审批时间线（180）；Task 8 结算凭证+红字冲正+vouchers 表重建（187，冲正口径裁定=红字冲销净影响归零）；Task 9 general 批次逐单结算+存量凭证资金账户（195，含 fix：错误透出/门禁前移/负向测试）。每任务双裁决+opus 关键口径裁定。
- 关键决策：冲正严口径（已月结月份纠错须先反月结）；general 批次无批次级凭证（防双重贷记）；已付款禁作废仅 general。
- 测试：cargo 195 passed；tsc/lint/build 全过；浏览器 mock 实测收付款全流程。
- Windows 手工验收：未执行。
- 未完成/风险：advance_settlement 核销分流待 Task 14；operator 筛选后端参数未做；Windows 验收挂账。
- 下轮入口：Task 10 历史资金账户归集向导。
- 提交：6210a7c..3911b19（a797ef7/9a5235d/96018bb/6210a7c 前序、f1478d8/d13bfb5/3911b19）。

### 2026-09-05 — 7C Task 10：历史资金账户归集向导完成

- 目标：历史银行流水/旧付款批次资金分录归集到指定账户（spec 9），承接 Task 2 挂账"待归集计数未排除 void 凭证"。
- 完成：cashier.rs 归集领域函数三件（get_fund_migration_status 实时统计+按月分组+待归集批次清单+独立分录数、preview_fund_assignment 按账户/范围只读预览、apply_fund_assignment 单事务写入+联动+计数刷新）；commands.rs/lib.rs 三命令（写命令记 operation_logs 全量审计：对象类型/范围/目标账户/成功/联动/跳过条数）；db.rs build_stage7_report 口径修正（void 凭证分录 + void 批次排除，build/record 提为 pub(crate)）；models.rs 五结构体；前端 FundAccounts.tsx"历史归集"向导 Modal（统计 Alert → 银行流水/付款批次 Segmented → 归属月/目标账户选择（单账户唯一候选自动预填仍需确认）→ 自动预览 → Popconfirm 执行 → 成功 N/跳过 M 反馈）；OperationLogs 补 apply_fund_assignment 映射。
- 关键决策（spec 依据）：归集维度=流水（可按归属月圈范围）+批次（可单批次），均联动 active 凭证资金分录（bank_manual 源=流水 id；salary/reimbursement_payment 源=批次 id），void 凭证分录不动；科目≠账户挂接科目的分录跳过保持 NULL（spec 9.5 不猜测、不改账）；月结保护逐月 ensure_month_open 前置，任一命中整体回滚；幂等=UPDATE 带 fund_account_id IS NULL（流水范围重复执行零写入），指定批次重复归集明确报错"已归集或不存在"；停用账户不可作归集目标（与全应用口径一致）；归集后事务内刷新 stage7_migration_* 计数并写 stage7_fund_assignment_last_applied_at。
- 修改文件：cashier.rs、db.rs、models.rs、commands.rs、lib.rs；types/index.ts、api/index.ts（含 mock）、pages/FundAccounts.tsx、pages/OperationLogs.tsx。
- 测试：cargo 202 passed（基线 195 + 新增 7：void 口径、状态分组、流水归集联动 bank_manual/void 不动/幂等、批次归集联动+重复拦截、科目不一致跳过、月结拦截回滚、账户与类型校验）；tsc/lint/build 全过；浏览器 headless 实测向导（统计/两类别/唯一账户预填/空态预览/批次 Tab 表格，无新增 JS 报错）。
- 未完成/风险：独立凭证分录（manual 类等无来源联动）保持 NULL 属 spec 9.5 允许终态，月结 warning→blocking 升级归 Task 13；旧批次作废后不再计入待归集（口径与 void 凭证一致）；Windows exe 手工验收待做。
- 下轮入口：Task 11 流水账户化与导入预览。
- 提交：本条对应 commit 见 task-10-report.md。

### 2026-09-05 — 7C 资金对账（Task 10-13）完成

- 目标：历史归集、流水账户化、多对多核销、日记账与余额调节表。
- 完成：Task 10 归集向导+口径修正（202）；Task 11 流水账户化三步导入（207）；Task 12 allocation 引擎+六因子评分+旧匹配迁移（219，fix：迁移额双侧 min）；Task 13 日记账/对账工作台/调节表/旧匹配退役（226，fix：调节表月结保护+导出门禁）。每任务双裁决，金额守恒/勾稽数学经 opus 推演验证。
- 关键决策：评分归 Task 12（输入=资金分录）；日期窗口=自动匹配硬条件；旧三命令最小退役（confirm 拦截+前端移除入口）；blocking 判别=归集向导执行信号。
- 测试：cargo 226 passed；tsc/lint/build 全过；浏览器 mock 实测。
- Windows 手工验收：未执行（旧库升级→归集→核销→日记账→调节表全流程为 7D 阻断项）。
- 下轮入口：Task 14 员工借款、备用金与核销。
- 提交：3911b19..4a72815（a5349b1/b5363bc/0a8c63c/4606aff/53b5e7e/4a72815）。

### 2026-09-05 — Task 14：员工借款、备用金与核销

- 目标：advance_settlement_links 核销关系、核销方式分流（现金归还/报销抵扣/工资扣回/其他）、不得重复核销、借款台账与导出。
- 完成：DDL 补建 advance_settlement_links（迁移事务内+新表自检）+ fund_documents ensure_column settlement_mode/due_date；凭证方式分流（现金归还=借资金/贷1221，报销抵扣=借2241，工资扣回=借2211，其他=借指定科目；贷方统一取关联借款对方科目缺省 1221）；创建/更新校验（必须关联已发放借款、员工一致、分摊合计=单金额、累计核销≤借款额容差 0.005、方式与账户匹配）；取消核销（未结算→作废联动取消 links，已结算→冲正联动取消 links 恢复余额）；核销单作废/冲正钩子联动取消 links；借款单有 active 核销禁冲正；台账聚合（未清余额/未清天数/逾期天数/0-30/31-60/61-90/90+ 账龄/汇总）+ Excel 导出；4 命令暴露；前端 Advances.tsx（台账/时间线/新增借款/新增核销/取消核销/导出）+ 菜单路由 + 日志映射。
- 测试：cargo 234 passed（基线 226 + 新增 8：links schema、创建门禁、四方式分录+累计上限、台账账龄/筛选/时间线/导出、取消双路径、冲正阻断、跨月月结保护、草稿更新替换 links）；tsc/lint/build 全过。
- 关键决策：核销方式存 fund_documents.settlement_mode（ensure_column），links 只存关系与金额；借款须 settled（已发放）才可核销；缺省方式 cash_return 兼容历史资金回流建模；跨月核销允许、月结保护走既有 ensure_month_open。
- 未完成/风险：Windows exe 手工验收（借款发放→部分核销→台账核对）待做；历史未结算 advance_settlement 单据（无 links）按 1221 贷方结算（旧口径等价）；mock 预览不支持冲正取消核销（提示桌面端操作）。
- 下轮入口：Task 15 报销审批治理改造。

### 2026-09-05 — Task 15：报销审批治理改造

- 目标：spec 5.2 报销单治理——状态机命令化、审批事件统一、maker_checker、反审批联动。
- 完成：报销状态机命令化（submit/approve/reject/withdraw/unapprove 专用命令），直写状态通道物理删除；approval_events 统一 append 留痕；maker_checker 经办复核开关接入；unapprove 联动既有凭证/批次（已核验零回归）；前端 Reimbursements.tsx 改造 + 日志映射。
- 测试：cargo 241 passed；tsc/lint/build 全过。
- Minor 挂账：save 报错内嵌英文状态码；mock 状态机命令 default 返回 true；void 不查 payment_status（legacy 可达面极窄）。

### 2026-09-05 — Task 16：月结/仪表盘/预算/安全联动（spec 8 收口）

- 目标：spec 8 联动——月结新检查、严格对账账户级开关、仪表盘资金卡、预算去重、数据安全统计、operator 后端筛选。
- 完成：月结 5 项新检查（待审批资金单/已审批未付款未结算/已结算无辅助凭证均 blocking，借款逾期 warning）+ 严格模式 `fund_accounts.strict_reconciliation` 账户级开关（默认关；部分核销涉严格账户升级 blocking；strict 限 bank 类账户防死锁）+ 月结包新增资金日记账/余额调节表/借款台账 + 仪表盘 7 字段资金卡片 + 预算口径去重（已批报销内发票不再双计，纳入已审批付款资金单）+ DataSafetyStatus 附件/资金表/迁移/孤儿统计 + operator 后端筛选（Task 4 挂账承接）+ FK 门禁报错列明细；前端 MonthClose/Dashboard/FinancialAnalysis/DataSafety/SecurityCenter 联动。
- 测试：cargo 251 passed（+10）；tsc/lint/build 全过；Fix Round 1：strict 限银行类账户防死锁。
- 重大发现：`npx tsc --noEmit` 在本仓库为空检查（根 tsconfig 仅 refs + files:[]），实际验证须 `tsc -b`——勘误归 Task 17。

### 2026-09-05 — Task 17：导航整理、可访问性、全量回归与文档（收尾）

- 目标：承接 Task 16 挂账（tsc 勘误、cash 严格开关 UX、月结检查 account_type 兜底）+ 导航/日志映射核对 + 文档四件套 + 使用手册第七章。
- 完成：
  - tsc 勘误：实测注入类型错误证明 `tsc --noEmit` 恒过（空检查）、`tsc -b` 报错（exit 2）；CLAUDE.md 核心命令、`.claude/memory/commands-reference.md`、stage7 memory 测试门槛全部改为 `npx tsc -b` 并注明原因。
  - 月结兜底：db.rs `strict_reconciliation_unconfirmed` SQL 加 `account_type IN ('bank','third_party')`（防旧备份恢复出 cash+strict 死锁态）；新增测试 `test_month_close_strict_check_ignores_cash_dirty_data`（直插脏数据 + 并存严格银行账户差分断言：脏数据不计、银行账户照常 blocking、确认后恢复 ok）。
  - 前端 UX：FundAccounts 表单 cash 类型隐藏严格对账开关（Form.useWatch 驱动），切类型时同步关闭字段值 + handleSave 再兜一道，与后端拦截对齐避免"提交被拒且无处关闭"死路。
  - 导航核对：资金出纳分组（资金账户/收付款单/付款批次/银行对账/资金日记账/借款备用金）与薪酬分组（工资计算/社保台账）与 spec 7 节一致，无重复入口，无需改动；路由与菜单一一对应。
  - 日志映射补齐：generate/confirm/export_bank_reconciliation_period、export_fund_journal、export_month_close_package 七阶段缺漏 + close_month/reopen_month/backup_database/restore_database/compact_database/verify_database 历史缺漏（与 commands.rs 实际写入的 operation_type 一一核对）。
  - mock 核对：109 个显式 case + default 分流（get_/query_ → []、export_/delete_/update_ → true）覆盖全部前端命令，无需新增。
  - 文档：CLAUDE.md 第七阶段段落改为已交付口径 + 架构摘要补 cashier.rs/25 pages；`.claude/memory/stage7-cashier-operations.md` 重写为已交付能力 + 已知边界（冲正严口径须先反月结、严格模式账户级、tsc -b）+ Windows 验收清单；本文件追加 7D 完成记录；`docs/user-guide.html` 补第七章出纳功能卡片。
- 测试：cargo 252 passed（基线 251 + 新增 1）；`npx tsc -b`、`npm run lint`、`npm run build`、`cargo fmt --check`、`cargo check` 全过；graphify update 已跑。
- Windows 手工验收：未执行（无 Windows GUI 环境），验收清单已固化在 stage7 memory（旧库升级→建账户→归集→日记账；general 批次全流程+冲正；借款三方式核销；对账工作台/调节表；月结 12 项+月结包；锁屏脱敏回归）。
- 未完成/风险：Minor 挂账清单 triage 后留 stage7 memory（FundDocuments 编辑抹 settlement_mode、bank_manual 账户下拉收窄、expense_type 文本匹配、attachment_disk_stats O(n²) 等，均不阻断）。
- 下轮入口：Windows exe 手工验收 → 发版评估。

## 第七阶段完成

Task 0-17 全部完成（Task 1 于 8cf0039 前完成未走 SDD；Task 2-16 走 SDD 双轮 review；Task 17 收尾）。后端 252 测试全过；前端 `tsc -b`/lint/build 全过；cargo fmt/check 全过。文档四件套（CLAUDE.md、memory、progress、user-guide）与 graphify 已同步。Windows exe 手工验收为遗留阻断项（清单见 memory）。
