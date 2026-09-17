---
name: stage8-notes-tax-reports
description: 第八阶段（票据台账、账期提醒、现金盘点、资金日报、申报导出、进项台账）设计要点、进度与已知边界
---

# 第八阶段：票据台账与申报导出（进行中）

设计：`docs/superpowers/specs/2026-09-18-stage8-notes-tax-reports-design.md`（4.1 科目已勘误：2201=应付票据、2202=应付账款；借应付账款 2202/贷应付票据 2201）

计划：`docs/superpowers/plans/2026-09-18-stage8-notes-tax-reports.md`（3 批次 12 任务）

进度：`docs/superpowers/plans/2026-09-18-stage8-progress.md`

## 关键口径（阻断级）

- 票据凭证分录以勘误后 spec 4.1 为准；贴现息 fee>0 显式 6603 腿（凭证永平）、proceeds>face 严格拒绝
- 冲正恢复前置状态：背书/贴现→holding、承兑到账→holding、开出兑付→issued_outstanding、支票→void
- vouchers.source_type 白名单已含 instrument/cash_count；approval_events.entity_type 已含 negotiable_instrument
- 支票登记即终态（collected/paid），无持有态
- 月结双查：票据登记月+操作月均须 open，跨月流转允许

## 状态

- 8A 已交付：票据台账（DDL/状态机/命令/页面）+ 账期提醒，290 测试
- 8B 待做：现金盘点、资金日报
- 8C 待做：个税申报导出、进项台账、Minor 打磨、收尾
