# 第十二阶段：收入侧闭环——销项登记、应收台账与回款核销（规划中，spec 待审阅）

spec：`docs/superpowers/specs/2026-10-13-stage12-revenue-cycle-design.md`
plan：`docs/superpowers/plans/2026-10-13-stage12-revenue-cycle.md`（spec 审阅通过后补）
progress：`docs/superpowers/plans/2026-10-13-stage12-progress.md`（开工后建）

## 模块地图（规划）

- `invoices.direction` 列（'input'/'output'，存量默认 input）：销项复用发票 OCR/去重/加密归档底座
- 门禁回扫（阻断项）：报销关联/费用类型/进项台账/发票费用凭证/发票查询全部排除 output
- `ar_receipt_allocations`：收款单↔销项发票多对多部分核销（镜像 advance_settlement_links，取消留痕+月结保护）
- 销项凭证：借 1122（客户辅助）/贷 6001 amount /贷 2221 tax_amount；作废红字联动
- 应收台账页 + 账龄分桶（30/60/90/180）+ 提醒卡 ar_overdue（>90 天）+ OS 通知接入 + 两导出

## 关键设计

- 去重索引不变：(code, number) 全局唯一，本公司销项与进项天然不冲突
- 凭证金额全部取自票面（amount/tax_amount），无需税率配置
- 核销不生成新凭证（收款单已按 1122 入账，核销只做台账勾稽）
- 承兑票据与应收不做自动联动（票据台账已覆盖背书/贴现/托收），仅手工核销入口

## 关联

- v0.10.0 发版验收清单：`docs/superpowers/plans/2026-10-13-v0.10-release-acceptance.md`（阶段 7-11 汇编，发版先行）
