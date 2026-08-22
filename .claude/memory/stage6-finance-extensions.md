---
name: stage6-finance-extensions
description: 第六阶段财务功能拓展（科目余额表、年末结转、社保台账、累计预扣、工资条、同期列）计划与开发接力摘要
---

# 第六阶段：财务功能拓展

完整计划：`docs/superpowers/plans/2026-08-22-stage6-finance-extensions.md`
spec：`docs/superpowers/specs/2026-08-22-stage6-finance-extensions-design.md`
进度同步：`docs/superpowers/plans/2026-08-22-stage6-progress.md`

## 定位

在第五阶段凭证与报表底座上补全账簿与结账闭环、社保公积金全链路、个税累计预扣：科目余额表（试算平衡）、12 月月结自动年末结转、社保台账（基数/费率/调基/上下限）与工资计算/计提凭证联动、个税累计预扣法与年度汇总、工资条预览打印、三大报表上年同期对比列。

## 已交付能力

1. 科目余额表：区间试算平衡（期初/本期发生/期末借贷双侧），含全部凭证（含 period_close，反映真实账面）；财务报表页第 4 个 Tab + Excel 导出；试算不平衡红色提示
2. 年末结转：12 月正式月结自动生成两张 period_close 凭证（损益→3103 本年利润、3103→3104 未分配利润），反月结同事务作废；月结工作台 12 月显示"年末结转"检查项
3. 报表口径：三大报表计算统一排除 period_close 分录（利润表累计口径不因结转凭证归零）
4. 社保台账：`social_insurance_profiles` 表按员工×年度（UNIQUE），社保/公积金基数与四方费率；年度调基复制（×系数 + 上下限 clamp，目标年度非空拒绝）；基数上下限存 app_settings（0=不限制）
5. 工资计算挂接：有台账优先取台账（基数 clamp、费率含单位部分），`social_security_employer` / `housing_fund_employer` 落库 `salary_monthly_results`；无台账回退员工基数/全局费率、单位部分 0（行为与旧版一致）
6. 计提凭证升级：全额成本口径——借 费用科目(应发净额+单位社保公积金)、贷 2211 同额；代扣腿——借 2211(个人社保公积金+个税)、贷 2241 / 2221（全 0 时不生成代扣行）
7. 个税累计预扣法：`tax_rules.scope` 列 + 累计 7 档预扣率表；`calculate_cumulative_tax` 按当年已存工资记录聚合累计应纳税所得额与已预扣，无迁移平滑启用；工资计算改走累计预扣
8. 个税年度汇总：按员工×年度汇总月度收入/扣除/已预扣，SalaryCalculate 页"个税年度汇总"弹窗 + Excel 导出
9. 工资条：SalaryCalculate 页"工资条"预览与打印，含明文金额，需先解锁敏感数据（5min 全局窗口）
10. 报表同期列：资产负债表年初、利润表/现金流量表新增"上年同期"列（`has_prior_year` 标志，无历史数据隐藏列），Excel 导出同步

## 批次划分

- 批次一（Task 1-4）：科目余额表引擎/命令/导出/Tab + 年末结转生成作废/报表口径排除 + 月结挂接
- 批次 2a（Task 5-8）：社保台账 DDL/CRUD + 命令页面 + 工资计算挂接 + 计提凭证升级
- 批次 2b（Task 9-10）：个税累计预扣 + 年度汇总
- 批次 2c（Task 11）：工资条打印
- 批次三（Task 12）：报表上年同期对比列
- 收尾（Task 13）：全量回归 + 文档四件套

## 测试门槛

```bash
npx tsc --noEmit
npm run lint
npm run build
cd src-tauri && cargo fmt --check
cd src-tauri && cargo check
cd src-tauri && cargo test --lib
```

收尾基线：141 个后端测试通过；cargo 既有 5 个 dead_code/unused warning 可保留。

## 已知边界

- **启用月之前存量数据报表为 0 属预期**：启用月之前或无凭证期间的数据不进报表滚入窗口，Windows 验收时不要误判为 bug。
- **个税历史月按月度算法视为已预扣**：切换累计预扣法前已存的税额自然作为"累计已预扣"基数参与抵减，不做历史重算（专项附加按当月值×月数近似）。
- **利润表上年同期为启用月起累计口径**：上年早于启用月的部分为 0，同期列只在两年度都有数据时有意义（`has_prior_year` 控制）。
- **Windows 手工验收待做**：科目余额表 Tab、12 月月结结转、社保台账页、工资条打印、同期列展示需在 Windows exe 下人工验证。
