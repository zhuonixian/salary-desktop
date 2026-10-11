# 第十阶段：上手引导与办税落地辅助（进行中）

spec：`docs/superpowers/specs/2026-10-11-stage10-onboarding-tax-filing-design.md`
plan：`docs/superpowers/plans/2026-10-11-stage10-onboarding-tax-filing.md`
progress：`docs/superpowers/plans/2026-10-11-stage10-progress.md`

## 模块地图

- `onboarding.rs`（新）：bootstrap_status、公司信息（app_settings.company_name/taxpayer_id）、示例账套 seed_demo_data/clear_demo_data、征期日历（内置规则常量 + tax_filing_records 完成留痕）
- `db.rs`：create_tables 增 tax_filing_records（新表无需 ALTER 迁移）
- `excel.rs`：export_eit_payer_template / export_eit_salary_template（复用扣缴申报表取数与"仅锁定月份"门禁）/ export_social_base_declaration
- 前端：`SetupWizard.tsx`（/setup，7 步可跳过）、Dashboard 首页检查清单卡 + 征期提醒卡、导出中心三入口

## 关键设计

- 示例账套双门禁：seed 仅核心业务表全 0；clear 校验"除 demo_marker 外仍全 0"后整表清理（保留科目/规则/设置/审计日志），marker 记 app_settings
- seed 全走既有领域函数（db/salary/cashier/invoice/accounting），凭证联动与真实操作路径一致，本身就是活体回归链
- 征期规则为 Rust 常量（不入库），启用项存 app_settings.tax_calendar_enabled_kinds；不内置节假日顺延
- 公司名/信用代码存 app_settings，申报模板表头消费；未配置时表尾注"请补填"

## 已知边界

- 手册（docs/user-guide-v2.html）当时有用户未提交的 v0.9.0 手册改动，本阶段手册更新挂账待用户提交后补
- 扣缴端模板列序需 Windows exe 手工验收时人工导入核对（税局模板可能更新）
- 征期截止日按自然日计算，卡片注明"如遇节假日顺延，以税局公告为准"
