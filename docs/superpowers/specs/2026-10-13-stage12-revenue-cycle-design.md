# 第十二阶段设计（草案）：收入侧闭环——销项登记、应收台账与回款核销

日期：2026-10-13
状态：草案（待审阅）
基线：v0.9.0 + 第十/十一阶段（后端 380 测试）

## 1. 背景与目标

产品至今覆盖的基本是"花钱"侧：进项发票、报销、付款、借款。收入侧只有收款单冲应收（counter_account 1122）和票据台账（承兑），缺三块：**销项发票登记、应收台账与账龄、回款核销**。对小微企业老板，"谁欠我钱"比"我欠谁钱"更上心——这是产品功能版图上最后一个大空白。

第十二阶段目标：补齐"开票登记 → 应收挂账 → 回款核销 → 账龄监控"闭环，**复用既有底座**（发票 OCR/去重/加密归档、往来单位、收款单与资金辅助核算、借款核销模型、银行流水核销、账期提醒卡），零新增外联。

底座盘点（已核实）：
- `invoices` 表已有 buyer/seller 双侧字段，无方向概念
- `business_partners` 已支持 customer 类型（demo 已挂 1122）
- 收款单（receipt + partner_id + counter_account_code）已能生成 借银行/贷应收 凭证
- 借款核销（advance_settlement_links）提供了"部分核销 + 取消恢复"的成熟模型
- 进项台账（db.rs:7327 / excel.rs:3053）提供台账查询+导出的镜像范式

## 2. 范围

| 模块 | 深度 | 新表/列 |
|---|---|---|
| 销项发票登记 | 复用 invoices + direction 列；OCR/手工双入口 | invoices.direction |
| 进项侧门禁回扫 | 既有进项/报销流程排除 output 方向 | 无 |
| 销项凭证 | 登记即生成 借应收/贷收入+销项税 凭证 | 无（accounting 挂接） |
| 应收台账 | 按客户聚合未核销余额 + 回款进度 | ar_receipt_allocations |
| 回款核销 | 收款单 ↔ 销项发票 多对多部分核销 + 取消 | 同上 |
| 账龄与提醒 | 30/60/90/180 分桶 + 提醒卡/OS 通知接入 | 无 |
| 导出 | 销项台账 Excel + 应收账龄表 | 无 |

## 3. 数据模型

### 3.1 invoices.direction

```sql
ensure_column(c, "invoices", "direction", "TEXT NOT NULL DEFAULT 'input'")
-- CHECK 约束迁移期不加（ALTER 无法加 CHECK），写入层校验 IN ('input','output')
```

- 存量行全部 'input'（默认值兜底）
- **去重索引不变**：`(COALESCE(invoice_code,''), invoice_number)` 全局唯一——同一张票在买方是进项、在卖方是销项，但我们只登记本公司销项，与进项不冲突；若同号冲突按现有报错处理
- **门禁回扫**（阻断项，逐处核对）：报销单关联发票、发票费用类型、进项台账、发票费用凭证（invoice_expense）、发票管理页查询——全部加 `direction='input'` 过滤，防止销项票混入费用流程

### 3.2 ar_receipt_allocations

```sql
CREATE TABLE IF NOT EXISTS ar_receipt_allocations (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  invoice_id INTEGER NOT NULL REFERENCES invoices(id),
  receipt_id INTEGER NOT NULL REFERENCES fund_documents(id),
  allocated_amount REAL NOT NULL CHECK (allocated_amount > 0),
  status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','cancelled')),
  remark TEXT,
  created_by TEXT,
  created_at TEXT NOT NULL,
  cancelled_at TEXT,
  cancel_reason TEXT
);
CREATE INDEX idx_ar_alloc_invoice ON ar_receipt_allocations(invoice_id) WHERE status='active';
CREATE INDEX idx_ar_alloc_receipt ON ar_receipt_allocations(receipt_id) WHERE status='active';
```

镜像 advance_settlement_links 语义：active 合计为已核销额；取消核销恢复余额并留痕（月结保护：已月结月份禁止取消）。

## 4. 功能设计

### 4.1 销项发票登记（12A）

- 入口：发票管理页新增「进项/销项」Tab（OCR 识别与手工录入共用，页面按 Tab 切换字段口径）
- 销项口径：seller=本公司（自动带 app_settings.company_name/taxpayer_id，可改）、buyer=客户；expense_type/employee/belong_month 语义照旧（belong_month=收入归属月）
- OCR：百度 vat_invoice 对销项票同样适用，返回字段映射时 buyer/seller 按方向解释；全电票 PDF 直传与图片加密归档复用
- **凭证**：登记（非 void）自动生成——借 1122 应收账款（客户辅助=partner）/ 贷 6001 主营业务收入 amount / 贷 2221 应交税费 tax_amount；作废联动红字（复用既有 voucher void 联动机制）；红票（total<0 或备注注明）允许负数行冲回
- 客户字段：新增可选 partner_id 关联（buyer_name 自由文本兼容手工登记，partner_id 缺省按名称模糊匹配提示补挂）

### 4.2 应收台账与回款核销（12B）

- 新页「应收台账」（资金出纳组）：按发票行展示——票号/客户/开票日/归属月/票面/已核销/未核销余额/账龄/状态（未核销/部分核销/已核销）
- 核销入口：收款单（status='settled' 且 partner 匹配或手工指定）→ 勾选销项发票逐张分摊（默认按开票日 FIFO 预填）；部分核销支持；核销不生成新凭证（收款单已按 1122 入账，核销只做台账勾稽）
- 取消核销：留痕恢复（cancelled + 原因必填）；月结保护——invoice 或 receipt 归属月已月结则拒绝
- 月结检查新增：归属月存在未核销且账龄>90 天的销项 → warning（不阻断）

### 4.3 账龄、提醒与导出（12C）

- 账龄分桶：开票日起 30/60/90/180/超 180（基准日可调，借款台账同款）
- 账期提醒卡新增类别 `ar_overdue`：账龄>90 天未核销销项（复用 ReminderItem 结构与脱敏口径）；OS 通知（第十一阶段 collect_due_items）同步接入
- 导出：销项台账（区间+月度小计，镜像进项台账列结构：票号/客户/金额/税额/价税合计/核销进度）、应收账龄表（客户 × 分桶矩阵）

## 5. 错误处理与边界

- direction 非法值写入拒绝；存量数据迁移后抽检 buyer=本公司的进项票（提示人工确认是否录错方向，不自动改）
- 核销金额>未核销余额拒绝；收款单作废/冲正联动检查 active 核销（有则拒绝并提示先取消核销）
- 票据台账（承兑）与应收台账的关系：承兑收到时冲应收（背书/贴现/托收已在票据台账），本阶段**不做**自动联动核销，仅提供手工核销入口（票据结算后可手工勾稽对应销项）
- 月结保护：已月结月份的销项登记/核销/取消均拦截

## 6. 测试策略

- 单元测试（内存库）：direction 门禁回扫（报销/进项台账/费用凭证不混入销项）、销项凭证借贷平衡与红字联动、核销部分/超额/取消恢复、月结拦截、账龄分桶边界（29/30/31、89/90/91、179/180）、提醒卡 ar_overdue 命中、导出门禁
- 全量回归：cargo test --lib / npx tsc -b / npm run lint
- Windows 手工验收挂账：销项 OCR 录入→凭证核对→收款核销→账龄提醒→导出全流程

## 7. 交付批次

- **12A**（Task 1-3）：direction 迁移与门禁回扫 → 销项登记（页 Tab + OCR 口径 + 凭证）
- **12B**（Task 4-6）：ar_receipt_allocations 与核销命令 → 应收台账页
- **12C**（Task 7-9）：账龄/提醒接入 → 两导出 → 月结检查
- 收尾：导航核对、全量回归、文档四件套

## 8. 不做范围（未来增强池）

- 增值税申报表生成（销项-进项勾稽需税会判断，超出定位）
- 收入确认的税会差异调整（时点/履约进度）
- 承兑票据与应收台账自动联动核销
- 客户信用额度、催收函生成
- 多账套（独立大阶段）
