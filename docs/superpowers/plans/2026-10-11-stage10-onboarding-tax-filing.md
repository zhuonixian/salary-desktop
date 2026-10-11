# 第十阶段执行计划：上手引导与办税落地辅助

日期：2026-10-11
spec：`docs/superpowers/specs/2026-10-11-stage10-onboarding-tax-filing-design.md`
基线：v0.9.0（后端 360 测试）

## 批次

- **10A 上手引导**（Task 1-4）：bootstrap_status → 公司信息 → 示例账套 seed/clear → 向导/清单前端
- **10B 申报模板**（Task 5）：excel.rs 三导出（扣缴端人员/工资薪金导入模板、社保基数申报表）
- **10C 征期日历**（Task 6-7）：规则与期次生成 → Dashboard 征期卡与完成留痕
- **收尾**（Task 8）：集成回归、导航核对、文档四件套

## 后端 API 契约（命令层 ↔ 前端，所有模块共同遵守）

```text
get_bootstrap_status() -> {
  has_password, has_employees, has_salary_rules, has_social_profiles,
  has_opening_balance, has_locked_salary, is_empty_ledger,
  company_name, taxpayer_id          // Option<String>
}
save_company_info(company_name: Option<String>, taxpayer_id: Option<String>)
seed_demo_data() -> DemoSeedSummary { months: Vec<String>, employees, invoices, claims, fund_documents, bank_transactions, ... }
clear_demo_data() -> bool
get_tax_calendar(from_month: String, months: u32) -> [TaxCalendarItem]
  // item: { period, kind, label, due_date, days_left, status: 'pending'|'done', overdue, hint }
get_tax_calendar_settings() -> { enabled_kinds: Vec<String> }
save_tax_calendar_settings(enabled_kinds: Vec<String>)
complete_tax_filing(period, kind, operator) / undo_tax_filing(period, kind)
export_eit_payer_template(path) -> String
export_eit_salary_template(month, path) -> String
export_social_base_declaration(year, path) -> String
```

## 文件归属（防冲突）

| 文件 | 负责 |
|---|---|
| src-tauri/src/onboarding.rs（新建） | 主 agent：bootstrap_status、公司信息、征期日历、seed/clear |
| src-tauri/src/db.rs | 主 agent：create_tables 增 tax_filing_records |
| src-tauri/src/commands.rs、lib.rs | 主 agent：全部新命令包装与注册 |
| src-tauri/src/excel.rs | subagent B：三个导出函数 + 单测 |
| src/** | subagent F：SetupWizard、Dashboard 两卡、导出中心、路由 |

## 示例账套（seed_demo_data）实现要点

- 门禁：核心业务表（employees/salary_monthly_results/vouchers/fund_accounts/fund_documents/bank_transactions/invoices）全 0 行
- 内容：示例操作人（并设为当前）、8 员工（假号段身份证/example.com 邮箱）、当年社保台账、上月+当月考勤、上月算薪+复核+锁定（当月只算）、期初一对平衡、2 资金账户（期初与账簿一致）、3 发票、2 报销单（提交+审批）、收款/付款资金单各 1（审批+结算）、银行流水 5 条（1-2 条自动匹配已结算资金单）
- 全程走既有领域函数（db::/salary::/cashier::/invoice::/accounting::），事务包裹；marker 写 app_settings.demo_marker
- clear：校验"除 marker 外核心表全 0"→ 整表清空核心业务数据（保留科目/规则/设置/审计日志）→ PRAGMA foreign_key_check 兜底 → 清 marker
- 身份证号用 110101 1900 假号段，避免撞真实证件号

## 征期规则（onboarding.rs 常量，不入库）

- eit_withholding 每月（次月 15 日）、eit_annual / gs_annual_report 年度（6-30）、social_base 年度（7-15）、cit_quarterly 季度（季后 15 日）
- 默认启用：eit_withholding / eit_annual / gs_annual_report；cit_quarterly / social_base 默认关
- 不内置节假日顺延；卡片副文案"如遇节假日顺延，以税局公告为准"
- eit_withholding 期次关联软状态：上月工资已锁定与否（hint 展示）

## 门禁与口径

- 申报模板：仅锁定月份（复用 export_tax_withholding_declaration 门禁）；身份证号导出遵循敏感数据体系
- seed 仅限空账套；clear 二次校验防误删真实数据
- tax_filing_records UNIQUE(period, kind) 幂等；删除记录即恢复提醒

## 测试与验收

- cargo test --lib：seed 幂等/隔离、clear FK 倒序与混入真实数据拒绝、期次跨年跨季边界、bootstrap_status 各布尔位
- npx tsc -b + npm run lint（subagent F 自验 + 主 agent 复验）
- Windows exe 手工验收（挂账）：向导 7 步 → 示例账套体验 → 清除 → 真实建账；扣缴端模板人工导入一次核对列序
