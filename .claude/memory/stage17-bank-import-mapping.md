# 第十七阶段：银行流水导入增强——列识别诊断与手工映射（已交付）

spec：`docs/superpowers/specs/2026-10-25-stage17-bank-import-mapping-design.md`
路线图：`docs/superpowers/plans/2026-10-25-roadmap-v0.11-iterations.md`（迭代 3，来源 stage3 增强池"复杂银行模板字段映射预览"）

## 模块地图

- `excel.rs`：八字段别名常量（扩充 交易时间/收入金额/贷方发生额/支出金额/借方发生额/账户余额）+ 单列金额别名（交易金额/发生额/金额/交易额/收支金额）；`BankColumnMapping`（手工映射，优先于别名）；`resolve_bank_columns` → `BankColumnHint { field, column, source }`（auto/manual/manual_missing/missing）；`read_bank_transactions_file_content_with_mapping`（旧签名 None 委托，既有调用零改动）
- `db.rs`：`preview_bank_transaction_import_with_mapping`（旧签名委托）；`BankImportPreview` + column_hints
- `commands.rs`：preview/import 命令增加可选 `column_mapping`（同一映射贯穿预览与入库）
- 前端：导入 Modal 识别诊断标签行（绿=auto/蓝=manual/橙=映射列不存在/红=关键字段缺失）+ 关键字段缺失时的映射编辑器与「重新解析」；确认导入沿用映射；0 行时禁用确认按钮

## 关键设计

- 单列金额兜底：收入与支出列均未识别且单列金额列存在 → 正=收入、负=支出（取绝对值）；零金额行照旧跳过
- manual_missing（映射到不存在的列名）不报错不 panic，仅橙色提示——用户可改选
- 关键字段定义：date，或 income/expense/single_amount 全缺；缺失才展示映射编辑器，避免常态打扰
- 手工映射不持久化（v1 每次指定；银行预设档案留增强池）

## 已知边界

- 多流水对多批次聚合匹配、明细级匹配、表头行自动探测：spec 第 5 节不做范围
- Excel 前置杂行（标题行在表头之前）仍需手工清理后导入
