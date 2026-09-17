# 第八阶段进度同步

本文件用于第八阶段（票据台账、账期提醒与申报导出）开发接力。每轮开发结束必须追加记录。

## 当前基线

- 分支：`master`
- 阶段计划：`docs/superpowers/plans/2026-09-18-stage8-notes-tax-reports.md`
- 设计说明：`docs/superpowers/specs/2026-09-18-stage8-notes-tax-reports-design.md`（4.1 已勘误：2201=应付票据、2202=应付账款）
- 长期摘要：`.claude/memory/stage8-notes-tax-reports.md`
- 自动测试基线：8A 收尾 290 passed；前端 `npx tsc -b` / lint / build 全过
- 工作区注意：用户未跟踪文件 `docs/user-guide-v2.html` 不得覆盖、删除或纳入提交

## 目标交付

票据台账（状态机+凭证联动+背书链）、账期提醒（借款/票据/滞留应付）、现金盘点单、资金日报、个税扣缴申报表导出、增值税进项台账、第七阶段 12 项 Minor 打磨。

## 批次状态

| 批次 | 状态 | 说明 |
|---|---|---|
| 8A | 完成 | 票据台账 + 账期提醒（Task 1-4）+ 批次收尾（Task 5） |
| 8B | 未开始 | 现金盘点、资金日报（Task 6-8） |
| 8C | 未开始 | 个税申报导出、进项台账、Minor、全量回归（Task 9-12） |

## 协作规则

- 开发前读 CLAUDE.md、本文件、stage8 spec 与 plan。
- SDD：每任务独立实现者+审查者双裁决；主 agent 统一集成、回归、commit。
- 旧库迁移、状态机、凭证分录、月结保护为阻断级验收项。
- 前端类型检查必须 `npx tsc -b`（`--noEmit` 在本仓库为空检查）。

## 8A 记录

### 2026-09-18 — 8A 票据台账与账期提醒（Task 1-5）完成

- Task 1（019620b）：四表 DDL（negotiable_instruments/instrument_endorsements/cash_count_sheets/cash_count_denominations）+ partial unique + reminder_advance_days 缺省 7 + 模型常量，262 测试。
- Task 2（c15ab8b+fix a993525/1a15ca1）：notes.rs 状态机 8 函数+凭证联动+红字冲正+白名单表重建（vouchers.source_type +instrument/cash_count、approval_events.entity_type +negotiable_instrument）。**重大插曲**：spec 4.1 原把 2201/2202 编码名标反致实现借贷科目互换（开出承兑/支票/背书/兑付四处），审查揭示后勘误 spec（0e70f4f）并两轮 fix 修复；贴现息 fee>0 显式成腿保证凭证永平。285 测试。前实现者中途挂起由接手者零重写续完。
- Task 3（e78adc8）：10 命令+查询层+NotesInstruments 页（三 Tab+状态驱动行操作+背书链 Drawer+冲正弹窗）+mock 状态机+日志映射，286 测试。
- Task 4（6f28e5b）：三类账期提醒（借款 settled 未清扣核销/票据三活跃态/滞留应付 approved+batched）+提前天数 3/7/15/30 配置+仪表盘卡跳转，290 测试。
- Task 5：本批次收尾（本文件 + memory 骨架），全量回归六命令过。
- Minor 挂账（8C Task 11 消化）：冲正后 fund_account_id 残留；AMOUNT_TOLERANCE notes.rs 本地重定义待收敛 pub(crate)；belong_month 登记月口径跨月票据需切月；贴现日志 .max(0.0) 冗余；mock 登记不落审批事件；NotesInstruments 1400 行可拆；approved_at 空串不回落（建议 NULLIF）；前端 days=0 显示 7；滞留恰 N 天边界未直测。
- 环境事实：bundled SQLite FK 默认开，新表测试需先种父行；后台 agent 可能长时间无输出挂起（Task 2 曾发生，超时唤不醒即 TaskStop+接手者续做）。
- 下轮入口：Task 6 现金盘点单（cash_count.rs，表已建、cash_count source_type 已预置）。
