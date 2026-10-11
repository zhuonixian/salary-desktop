# 第十阶段进度：上手引导与办税落地辅助

spec：`docs/superpowers/specs/2026-10-11-stage10-onboarding-tax-filing-design.md`
plan：`docs/superpowers/plans/2026-10-11-stage10-onboarding-tax-filing.md`
基线：v0.9.0（后端 360 测试）

## Task 状态

| # | 任务 | 状态 |
|---|---|---|
| 1 | db.rs：create_tables 增 tax_filing_records（新表，无 ALTER 迁移） | ✅ |
| 2 | onboarding.rs：bootstrap_status（含 setup_wizard_done/checklist_dismissed/has_demo_data/company 信息） | ✅ |
| 3 | onboarding.rs：公司信息 get/save（app_settings，空串回 None） | ✅ |
| 4 | onboarding.rs：示例账套 seed/clear（双门禁 + marker + 失败回滚 + FK 倒序清理） | ✅ |
| 5 | onboarding.rs：征期日历（规则常量/期次生成/启用设置/完成留痕幂等） | ✅ |
| 6 | commands.rs + lib.rs：12 条新命令注册 | ✅ |
| 7 | 后端单元测试 9 个（seed 往返/隔离/混入拒绝/征期跨年跨季/幂等） | ✅（369 全绿） |
| 8 | 10B excel.rs 三导出（subagent 并行） | ✅（3 新测试，372 全绿） |
| 9 | 前端 SetupWizard + Dashboard 两卡 + 导出中心（subagent 并行） | 🔄 进行中 |
| 10 | 集成：excel 命令包装/注册 + 全量回归 | ✅（export 命令已注册） |
| 11 | 文档四件套（memory 收尾/progress/手册挂账/发版说明） | ⏳ |

## 后端 API（已注册命令）

get_bootstrap_status / get_company_info / save_company_info / seed_demo_data /
clear_demo_data / get_tax_calendar / get_tax_calendar_settings / save_tax_calendar_settings /
finish_setup_wizard / dismiss_setup_checklist / complete_tax_filing / undo_tax_filing /
export_eit_payer_template / export_eit_salary_template / export_social_base_declaration

## 实现决策与偏差记录

- **工资规则清单项不展示**：insert_default_data 预置 salary_rules，新库恒非空，清单该项无意义；bootstrap_status 仍返回真实状态。
- **示例账套自动匹配样本**：mark_payment_batch_paid 必须先导出（写用户文件），种子不代做——批次停在"已生成"，种子流水含一条与批次金额一致的记录，用户标记已付款后在银行对账页点自动匹配即可命中。付款单同样停在"已审批"（payment 类型须经批次付款后结算）。
- **种子含 2 个示例往来单位**（收款/付款单校验要求往来单位或员工），clear 校验从"必须 0 行"改为"数量与 marker 一致"。
- **is_empty_ledger 判定扩为 16 表**（比 spec 7 表宽）：含 month_closes/business_partners/票据/盘点等，clear 前置校验月结非空直接拒绝。
- **凭证平衡断言用容差 0.005**：SUM 浮点和直接 `!=` 会误报。
- **PRAGMA foreign_key_check 空结果集**：query_row 报 QueryReturnedNoRows，须 unwrap_or(0)。
- **删除顺序关键点**：voucher_lines.fund_account_id 外键 → 凭证行/凭证必须先于 fund_accounts 删除。
- **demo 标记存 app_settings.demo_marker**（JSON：实体 id 集合 + 月份 + 操作人 id）；clear 校验"数量一致 + 无月结/票据/盘点"后整表清理（等价且简单于逐 id），保留科目/规则/设置/审计日志/OCR 记录。
- **种子发票 employee_id 与报销单员工对齐**：报销单校验"发票报销人与报销单一致"。

## 挂账

- 手册更新（docs/user-guide-v2.html）：该文件有用户未提交的 v0.9.0 手册改动，为避免冲突本阶段手册更新挂账，待用户提交后补第十阶段章节。
- Windows exe 手工验收：向导 7 步 → 示例账套体验（含清除/重建）→ 真实建账；扣缴端两模板人工导入核对列序（税局模板可能更新）；征期卡完成/撤销。
- graphify update（收尾时统一执行）。
