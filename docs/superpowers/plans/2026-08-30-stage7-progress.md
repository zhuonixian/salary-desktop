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
| Gate 0 | 进行中 | 自动化基线通过；Windows 验收、真实备份恢复和旧库样本待完成 |
| 7A | 进行中 | Task 1 全局月份完成；基础资料、操作人、附件待开始 |
| 7B | 待开始 | 通用收付款、审批、付款批次、凭证冲正 |
| 7C | 待开始 | 历史归集、流水账户化、多对多对账、日记账 |
| 7D | 待开始 | 借款核销、报销治理、月结与收尾 |

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
