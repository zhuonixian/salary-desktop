# 第十二阶段进度：收入侧闭环——销项登记、应收台账与回款核销

spec：`docs/superpowers/specs/2026-10-13-stage12-revenue-cycle-design.md`
基线：v0.9.0 + 第十/十一阶段（后端 380 测试 → 本阶段 390）

## Task 状态

| # | 任务 | 状态 |
|---|---|---|
| 1 | db.rs：invoices.direction/partner_id（DDL+迁移+测试 schema）、ar_receipt_allocations 表、vouchers CHECK 加 sales_invoice（stage12 重建） | ✅ |
| 2 | 销项凭证：maybe_generate_invoice_expense_voucher 按 direction 分派 → sales_invoice（借1122/贷6001/贷2221）；void 扩展双 source_type；作废联动红字 | ✅ |
| 3 | 门禁回扫：报销关联拒销项、进项台账排除 output、query_invoices direction 过滤、update 不可改向 | ✅ |
| 4 | ar.rs：create/cancel 核销（月结保护+超余额拦截）、应收台账（账龄分桶）、账龄汇总矩阵 | ✅（10 单测） |
| 5 | 提醒：dashboard ar_overdue（>90 天）+ reminders.rs OS 通知分派（level=overdue） | ✅ |
| 6 | 月结检查：ar_aging_overdue warning（不阻断，跳转 /ar-ledger） | ✅ |
| 7 | 导出：export_ar_aging_report（客户×分桶矩阵+合计）、export_output_tax_ledger（区间+月度小计） | ✅ |
| 8 | 命令注册 6 条（get_ar_ledger/get_ar_allocations/create/cancel/export×2） | ✅ |
| 9 | 后端全量 390 测试通过 + 提交 | ✅ |
| 10 | 前端：发票页方向 Tab + 应收台账页 + Dashboard 提醒类别（subagent） | 🔄 |
| 11 | 集成回归 + 文档四件套 + push | ⏳ |

## 实现决策与偏差记录

- **凭证分派内嵌而非独立钩子**：maybe_generate_invoice_expense_voucher 读 direction 后分派到
  maybe_generate_sales_invoice_voucher；void_invoice_expense_voucher 扩展为双 source_type
  （invoice_expense + sales_invoice，void 幂等）——insert/update/soft_delete 三处调用点零改动。
- **vouchers CHECK 约束**：'sales_invoice' 需重建表（SQLite ALTER 不能改 CHECK），
  照搬 stage7/8 的 rebuild_vouchers_source_type_check 模式（幂等跳过/原序复制/FK 兜底）。
- **direction 不可改**：update_invoice 校验后拒绝（凭证口径与台账归属依赖方向），改向须作废重录。
- **去重索引不动**：(code, number) 全局唯一，本公司销项与进项不冲突（同号冲突走既有报错）。
- **核销不生成凭证**：收款单已按 counter_account 1122 入账（借银行/贷应收），核销只做台账勾稽。
- **红票（负数）边界**：amount+tax ≤ 0 时跳过凭证（与费用口径一致），留待后续按需支持负数行。
- **月结检查 cutoff 与提醒卡统一**：今天−90 天（最初写月首−3 月被修正，口径一致性）。
- **发票号入库 lowercase 归一**：测试断言需小写（normalize_invoice_number 既有行为）。
- **批量补 InvoiceInput 字段的一次事故**：脚本误把 `fn ... -> InvoiceInput {` 签名当构造器，
  吞了 6 处构造器的字段行，从 git diff 逐一恢复（db/invoice 手工 + accounting/cashier 脚本）。

## 挂账

- Windows exe 手工验收：销项 OCR 录入→凭证核对→收款核销→账龄提醒→两导出全流程（并入 v0.10 验收清单待补一节）
- 承兑票据与应收台账自动联动核销（spec 明确不做，手工核销入口已可用）
- 手册补收入侧章节（随发版统一更新）
