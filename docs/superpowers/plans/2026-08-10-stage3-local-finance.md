# 第三阶段计划：本地轻量财务管理能力

## 1. 背景与定位

当前项目是面向出纳本地使用的工资核算桌面工具，技术栈为 Tauri 2 + React + SQLite，最终交付 Windows exe / 安装包。已有能力包括员工管理、考勤 OCR、工资核算、发票 OCR 去重、报销管理、月结工作台、财务分析、Excel 导入导出与操作日志。

第三阶段不把产品扩展成完整 ERP，也不引入云端协作、复杂权限或在线审批。目标是在本地单机、轻量易用、可备份可恢复的前提下，把工资、发票、报销、付款、月结形成可长期使用的财务辅助闭环。

## 2. 阶段目标

1. 补齐本地数据安全能力：备份、恢复、数据目录打开、数据库体检、导出本地资料包。
2. 将“月结工作台”升级为正式月结：检查、快照、锁账、反月结、月结包导出。
3. 统一工资代发与报销付款，形成付款批次管理。
4. 支持银行流水导入与本地匹配，减少手工核对成本。
5. 增加费用预算与异常提醒，强化财务分析的可操作性。
6. 为后续发票类型扩展、工资规则版本化、凭证草稿导出预留边界。

## 3. 功能优先级

### P0：本地数据安全中心

最小范围：
- 新增“数据安全”页面，放在“输出审计”或“系统设置”分组。
- 展示当前数据库路径、应用数据目录、发票归档目录。
- 支持一键备份 `salary.db` 到用户选择目录，文件名包含时间戳。
- 支持从备份文件恢复数据库，恢复前自动创建当前库的安全备份。
- 支持打开数据目录。
- 支持数据库体检：文件是否存在、大小、主要表记录数、最近操作时间。

后端建议：
- 新增 `src-tauri/src/backup.rs` 或放入独立 `data_safety.rs`。
- 新增 Tauri 命令：
  - `get_data_safety_status()`
  - `backup_database(target_dir: String)`
  - `restore_database(backup_path: String)`
  - `open_app_data_dir()`
  - `export_local_data_package(target_dir: String)`
- 备份/恢复需要短事务和文件级保护；恢复时不得覆盖正在写入中的连接。

测试：
- Rust 单元测试覆盖备份文件名、非法路径、恢复前安全备份。
- 手工验证 Windows exe 下备份、恢复、目录打开。

### P0：正式月结与反月结

最小范围：
- 新增 `month_closes` 表，记录月份、状态、摘要 JSON、检查结果 JSON、closed_at、closed_by、remark。
- 月结前复用现有检查项；存在 blocking 时禁止关账。
- 关账后禁止修改该月关键业务数据：工资结果、考勤、发票、报销单、付款状态。
- 支持反月结，必须填写原因并写操作日志。
- 月结包导出：工资明细、银行代发、报销清单、发票清单、财务分析、检查结果。

后端建议：
- `db.rs` 增加 `month_closes` 初始化和 CRUD。
- `commands.rs` 增加：
  - `get_month_close_status(month)`
  - `close_month(month, remark)`
  - `reopen_month(month, reason)`
  - `export_month_close_package(month, dir)`
- 在已有 update/delete/save 命令入口增加月结锁校验。

测试：
- blocking 检查未通过时不能关账。
- 关账后禁止修改该月工资、考勤、发票、报销。
- 反月结后允许修改，并记录操作日志。
- 导出月结包包含预期文件。

### P1：付款批次管理

最小范围：
- 新增付款批次，用于统一管理工资代发与报销付款。
- 一个批次包含付款类型、月份、总金额、付款人数/单数、状态、生成时间、付款日期、备注。
- 支持从已锁定工资结果生成工资付款批次。
- 支持从已审批未付款报销单生成报销付款批次。
- 支持导出银行付款 Excel，标记已付款。

建议表：
- `payment_batches`
- `payment_items`

建议状态：
- `draft`
- `exported`
- `paid`
- `void`

关键规则：
- 已付款批次不可编辑，只能作废或补备注。
- 同一工资结果或报销单不能重复进入未作废批次。
- 月结后禁止新增/修改该月付款批次。

### P1：银行流水导入与匹配

最小范围：
- 支持 CSV / Excel 导入银行流水。
- 保存交易日期、摘要、对方户名、账号、收入、支出、余额、原始行 JSON。
- 按金额、日期、姓名、银行账号、付款批次单号匹配工资/报销付款。
- 提供“待匹配 / 已匹配 / 忽略”状态。

建议表：
- `bank_transactions`
- `bank_transaction_matches`

本地易用性：
- 第一版不做银行网银直连，只做文件导入。
- 提供导入模板与字段映射预览。

### P2：预算与异常提醒

最小范围：
- 按月份、部门、费用类型维护预算。
- 财务分析页增加预算执行率、超预算提醒。
- 月结工作台增加异常项：未分类发票、超预算费用、异常考勤、重复金额、未匹配付款。

建议表：
- `budgets`
- `alert_rules`
- `alert_items`

### P2：后续增强池

可排队但不作为第三阶段第一批开发：
- 交通票、火车票、机票、出租车票、餐饮定额发票识别。
- 图片 pHash 去重。
- 发票验真。
- 工资规则按月份版本化。
- 凭证草稿导出。
- 报表自定义模板。

## 4. 不做范围

第三阶段不做：
- 多用户权限、在线审批、云同步。
- 完整总账、科目余额、资产负债表、利润表。
- 默认内置大型本地 OCR / AI 模型。
- 银行接口直连。
- 税务平台自动申报。

## 5. 推荐迭代顺序

### 3.1 数据安全中心

原因：本地 exe 软件首先要保证数据可控、可迁移、可恢复。

交付：
- 页面：`src/pages/DataSafety.tsx`
- 后端：`src-tauri/src/data_safety.rs`
- API：`src/api/index.ts`
- 类型：`src/types/index.ts`
- 菜单：`src/App.tsx`
- 测试：Rust 单元测试 + 前端类型检查 + 本地手工验证

### 3.2 正式月结

原因：现在已有月结检查，但缺少快照和锁账，长期管理风险较高。

交付：
- 表：`month_closes`
- 命令：关账、反关账、查询状态、导出月结包
- 页面：增强 `src/pages/MonthClose.tsx`
- 保护：工资、考勤、发票、报销、付款写操作统一检查月结状态

### 3.3 付款批次

原因：工资代发与报销付款需要统一管理，便于和银行流水匹配。

交付：
- 页面：`src/pages/Payments.tsx`
- 表：`payment_batches` / `payment_items`
- 导出：复用 `excel.rs`，扩展银行付款模板

### 3.4 银行流水匹配

原因：付款完成后需要本地核对闭环。

交付：
- 页面：`src/pages/BankTransactions.tsx`
- 表：`bank_transactions` / `bank_transaction_matches`
- 导入：Excel/CSV
- 匹配：规则优先，人工确认兜底

### 3.5 预算与异常

原因：把财务分析从“看数”升级为“发现问题”。

交付：
- 页面：预算配置或集成到财务分析页
- 月结检查项扩展
- 异常列表和导出

## 6. Subagent 协作方案

每个 subagent 必须围绕独立写入范围工作，避免互相覆盖。

建议角色：
- `planner-db`：只负责 schema、Rust model、db.rs 测试。
- `planner-command`：只负责 commands.rs、lib.rs 注册、API 合约。
- `planner-ui`：只负责页面、菜单、类型、前端 API。
- `planner-test`：只负责测试清单、回归脚本、验收记录。

协作规则：
- 每个 subagent 开始前必须读取 `CLAUDE.md` 和本计划。
- 每个 subagent 输出必须包含：已改文件、未改文件、测试结果、遗留风险。
- 同一轮并行任务不得写同一个文件；如果不可避免，由主 agent 合并。
- 主 agent 负责最终集成、格式化、测试、commit、push。

## 7. 进度同步文件

第三阶段开发过程中维护：

- `.claude/memory/stage3-local-finance.md`：长期上下文摘要。
- `docs/superpowers/plans/2026-08-10-stage3-local-finance.md`：完整计划。
- `docs/superpowers/plans/2026-08-10-stage3-progress.md`：每次开发进度、测试、提交记录。

每次开发结束必须追加：
- 日期时间。
- 本轮目标。
- 完成项。
- 修改文件。
- 已跑测试。
- 未完成项。
- 下轮建议入口。

## 8. 回归验证门槛

每个功能批次至少运行：

```bash
npx tsc --noEmit
npm run build
cd src-tauri && cargo test --lib
```

涉及 Rust 新模块时增加：

```bash
cd src-tauri && cargo fmt --check
cd src-tauri && cargo check
```

涉及 exe 或 Tauri 能力时增加：

```bash
npm run tauri build
```

手工验收：
- Windows exe 启动。
- 备份恢复不丢数据。
- 月结后关键修改被拦截。
- 反月结后可重新修改。
- Excel 导入导出文件可打开。

## 9. 提测与推送

提测前检查：
- `git status --short` 只包含本轮相关文件。
- 文档、代码、测试同步更新。
- 操作日志覆盖关键动作。
- 错误提示为中文且可指导用户处理。

提交建议：
- `docs: 添加第三阶段本地财务管理计划`
- `feat(data): 新增本地数据安全中心`
- `feat(close): 新增正式月结与反月结`
- `feat(payment): 新增付款批次管理`
- `feat(bank): 新增银行流水导入匹配`

推送：

```bash
git push origin master
```

如需发布 exe：

```bash
git tag -a v0.3.0 -m "feat: 第三阶段本地财务管理能力"
git push origin v0.3.0
```

## 10. Subagent 规划补充

### 数据安全中心

规划结论：
- 首版不新增 SQLite 表，复用 `operation_logs` 记录备份、恢复、体检、压缩操作。
- 备份历史不建议存入数据库表；数据库恢复后表内历史会随备份回滚，容易造成用户误解。
- WAL 模式下不直接裸拷贝 `salary.db`，使用 checkpoint + `VACUUM INTO` 生成一致性数据库副本。
- 发票归档目录必须和数据库一起备份，否则 `invoices.image_path` 会断链。
- 恢复前必须自动生成保护备份；恢复后建议重启应用再继续录入。

第一批落地文件：
- `src-tauri/src/data_safety.rs`
- `src-tauri/src/models.rs`
- `src-tauri/src/commands.rs`
- `src-tauri/src/lib.rs`
- `src/types/index.ts`
- `src/api/index.ts`
- `src/pages/DataSafety.tsx`
- `src/App.tsx`

### 正式月结与付款批次

规划结论：
- 下一批建议新增 `month_closes` 表，记录月结状态、摘要 JSON、检查 JSON、关账/反关账时间和原因。
- 再下一批建议新增 `payment_batches` 与 `payment_batch_items`，统一工资代发和报销付款。
- 月结状态流为 `open/reopened -> closed`，`closed -> reopened`；不物理删除月结记录。
- 月结后禁止修改该月工资结果、考勤、发票、报销和付款批次；反月结只解除编辑锁，不回滚付款状态。
- 当前项目没有迁移系统，给旧库补字段时需要显式 `ALTER TABLE` 兼容。
