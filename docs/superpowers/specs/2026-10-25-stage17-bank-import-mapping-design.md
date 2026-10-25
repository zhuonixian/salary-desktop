# 第十七阶段设计：银行流水导入增强——列识别诊断与手工映射

日期：2026-10-25
状态：待审阅
基线：第十六阶段已交付（后端 414 测试）
路线图：`docs/superpowers/plans/2026-10-25-roadmap-v0.11-iterations.md`（迭代 3，来源 stage3 增强池"复杂银行模板字段映射预览"）

## 1. 背景与问题

现行导入（spec 4.8 三步：选账户 → 解析预览 → 确认入库）的列识别是**硬编码别名表**，存在两个实际痛点：

1. **静默失败**：银行导出列名不在别名表内（如"交易金额"单列、"贷方发生额"变体）时，行被静默跳过（日期缺失或收支均为 0 → None），用户只看到"0 行"却不知原因。
2. **无法自救**：未识别列没有任何手工指定入口，换家银行导出就要改代码。

## 2. 交付内容

### 2.1 列识别诊断（后端）

- `BankColumnHint { field, column, source }`：八个标准字段（date/summary/income/expense/single_amount/counterparty_name/counterparty_account/balance）的识别结果——column=命中的表头名，source ∈ auto（别名命中）/ manual（手工映射命中）/ manual_missing（手工映射的列名不存在）/ missing（未识别）
- `BankImportPreview` 新增 `column_hints`；预览前端据此展示识别状态标签

### 2.2 手工映射（全链路）

- `BankColumnMapping { date, summary, income, expense, single_amount, counterparty_name, counterparty_account, balance }`（全 Option，serde 前端直传）
- `preview_bank_transaction_import_with_mapping` / `read_bank_transactions_file_content_with_mapping` 新变体；**旧签名保留为 None 委托**（既有调用与测试零改动）
- `import_bank_transactions_file` 命令新增可选 `column_mapping` 参数（与预览同一映射解析，杜绝"预览一套、入库另一套"）

### 2.3 解析能力扩充

- 别名扩充：日期 +"交易时间"；收入 +"收入金额"/"贷方发生额"；支出 +"支出金额"/"借方发生额"；余额 +"账户余额"
- **单列金额支持**：收入与支出列均未识别且"交易金额/发生额/金额/交易额/收支金额"列存在时，正值→收入、负值→支出（取绝对值）；hints 中以 single_amount 字段呈现
- 手工映射优先级高于别名（manual 覆盖 auto）

### 2.4 前端（导入 Modal）

- 预览区顶部新增"列识别"标签行：每字段一枚 Tag（绿=已识别列名，灰=可选未识别，红=关键字段未识别）
- **关键字段（date 与 income/expense/single_amount 三者之一）缺失时**展示映射编辑器：每缺失字段一个 Select（选项=文件表头），填毕点「重新解析」带映射重跑预览；确认导入沿用同一映射
- 换文件/换账户时清空映射状态（预览本来就会重置）

## 3. 测试策略

- 别名扩充命中（贷方发生额/交易时间/账户余额）
- 单列金额：正负两行、与收入列并存时优先双列
- 手工映射：怪列名（"A 列"）经映射解析成功；映射到不存在的列名 → manual_missing 不炸
- hints：auto/manual/missing 三态断言
- 旧签名委托回归：既有 CSV 带引号摘要测试不变
- 回归：414 存量全绿

## 4. 不做范围（未来增强池）

- 多流水对多批次聚合匹配、明细级匹配（工作台多对多核销已覆盖大部分场景，等用户反馈）
- 映射配置持久化/银行预设档案（v1 每次手工指定即可）
- 表头行自动探测（跳过前置杂行的启发式，风险大于收益）
