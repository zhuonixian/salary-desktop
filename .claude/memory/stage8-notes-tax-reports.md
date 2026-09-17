---
name: stage8-notes-tax-reports
description: 第八阶段（票据台账、账期提醒、现金盘点、资金日报、申报导出、进项台账）已交付能力、关键口径与已知边界
---

# 第八阶段：票据台账与申报导出（已交付）

设计：`docs/superpowers/specs/2026-09-18-stage8-notes-tax-reports-design.md`（4.1 科目已勘误：2201=应付票据、2202=应付账款；借应付账款 2202/贷应付票据 2201）

计划：`docs/superpowers/plans/2026-09-18-stage8-notes-tax-reports.md`（3 批次 12 任务，已全部交付）

进度：`docs/superpowers/plans/2026-09-18-stage8-progress.md`

## 已交付能力

- **票据台账**（`notes.rs` + `NotesInstruments.tsx`，菜单「资金出纳 → 票据台账」）：四表 DDL（negotiable_instruments / instrument_endorsements）+ partial unique（同类型同号唯一，void 除外）；状态机（holding → endorsed_out/discounted/collecting → collected；issued_outstanding → paid；支票登记即终态）；登记/背书/贴现/托收/到账/兑付/作废/红字冲正 10 命令；背书链 Drawer；凭证联动与 `vouchers.source_type='instrument'`
- **账期提醒**（`db.rs get_dashboard_reminders` + Dashboard 提醒卡）：借款到期（settled 未清余额扣核销）/ 票据到期（holding/collecting/issued_outstanding）/ 滞留应付（已审批未付款），提前天数 3/7/15/30 可配（`app_settings.reminder_advance_days` 缺省 7），卡片可跳转对应页面
- **现金盘点**（`cash_count.rs` + `CashCount.tsx`，菜单「资金出纳 → 现金盘点」）：draft/confirmed/void 状态机、快速+面额双模式、盘亏借 1901/贷 1001、盘盈反向、差异≠0 原因必填、confirmed 走红字冲正作废、`vouchers.source_type='cash_count'`
- **资金日报**（`cashier::get_fund_daily_report` + `excel::export_fund_daily_report`）：期初+收−支=期末勾稽、按账户明细、两 sheet Excel；入口「资金日记账 → 导出日报」（敏感门禁与日记账导出一致）
- **个税扣缴申报表**（`excel::export_tax_withholding_declaration`）：仅锁定月份可导出（后端门禁）、三险拆列（台账份额优先，全局规则键兜底，比例≠100% 退回合并展示留备注）、列头对齐电子税务局措辞；入口「工资计算 → 扣缴申报表」+ 导出中心
- **进项台账**（`db::get_input_tax_ledger` + `excel::export_input_tax_ledger` + `InputTaxLedger.tsx`，菜单「票据报销 → 进项台账」）：区间查询、月度小计、报销单反查、void 排除、导出 `进项台账_YYYYMM-YYYYMM.xlsx`
- **Minor 12 项 + 收口**：stage7 审查留档 12 项全部消化（冲正跨月口径显式化、附件统计单遍聚合、预算费用类型下拉、报销报错中文化、mock 状态机 default 不恒 true 等）；Task 9 挂账收口补社保三险个人份额端到端（social_insurance_profiles 三列 + salary_rules 三键 `upsert_salary_rule_key` 命令 + SocialInsurance/SalaryRules 入口）

## 关键口径（阻断级）

- 票据凭证分录以勘误后 spec 4.1 为准；贴现息 fee>0 显式 6603 腿（凭证永平）、proceeds>face 严格拒绝
- 冲正恢复前置状态：背书/贴现→holding、承兑到账→holding、开出兑付→issued_outstanding、支票→void
- vouchers.source_type 白名单：instrument / cash_count；approval_events.entity_type 含 negotiable_instrument
- 支票登记即终态（collected/paid），无持有态
- 月结双查：票据登记月+操作月均须 open，跨月流转允许

## 已知边界

- **日报 vs 日记账口径**：资金日报按 `voucher_date`（凭证记账日期）取数，资金日记账按 `belong_month`（业务归属月）过滤；正常单据两者一致，跨月调整凭证（如补记上月）会在两视图间出现一天/一月的口径差，属设计内行为
- **盘点负 id 冲正锚点**：盘点差异凭证冲正生成的反向凭证以「负盘点单 id」为 source_id 锚点（复用 stage7 冲正负 id 约定），从凭证反查盘点单时需按绝对值解析
- **三险比例配置端到端**：台账年度三列份额优先，缺省回落全局 `salary_rules` 三键（pension/medical/unemployment_personal_rate，`upsert_salary_rule_key` 0~1 白名单校验）；份额合计≠100% 时申报表三险整列退回合并展示并留备注，0=未配置
- 部分背书、票据池融资、ECDS 对接、进项认证状态、在线申报不做（spec 13 范围外）

## 回归基线

- 后端 `cargo test --lib`：324 passed（8A 收尾 290 → 8B 307 → 8C 324）
- 前端 `npx tsc -b` / `npm run lint` / `npm run build` 全过（勿用 `tsc --noEmit`：根 tsconfig 仅 refs+files:[]，为空检查）

## Windows 验收清单（挂账，随 v0.8.0 发版前手工执行）

1. **票据全流程**：登记承兑（收/开两个方向各一）→ 背书转出 → 贴现（含贴现息）→ 托收 → 到账确认 → 兑付开出票据 → 已流转票据红字冲正（核对原/反向凭证并存、票据状态回滚）；支票登记即终态
2. **盘点差异凭证**：新建现金盘点（面额明细模式合计≠实存应拦截）→ 差异≠0 确认生成 1901 凭证 → 冲正作废该盘点单
3. **提醒卡**：造一笔临期借款 + 临期票据 + 滞留报销单，仪表盘账期提醒卡三类齐出、天数/金额正确（脱敏态金额打码）、点击跳转正确；提前天数改 15 后提醒面扩大
4. **三导出**：资金日报（日记账页按钮）、个税扣缴申报表（未锁定月份应被拒、锁定月份导出三险拆列正确）、进项台账（区间+月度小计）
5. **三险配置**：社保台账录三列份额 → 申报表按份额拆列；清空份额回落全局规则三键
6. **既有 v0.7 挂账项**：见 `.claude/memory/stage7-cashier-operations.md` 的 Windows 验收清单（资金账户/收付款审批/批次/多对多对账/日记账/借款核销/月结联动）

## Minor 留档（终审 triage 全部不阻断，后续打磨）

冲正后 fund_account_id 残留；AMOUNT_TOLERANCE notes.rs 本地重定义待收敛 pub(crate)；票据 belong_month 登记月口径跨月需切月；贴现日志 .max(0.0) 冗余；mock 登记不落审批事件；NotesInstruments.tsx 1400 行可拆；提醒 approved_at 空串不回落（建议 NULLIF）；滞留恰 N 天边界未直测；盘点前端凭证号显示裸 voucher_id；快速模式参考差异渲染滞后；excel.rs 反向依赖 cashier；申报门禁计数未滤 void；进项台账映射变量名 code 实为 name；台账页月份初值不随业务月；进项 Excel 未断言报销单号列；全局三险键后端不校验和 100%（前端拦+导出兜底）；upsert_salary_rule_key 预览无 mock case；申报合并路径仅单测覆盖（Windows 验收人工核对一份）
