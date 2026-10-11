# 第十阶段设计（草案）：上手引导与办税落地辅助

日期：2026-10-11
状态：草案（待审阅）
基线：v0.9.0（第九阶段已发版，后端 360 测试）

## 1. 背景与目标

产品主张"帮助出纳独立完成全流程"，但当前存在两个体验断层：

1. **冷启动重**：首次配置（启动密码、员工、工资规则、社保基数、期初余额）分散在 8 个页面，13 章手册虽全，上手仍是一堵墙；且无示例数据，新用户无法在注册真实员工前先体验完整闭环。
2. **报税最后一公里**：个税扣缴申报表已能导出（列序对齐申报录入页），但离"报完税"还差一步——扣缴端是按**导入模板**接收数据的；社保年度调基也要另出申报表；各税种征期靠用户记忆。

第十阶段目标：**零外联原则不变**，补齐"第一次使用不迷路"和"导出即可导入申报"两段体验。分三个批次：10A 上手引导（向导/示例账套/检查清单）、10B 申报模板三件、10C 办税征期日历。

## 2. 范围

| 模块 | 深度 | 新表/列/键 |
|---|---|---|
| 首启向导 | 7 步可跳过向导，复用既有保存命令 | app_settings.setup_wizard_done |
| 示例账套 | 一键种子/一键清除，强隔离门禁 | app_settings.demo_marker |
| 首页检查清单 | 动态完成态 + 可关闭 | app_settings.setup_checklist_dismissed |
| 企业信息 | 公司名/信用代码，供申报表头 | app_settings.company_name 等 |
| 申报模板 | 扣缴端人员/工资导入模板 + 社保基数申报表 | 无新表（全部复用既有数据） |
| 征期日历 | 内置规则 + 期次完成留痕 + Dashboard 卡片 | tax_filing_records |

## 3. 数据模型

### 3.1 app_settings 新键

- `setup_wizard_done`：'1' 后不再自动进入向导（手动重进入口不受限）
- `setup_checklist_dismissed`：'1' 后首页清单卡不再显示
- `company_name` / `taxpayer_id`：企业名称与统一社会信用代码（可空），10B 申报表表头消费；员工管理/导出中心不在本期改造

### 3.2 tax_filing_records

```sql
CREATE TABLE tax_filing_records (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  period TEXT NOT NULL,              -- '2026-10'（月）/ '2026-Q3'（季）/ '2026'（年）
  kind TEXT NOT NULL,                -- 'eit_withholding'|'cit_quarterly'|'eit_annual'|'gs_annual_report'|'social_base'
  completed_at TEXT NOT NULL,
  operator TEXT NOT NULL,
  UNIQUE (period, kind)
);
```

期次数据不落库：由内置规则即时生成（见 §6），本表仅记"已完成"，删除记录即恢复提醒。

## 4. 10A 上手引导

### 4.1 bootstrap_status 命令

聚合检测并返回检查清单状态，前端向导与首页清单共用：

```json
{
  "has_password": true,            // security 模块已启用（三 KEK 之一存在）
  "has_employees": false,
  "has_salary_rules": false,
  "has_social_profiles": false,
  "has_opening_balance": false,     // opening_balances 任一月份有行
  "has_locked_salary": false,       // salary_results 存在 status='已锁定'
  "is_empty_ledger": true           // 核心业务表全 0 行（示例账套门禁用）
}
```

`is_empty_ledger` 判定范围：employees / salary_results / vouchers / fund_accounts / fund_documents / bank_transactions / invoices 全为 0 行。

### 4.2 首启向导（路由 /setup）

自动进入条件：`!has_password && is_empty_ledger`；每步均可"跳过"。

| 步 | 内容 | 复用 |
|---|---|---|
| 1 | 欢迎 + 定位一句话（本地单机/数据不出本机/可先体验示例） | — |
| 2 | 设置启动密码（密码/找回码/安全问题） | setup_security |
| 3 | 企业信息：公司名 + 统一社会信用代码（可跳过） | app_settings 写入 |
| 4 | 工资规则 + 社保基数：预填当前常规默认值供修改 | upsert_salary_rule_key、社保 profile 命令 |
| 5 | 员工导入：下载模板 / Excel 导入 / 手工加一名 / 跳过 | 既有导入命令 |
| 6 | 期初余额：跳转科目表页说明（不内嵌编辑） | 深链 + 返回续向导 |
| 7 | 完成：账套为空时询问「载入示例数据先体验？」→ Dashboard | seed_demo_data |

步骤 4/5 需敏感解锁态的按既有门禁（密码设置后即处于解锁态，正常流程无感）。

### 4.3 示例账套 seed_demo_data / clear_demo_data

- **seed 门禁**：仅 `is_empty_ledger` 为真可执行；事务包裹；**全部走既有领域函数**（建员工→考勤导入→算薪→锁定→报销审批→收付款→流水核销），保证凭证联动与真实操作路径一致
- **内容**：8 名虚构员工（姓名/部门/身份证号用合规假号段/银行号/邮箱齐全）、当年 2 个月考勤与锁定工资、社保台账、3 张发票与 2 张报销单（含审批+付款）、2 个资金账户 + 收付款单 + 银行流水（含多对多核销样例）、一笔平衡期初；随业务事件自动生成凭证
- **隔离**：seed 结束将全部新建 id 范围写入 `demo_marker`（JSON）
- **clear 门禁**：除 marker id 外核心业务表仍全 0 行（未混入真实数据）才允许；按 FK 依赖倒序删除 marker 记录并清理 demo_marker 键；检测失败时报错"账套已有真实数据，示例数据请保留或逐项处理"
- 向导与 Dashboard 清单均注明：示例数据体验后请清除

### 4.4 首页检查清单卡

Dashboard 顶部 Card：按 bootstrap_status 列未完成项（启动密码→导入员工→工资规则→社保基数→期初余额→首次算薪锁定），每项带跳转链接；全部完成或点"不再显示"（setup_checklist_dismissed）即消失。账期提醒卡（阶段 8）保持独立不受影响。

## 5. 10B 申报模板（excel.rs + 导出中心入口）

三个新导出命令，全部复用 rust_xlsxwriter，入口挂导出中心并写 operation_logs（惯例键名）：

### 5.1 export_eit_payer_template（人员信息采集）

- 全体在职员工一行：身份证件号码、姓名、身份证件类型（居民身份证）、是否雇员、入职日期等
- 列序对齐自然人电子税务局（扣缴端）**人员信息采集**导入模板；导出页固定提示："税局模板可能更新，若列序不符请以扣缴端当前模板为准并反馈调整"
- 身份证号为敏感字段：受敏感解锁门禁（明文导出需解锁态，同现有扣缴申报表）

### 5.2 export_eit_salary_template（正常工资薪金收入）

- 按月导出，**仅锁定月份**（复用 export_tax_withholding_declaration 的门禁与报错文案）
- 取数复用扣缴申报表既有查询（工资结果联员工表），列序改为扣缴端**正常工资薪金收入**导入模板：本期收入、基本养老/医疗/失业保险费、住房公积金、累计收入、累计减除费用、累计专项扣除、累计专项附加扣除、累计已预扣预缴税额等
- 无专项附加扣除的员工填 0，不缺列

### 5.3 export_social_base_declaration（社保缴费基数年度申报表）

- 员工 × 年度，来源 social_insurance_profiles（基数/费率/上下限截断结果）
- 表头含单位名称与统一社会信用代码（消费 app_settings.company_name / taxpayer_id，未配置时留空并在表尾注明"请补填"）

## 6. 10C 办税征期日历

### 6.1 内置规则（Rust 常量，不入库）

| kind | 期次 | 截止 | 开关 |
|---|---|---|---|
| eit_withholding | 每月（上月工资） | 次月 15 日 | 默认开 |
| cit_quarterly | 每季 | 季后 15 日 | 默认关（按需开启） |
| eit_annual | 每年 | 6-30 | 默认开 |
| gs_annual_report | 每年 | 6-30 | 默认开 |
| social_base | 每年 | 7-15（按当地，可配天数偏移） | 默认关 |

**设计决策**：不内置节假日顺延表——截止日按自然日计算，卡片固定副文案"如遇节假日顺延，以税局公告为准"。避免本地维护一份逐年公告数据带来错误信息。

### 6.2 呈现与留痕

- `get_tax_calendar` 命令：生成当前月起 3 个月的期次视图，关联 tax_filing_records 与 eit_withholding 的"上月工资已锁定"软状态
- Dashboard 新增「征期提醒」分组（并入账期提醒卡区域，样式复用 stage8 提醒卡）：15 日内待办金色、逾期红色、已勾完成灰色
- 勾「已完成本期申报」→ 写 tax_filing_records（operator 记录），该期不再提醒；可撤销
- 仅应用内提醒，不做 OS 系统通知（见不做范围）

## 7. 错误处理与边界

- 非空账套误触 seed → 明确报错并指引"示例数据仅限全新账套体验"
- clear 校验失败 → 保留数据原状，报错不含删除细节（防止误导手工清理）
- 模板列序与税局版本不符 → 导出页常驻提示 + 反馈调整路径，不做在线更新
- 征期计算不依赖系统时区设置错误：期次按本地日期（与 belong_month 口径一致）
- tax_filing_records 的 UNIQUE 冲突 → 幂等返回成功

## 8. 测试策略

- 单元测试（`Connection::open_in_memory()`）：seed 幂等与隔离门禁、clear 的 FK 倒序删除与"混入真实数据拒绝"、模板取数仅锁定月份、申报表表头字段回填、期次生成跨年/跨季/闰年边界、tax_filing_records 幂等
- 命令层：bootstrap_status 各布尔位正确性（逐项造数验证）
- 前端：`npx tsc -b` + ESLint；向导各步跳过/返回状态保持
- 全量回归：seed → 完整月度节奏 → clear → 真实向导建账，核对凭证借贷平衡与报表非 0

## 9. 交付批次与任务划分（草案）

- **10A**（Task 1-4）：bootstrap_status 命令 → 向导页面 → 示例账套 seed/clear → 首页清单卡
- **10B**（Task 5-7）：人员信息采集模板 → 工资薪金导入模板 → 社保基数申报表
- **10C**（Task 8-9）：征期规则与期次生成 → Dashboard 征期卡与完成留痕
- 收尾：导航核对、全量回归、文档四件套（memory/progress/手册/发版说明）

## 10. 不做范围（未来增强池）

- 节假日顺延精确化（逐年公告数据维护成本高于价值）
- 税局 API 对接 / 在线校验（违背零外联）
- 增值税、企业所得税申报表生成（需进销项与税会判断，超出本工具定位）
- PDF 版申报模板、批量多月份模板
- OS 级系统通知/托盘（与 10C 同属提醒增强，独立立项）
- 向导内嵌期初余额编辑（跳转科目表页即可，避免二次实现）
