# 第十阶段：上手引导与办税落地辅助（已交付）

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

## 关键集成决策（前端向导 × 安全门禁）

- **向导 standalone 模式**：全新账套（无密码+空账+未完成向导）时 App 启动门禁直接渲染
  `SetupWizard standalone`（HashRouter 包裹、route `*`），替代旧 SetupSecurity 首屏；
  探测失败回退 SetupSecurity。应用内 /setup 路由保留作手动重进入口（AppLayout 内）。
- **密码延迟生效**：standalone 下 setup() 会翻转 isInitialized 使门禁切换、向导被卸载，
  故第 2 步仅校验暂存，`finalizeStandalone` 在收尾按序执行：finish_setup_wizard →
  setup（已填且通过校验才执行，跳过向导也生效避免重填）→ `window.location.hash='#/'` →
  onFinished 重探测。应用内模式保持即时 setup。
- AppLayout 内原有的启动探测 effect 已移除（职责上移到 App 门禁，避免双探测）。

## 验收记录（开发机）

- cargo test --lib 372 通过（新增 9 onboarding + 3 excel）
- npx tsc -b / npm run lint 0 error
- Windows exe 手工验收挂账：向导全流程（含跳过/未设密码回退 SetupSecurity）、示例账套
  体验与清除（混入真实数据拒绝场景）、扣缴端两模板人工导入核对列序、征期卡完成/撤销
