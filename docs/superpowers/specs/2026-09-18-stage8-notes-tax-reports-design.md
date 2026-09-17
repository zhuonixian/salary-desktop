# 第八阶段设计：票据台账、账期提醒与申报导出

日期：2026-09-18
状态：待审阅
基线：v0.7.0（HEAD 3cd204c，第七阶段出纳运营闭环已交付，后端 255 测试）

## 1. 背景与目标

第七阶段已把资金账户、收付款、付款批次、多对多银行对账、资金日记账、余额调节表、借款核销串成出纳运营闭环。第八阶段从资深出纳视角补齐四类高频缺口：

1. **票据管理缺位**——承兑汇票的收付、背书、贴现、托收目前无登记，应收/应付票据科目余额与实际票据脱节
2. **账期靠脑记**——借款到期、票据到期、已审批未付款滞留均无提醒
3. **现金无盘点闭环**——账实差异无留痕调账路径
4. **申报与台账导出缺失**——个税扣缴申报表格式、增值税进项台账均靠手工誊录

同步消化第七阶段审查留档的 12 项 Minor。

## 2. 范围

| 模块 | 深度 | 新表 |
|---|---|---|
| 票据台账 | 状态机 + 凭证联动（含背书链） | negotiable_instruments、instrument_endorsements |
| 账期提醒 | 仪表盘提醒卡三类，应用内 | 无（app_settings 配置键） |
| 现金盘点单 | 轻量单 + 差异凭证 | cash_count_sheets、cash_count_denominations |
| 资金日报 | 查询 + Excel 导出 | 无 |
| 个税扣缴申报表 | 按月导出 Excel | 无 |
| 增值税进项台账 | 查询 + Excel 导出 | 无 |
| Minor 打磨 | 第七阶段 12 项留档 | 无 |

## 3. 数据模型

### 3.1 negotiable_instruments（票据）

```sql
CREATE TABLE negotiable_instruments (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  instrument_type TEXT NOT NULL CHECK (instrument_type IN ('bank_acceptance','commercial_acceptance','check')),
  direction TEXT NOT NULL CHECK (direction IN ('received','issued')),
  instrument_no TEXT NOT NULL,              -- 票据号码
  face_amount REAL NOT NULL CHECK (face_amount > 0),
  issue_date TEXT NOT NULL,                 -- 出票日
  due_date TEXT NOT NULL,                  -- 到期日（支票可等于出票日）
  drawer TEXT,                              -- 出票人
  acceptor TEXT,                            -- 承兑人（承兑汇票）
  payee TEXT,                               -- 收款人
  partner_id INTEGER,                       -- 关联往来单位（business_partners）
  fund_account_id INTEGER,                  -- 托收/贴现/兑付入账账户（承兑类）
  counter_account_code TEXT,                -- 对方科目编码（登记时点的贷/借方，缺省按类型）
  status TEXT NOT NULL DEFAULT 'holding' CHECK (status IN ('holding','endorsed_out','discounted','collecting','collected','issued_outstanding','paid','void')),
  voucher_id INTEGER,                       -- 登记凭证
  remark TEXT,
  created_by TEXT, created_at TEXT, updated_at TEXT,
  FOREIGN KEY (partner_id) REFERENCES business_partners(id),
  FOREIGN KEY (fund_account_id) REFERENCES fund_accounts(id)
);
-- 票据号 + 类型唯一（非 void）：UNIQUE index ON (instrument_type, instrument_no) WHERE status != 'void'
```

方向与状态解释：

- **received（收到）**：holding（持有）→ endorsed_out（背书转出）/ discounted（贴现）/ collecting（托收中）→ collected（已到账）
- **issued（开出）**：issued_outstanding（已开出未兑付）→ paid（已兑付）
- 通用终态旁路：void（作废，仅未发生资金流转时允许）
- **支票**（check）不适用 holding 等持有语义：received 支票登记即结算到账（collected，凭证=收款）；issued 支票登记即付款（paid，凭证=付款）。状态机对 check 直接限定终态登记。

### 3.2 instrument_endorsements（背书链）

```sql
CREATE TABLE instrument_endorsements (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  instrument_id INTEGER NOT NULL,
  endorse_order INTEGER NOT NULL,           -- 背书序号（同一票据内递增唯一）
  endorsee TEXT NOT NULL,                   -- 被背书人
  endorse_date TEXT NOT NULL,
  purpose TEXT,                             -- 事由（付货款/转让等）
  amount REAL NOT NULL CHECK (amount > 0),  -- 背书金额（本期约定全额背书，校验 = 票面）
  voucher_id INTEGER NOT NULL,
  created_by TEXT, created_at TEXT,
  FOREIGN KEY (instrument_id) REFERENCES negotiable_instruments(id)
);
```

本期约定背书为全额背书（金额校验等于票面），部分背书留范围外。

### 3.3 cash_count_sheets / cash_count_denominations（现金盘点）

```sql
CREATE TABLE cash_count_sheets (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  count_date TEXT NOT NULL,                 -- 盘点日期
  belong_month TEXT NOT NULL,               -- 冗余月份，月结保护用
  fund_account_id INTEGER NOT NULL,         -- 限 account_type='cash'
  book_balance REAL NOT NULL,               -- 账面余额快照（确认时点）
  counted_amount REAL NOT NULL CHECK (counted_amount >= 0),
  difference REAL NOT NULL,                 -- 实存-账面，自动算
  difference_reason TEXT,                   -- 差异原因（差异≠0 时必填）
  status TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft','confirmed','void')),
  voucher_id INTEGER,                       -- confirmed 且差异≠0 时生成
  remark TEXT,
  created_by TEXT, created_at TEXT, updated_at TEXT,
  FOREIGN KEY (fund_account_id) REFERENCES fund_accounts(id)
);

CREATE TABLE cash_count_denominations (     -- 面额明细（可选子表）
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  sheet_id INTEGER NOT NULL,
  denomination REAL NOT NULL CHECK (denomination > 0),  -- 100/50/20/10/5/1/0.5/0.1
  quantity INTEGER NOT NULL CHECK (quantity >= 0),
  subtotal REAL NOT NULL,                   -- = denomination × quantity，入库校验
  FOREIGN KEY (sheet_id) REFERENCES cash_count_sheets(id) ON DELETE CASCADE
);
```

### 3.4 app_settings 新键

- `reminder_advance_days`：账期提醒提前天数，缺省 7

### 3.5 迁移

`migrate_stage8_schema`：四表 + 唯一索引 + app_settings 键，全部走既有迁移事务框架（单事务、异常回滚、`PRAGMA foreign_key_check`、重复执行幂等）。无存量数据迁移。

## 4. 票据台账：状态机与凭证联动

### 4.1 状态流转命令

| 命令 | 前置状态 | 后置状态 | 凭证 |
|---|---|---|---|
| register（收到承兑登记） | — | holding | 借 1121 应收票据 / 贷 counter_account（缺省 1122 应收账款） |
| register（开出承兑登记） | — | issued_outstanding | 借 counter_account（缺省 2201 应付账款）/ 贷 2202 应付票据 |
| register（收到支票） | — | collected | 借 1002（所选账户挂接科目）/ 贷 counter_account（缺省 1122） |
| register（开出支票） | — | paid | 借 counter_account（缺省 2201）/ 贷 1002 |
| endorse（背书转出） | holding | endorsed_out | 借 counter_account（缺省 2201 应付账款）/ 贷 1121 |
| discount（贴现） | holding | discounted | 借 1002 实收 + 借 6603 财务费用（票面-实收）/ 贷 1121 票面 |
| collect（托收） | holding | collecting | 无（在途不记账） |
| confirm_collection（托收到账确认） | collecting | collected | 借 1002 / 贷 1121 |
| settle_issued（开出承兑兑付） | issued_outstanding | paid | 借 2202 / 贷 1002 |
| void | holding / issued_outstanding（未流转） | void | 作废登记凭证 |

约束：

- 所有写命令 `require_current_operator` 署名、`ensure_month_open`（按票据登记月与操作月双查，与第七阶段冲正双月口径一致）
- 贴现实收不得大于票面；财务费用 = 票面 − 实收（容差 0.005），等于 0 时免财务费用腿
- 托收中（collecting）不可背书/贴现；endorsed_out/discounted/collected/paid 为终态
- 已流转票据纠错走**红字冲正**（复用第七阶段口径：原凭证 active + 反向凭证并存，净影响 0，approval_events 留痕）；已月结月份须先反月结
- 背书金额 = 票面（全额背书校验）

### 4.2 与资金账户联动

贴现/托收到账/兑付的借方或贷方资金科目必须带 `fund_account_id`（走 `ensure_fund_voucher_lines`，资金科目必带、对方科目必空）；票据登记/背书不涉及资金账户。

## 5. 账期提醒

新命令 `get_dashboard_reminders`（只读）返回三类：

| 类别 | 数据源 | 规则 |
|---|---|---|
| 借款到期 | fund_documents（type=advance，status=settled，due_date 非空） | 距到期 ≤ N 天或已逾期，展示未清余额（借款额-累计核销） |
| 票据到期 | negotiable_instruments（status ∈ holding/collecting/issued_outstanding） | 距到期 ≤ N 天或已逾期 |
| 滞留应付 | 报销单（已审批未付款）+ 资金单（已审批未结算 payment 类） | 按审批时间算滞留天数，≥ N 天提示 |

N 取 `app_settings.reminder_advance_days`（缺省 7）。仪表盘新增提醒卡（分组 + 天数 + 金额，金额走 SensitiveText 脱敏）。不做系统通知/推送。

## 6. 现金盘点

流程：新建（选 cash 账户 + 日期）→ 账面余额自动快照 → 录实存（快速模式直接填总额；或面额明细自动合计）→ 差异自动算 → 差异≠0 填原因（必填）→ 确认：

- 差异 < 0（盘亏）：借 1901 待处理财产损溢 / 贷 1001
- 差异 > 0（盘盈）：借 1001 / 贷 1901
- 差异 = 0：不生成凭证，仅留盘点记录

约束：draft 可改可作废；confirmed 不可改，作废须走红字冲正（差异凭证场景）；账面余额快照取确认时点账户余额（voucher_lines 派生，与日记账同源）；`ensure_month_open(belong_month)`；面额明细合计与 counted_amount 必须一致才允许确认。

## 7. 资金日报

命令 `get_fund_daily_report(date)`（只读）+ `export_fund_daily_report(date, path)`：

- 各账户行：期初余额（前日期末）、当日收入、当日支出、期末余额
- 当日收支明细（按账户分组，含凭证号与摘要）
- 近 7 日趋势（各账户期末余额序列）
- 数据源：voucher_lines 资金分录按日聚合，与资金日记账同源同口径
- 导出 `资金日报_YYYYMMDD.xlsx`（账户汇总 + 明细两张 sheet）
- 入口：资金日记账页「导出日报」按钮；导出前校验敏感数据解锁状态（与日记账导出同门禁）

## 8. 个税扣缴申报表导出

命令 `export_tax_withholding_declaration(month, path)`：

- 数据源：salary_results（仅 status='已锁定' 月份；未锁定月份导出被拒）
- 每员工一行：姓名、身份证号（employees.id_card）、收入额（应发合计）、基本养老保险/基本医疗保险/失业保险（个人三险拆列：有社保台账的按台账各险种个人费率拆 salary_results.social_insurance；无台账的按全局规则页各险种个人比例拆，比例之和不等于 100% 时整列退回为合并展示并留备注）、住房公积金（个人）、减除费用（5000/月，取当月规则）、专项附加扣除（special_deduction）、累计应纳税所得额、当期预扣率、速算扣除数、累计已预扣税额
- 列头对齐电子税务局申报录入页措辞；表尾合计行
- Excel 走 excel.rs 新函数；入口：工资计算页（个税年度汇总旁）+ 导出中心
- 仅导出格式，不做在线申报、不集成税务系统

## 9. 增值税进项台账

命令 `get_input_tax_ledger(from_month, to_month)`（只读）+ 导出：

- 行：发票代码、发票号码、开票日期、销方名称、销方税号、不含税金额、税额、价税合计、费用类型、关联报销单号（reimbursement_claim_invoices 反查）、所属月份
- 汇总：区间税额合计、价税合计；按月小计
- 排除 status='void'
- 页面明示「发票登记 ≠ 进项认证，认证状态以税务系统为准」
- 入口：票据报销组「进项台账」页；导出 `进项台账_YYYYMM-YYYYMM.xlsx`

## 10. 导航变更

| 菜单组 | 变更 |
|---|---|
| 资金出纳 | +「票据台账」「现金盘点」（变 8 项） |
| 票据报销 | +「进项台账」（变 3 项） |
| 资金日记账 | 页内加「导出日报」按钮 |
| 工资计算 | 页内加「扣缴申报表」按钮 |

## 11. Minor 打磨清单（8C 消化，以 stage7 memory 留档为准）

1. FundDocuments 编辑核销草稿透传 settlement_mode/due_date
2. bank_manual 账户下拉收窄为 bank/third_party
3. bank_manual 幂等判断 sql.contains 子串匹配改结构化判断
4. 跨月冲正凭证口径显式化（注释/测试）
5. attachment_disk_stats O(n²) 优化
6. 预算 expense_type 下拉化
7. 借款台账统计卡口径说明（含未发放借款）
8. 报销 save 报错中文化；mock 状态机命令 default 不再恒 true
9. 存量单 counter_account_code 贷方回落 1221 边角处理
10. 同分平局断言语义明确化
11. 旧匹配伪未达项提示优化；legacy 取消后 allocation 残留两步恢复提示
12. unmatched_paid_batch_count 月结口径注释（已切 allocation，补文档说明）

## 12. 测试策略

- 后端单元测试（open_in_memory，基线 255）：
  - 票据：登记/背书/贴现/托收/兑付各流转的凭证借贷科目断言；贴现实收>票面拒绝；财务费用=票面-实收（0 时免腿）；全额背书校验；托收中禁背书贴现；冲正净影响 0；月结保护（登记月+操作月）；票据号唯一（void 除外）；支票限定终态登记
  - 盘点：面额合计=实存校验；差异凭证双向；差异 0 免凭证；差异≠0 原因必填；confirmed 冲正作废；月结保护
  - 提醒：提前天数边界（当天/前 N 天/逾期）、滞留天数计算、配置读取缺省
  - 日报：期初+收支=期末勾稽；空日/跨月边界
  - 个税导出：仅锁定月份门禁；列合计断言
  - 进项台账：void 排除、区间小计
  - 迁移：四表幂等、FK 检查
- 前端：`npx tsc -b`、lint、build 全过
- Windows exe 手工验收：并入既有挂账清单，新增票据全流程（登记→背书/贴现/托收→冲正）、盘点差异凭证、提醒卡展示、三导出

## 13. 范围外

- 部分背书、票据池融资、电子商业汇票系统（ECDS）对接
- 进项认证状态登记（认证以税务系统为准）
- 在线个税申报、税务系统对接
- 系统通知/消息推送（沿袭 stage7 范围外）
- 多用户、RBAC、云同步（沿袭既有范围外）

## 14. 批次任务骨架

- **8A（Task 1-5）**：DDL 迁移与模型 → 票据状态机与凭证领域层 → 命令与票据台账页 → 账期提醒（命令+仪表盘卡）→ 批次收尾
- **8B（Task 6-8）**：现金盘点（表+凭证+页面）→ 资金日报（查询+导出+入口）→ 批次收尾
- **8C（Task 9-12）**：个税扣缴申报表导出 → 进项台账 → Minor 12 项 → 全量回归+文档四件套（CLAUDE.md/memory/progress/手册）+ 发版评估
