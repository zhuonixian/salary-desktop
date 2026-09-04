---
name: stage7-cashier-operations
description: 第七阶段出纳运营闭环（资金账户、通用收付款、多对多对账、审批留痕、借款核销）计划与开发接力摘要
---

# 第七阶段：出纳运营闭环

设计：`docs/superpowers/specs/2026-08-30-stage7-cashier-operations-design.md`

实施计划：`docs/superpowers/plans/2026-08-30-stage7-cashier-operations.md`

进度：`docs/superpowers/plans/2026-08-30-stage7-progress.md`

## 定位

保持单公司、本地单机、出纳主导，不扩展为完整 ERP。主线是把资金账户、收付款、付款批次、银行流水、凭证辅助核算、日记账、对账和月结串成闭环，并补足流程留痕与员工借款核销。

## 核心决策

1. 账面日记账从 `voucher_lines` 派生，分录新增 `fund_account_id`；不维护第二套可编辑账。
2. 银行对账改为银行流水 ↔ 账面资金分录 allocation，多对多且金额守恒。
3. 通用 `fund_documents` 覆盖 receipt/payment/transfer/advance/advance_settlement/reversal。
4. 状态通过专用命令流转，approval_events 追加留痕；已结算只能冲正。
5. 本地 operator_profiles 只用于署名和流程约束，不是账号/RBAC；安全事件继续记 security。
6. 付款批次保留工资/报销并增加 general，所有新批次必须选择资金账户。
7. 历史数据无法唯一判断账户时不猜测，进入待归集向导。
8. 顶部月份由 BusinessMonthContext 贯通全部月度页面。

## 实施批次

- Gate 0：第六阶段 Windows 验收、备份恢复、旧库迁移样本。
- 7A：全局月份、资金账户、往来单位、操作人、加密附件。
- 7B：资金单状态机、审批历史、通用付款批次、自动凭证和冲正。
- 7C：历史归集、流水账户化、多对多核销、日记账、余额调节表。
- 7D：借款备用金、报销治理、月结/仪表盘/安全联动和全量回归。

## 当前进度

- Gate 0 自动化基线通过：前端检查全绿，Rust 141 tests 通过；Windows 手工验收和真实备份恢复待完成。
- 7A Task 1 已完成：BusinessMonthContext 已接入顶部和主要月度业务页面，最近月份保存到 localStorage。
- 下一入口：Task 2 资金基础 DDL、模型与迁移框架。

## 新增模块与页面

- 后端：`src-tauri/src/cashier.rs`
- 前端 Context：`BusinessMonthContext.tsx`、`OperatorContext.tsx`
- 页面：`FundAccounts.tsx`、`FundDocuments.tsx`、`FundJournals.tsx`、`Advances.tsx`
- 现有重点改造：Payments、BankTransactions、Reimbursements、MonthClose、Dashboard、accounting/db/excel/commands。

## 阻断级约束

- 状态和审批事件必须同事务，不允许表单直接写 status/payment_status。
- 资金凭证分录必须标资金账户；对账 allocation 不得跨账户、反方向或超额。
- 已结算单据不直接作废；冲正保留原凭证。
- 所有资金写操作遵守正式月结锁定。
- vouchers CHECK 扩展必须安全重建并保留 ID/索引/外键。
- 旧匹配迁移失败要生成报告，不能静默丢失。

## 范围外

多用户账号、强权限、在线审批、云同步、银企直联、外币、多账套、采购库存销售、税务申报和完整行政 OA。

## 测试门槛

每 Task 针对性测试；每批收尾运行：

```bash
npx tsc --noEmit
npm run lint
npm run build
cd src-tauri && cargo fmt --check
cd src-tauri && cargo check
cd src-tauri && cargo test --lib
```

旧库迁移和 Windows exe 主流程手工验收不可省略。
