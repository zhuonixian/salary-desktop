# 第八阶段进度同步

本文件用于第八阶段（票据台账、账期提醒与申报导出）开发接力。每轮开发结束必须追加记录。

## 当前基线

- 分支：`master`
- 阶段计划：`docs/superpowers/plans/2026-09-18-stage8-notes-tax-reports.md`
- 设计说明：`docs/superpowers/specs/2026-09-18-stage8-notes-tax-reports-design.md`（4.1 已勘误：2201=应付票据、2202=应付账款）
- 长期摘要：`.claude/memory/stage8-notes-tax-reports.md`
- 自动测试基线：8C 收尾 324 passed；前端 `npx tsc -b` / lint / build 全过
- 工作区注意：用户未跟踪文件 `docs/user-guide-v2.html` 不得覆盖、删除或纳入提交

## 目标交付

票据台账（状态机+凭证联动+背书链）、账期提醒（借款/票据/滞留应付）、现金盘点单、资金日报、个税扣缴申报表导出、增值税进项台账、第七阶段 12 项 Minor 打磨。

## 批次状态

| 批次 | 状态 | 说明 |
|---|---|---|
| 8A | 完成 | 票据台账 + 账期提醒（Task 1-4）+ 批次收尾（Task 5） |
| 8B | 完成 | 现金盘点、资金日报（Task 6-8） |
| 8C | 完成 | 个税申报导出、进项台账、Minor、全量回归（Task 9-12） |

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

### 2026-09-18 — 8B 现金盘点与资金日报（Task 6-8）完成

- Task 6（fc30c84）：cash_count.rs 领域层（create/update/confirm/void，快速+面额双模式、差异凭证 盘亏借1901/贷1001 盘盈反向、confirmed 红字冲正负 id 锚点文档化）+ CashCount.tsx + 6 命令，304 测试。
- Task 7（54b74c0）：get_fund_daily_report 勾稽（期初+收−支=期末、跨月边界）+ 两 sheet Excel 导出（calamine 回读断言）+ FundJournals「导出日报」按钮，307 测试。
- Task 8：批次收尾全量回归（307/tsc-b/lint/build 全过）。
- 下轮入口：Task 9 个税扣缴申报表导出。

### 2026-09-18 — 8C 申报导出、Minor 与收尾（Task 9-12）完成 —— 第八阶段收官

- Task 9（1e9ab8d + abd5b25）：`export_tax_withholding_declaration`——仅锁定月份门禁（后端兜底）、三险拆列（台账份额优先 → 全局规则键兜底 → 比例≠100% 整列退回合并展示留备注）、列头对齐电子税务局措辞；入口「工资计算 → 扣缴申报表」+ 导出中心；累计预扣口径抽共享函数消除逻辑复制。
- Task 9 收口 / Minor 13（38b0783）：社保三险个人份额端到端——social_insurance_profiles 三列 + salary_rules 三键（pension/medical/unemployment_personal_rate）+ `upsert_salary_rule_key` 命令（白名单 + 0~1 校验 + 留痕）+ SocialInsurance/SalaryRules 前端入口；项 8 mock 兜底 default 不再恒 true。324 测试（318+6）。
- Task 10（6faa59b）：进项台账——`get_input_tax_ledger` 区间查询（void 排除、月度小计、报销单反查）+ `export_input_tax_ledger` Excel + InputTaxLedger.tsx 页（菜单「票据报销 → 进项台账」），页内明示「发票登记 ≠ 进项认证」。
- Task 11（11268a4 + e1af425）：stage7 Minor 12 项全部消化——后端（冲正跨月口径注释/测试、存量核销 1221 回落、对账同分平局断言显式化、attachment_disk_stats 单遍聚合、月结口径注释、报销报错中文化）+ 前端（核销草稿透传 settlement_mode/due_date、bank_manual 下拉收窄与幂等结构化、伪未达两步恢复引导、借款统计口径提示、预算费用类型下拉化）。
- Task 12（本次 commit）：收尾——导航核对（资金出纳 8 项/票据报销 3 项与 App.tsx 一致，零修改）；OperationLogs 映射核对（补 `set_reminder_advance_days`/`upsert_salary_rule_key` 两枚 stage8 新留痕命令）；全量回归（324 测试/fmt/tsc -b/lint/build 全过 + graphify update）；文档四件套（CLAUDE.md 架构摘要+Memory References+第八阶段段落、stage8 memory 骨架完善为已交付+已知边界+Windows 验收清单、本文件、user-guide.html 第八章卡片+菜单速查+版本历史）。
- Windows exe 手工验收清单已固化到 `.claude/memory/stage8-notes-tax-reports.md`（票据全流程+冲正、盘点差异凭证、提醒卡、三导出、三险配置、既有 v0.7 项）。
- 发版建议：v0.8.0（待确认后走 `.claude/memory/release-workflow.md` 流程）。

## 阶段完成

第八阶段（票据台账、账期提醒与申报导出）Task 1-12 全部交付：3 批次（8A 票据+提醒 / 8B 盘点+日报 / 8C 申报+进项+Minor+收尾），后端 324 测试全过，前端 tsc -b / lint / build 全过。遗留：Windows exe 手工验收挂账（清单见 stage8 memory）；发版 v0.8.0 待用户确认。
