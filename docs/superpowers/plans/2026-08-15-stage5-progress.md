# 第五阶段进度同步

本文件用于第五阶段财务专业功能（科目表与三大报表）开发接力。每轮开发结束必须追加记录，避免上下文压缩、subagent 分工或新会话接手造成信息丢失。

## 当前基线

- 分支：`master`
- 阶段计划：`docs/superpowers/plans/2026-08-15-stage5-accounting-reports.md`
- spec：`docs/superpowers/specs/2026-08-15-stage5-accounting-reports-design.md`
- 长期摘要：`.claude/memory/stage5-accounting.md`
- 目标交付：科目表、自动派生记账凭证、资产负债表/利润表/现金流量表、Excel 导出。

## 协作规则

- 开发前先读 `CLAUDE.md`、本文件、阶段计划。
- subagent 只处理明确且互不重叠的文件范围。
- 主 agent 负责合并、测试、commit、push。
- 每轮结束补充"本轮记录"。

## 本轮记录模板

```md
### YYYY-MM-DD HH:mm

- 目标：
- 完成：
- 修改文件：
- 测试：
- 未完成：
- 下轮入口：
- 提交：
```

## 记录

### 2026-08-15 批次一：科目表与期初余额（Task 1-4）

- 目标：落地 5 张表 DDL、科目/期初/映射 CRUD 与 Tauri 命令、科目表页面与"财务管理"菜单。
- 完成：
  - `gl_accounts` / `vouchers` / `voucher_lines` / `opening_balances` / `account_mappings` 5 表 DDL + 62 预置科目 seed（断言 >=62）。
  - `accounting.rs` 8 个 CRUD 函数 + 4 测试：期初保存 HashSet 查重 + unchecked_transaction；停用校验只统计 active 凭证。
  - 8 个 Tauri 命令注册（科目/期初/映射），92 测试通过。
  - 科目表页面（五大类分组、启用/停用、新增、期初余额弹窗实时差额校验）；期初弹窗保留已停用科目的已有余额（只读行）。
- 修改文件：`src-tauri/src/db.rs`、`src-tauri/src/accounting.rs`（新）、`src-tauri/src/models.rs`、`src-tauri/src/commands.rs`、`src-tauri/src/lib.rs`、`src/types/index.ts`、`src/api/index.ts`、`src/pages/ChartOfAccounts.tsx`（新）、`src/App.tsx`。
- 测试：cargo test --lib 92 通过；tsc/lint/build 通过。
- 未完成：mock 模式缺 8 个新命令 case（Task 14 补）。
- 提交：`7f64c4c..99676f4`（Task 1: b61fe22、Task 2: 8db363e/d4eb152、Task 3: d4eb152、Task 4: 3d1cc75/510b998/99676f4）。

### 2026-08-15 批次二：凭证引擎与业务挂接（Task 5-9）

- 目标：凭证生成/作废/查询核心，六类业务事件挂接，银行流水手工凭证与月结检查。
- 完成：
  - 凭证核心 4 函数 + 5 结构体（93 测试）；insert_voucher 事务加固（is_autocommit 门控）。
  - 工资计提凭证 + 解锁联动；save/update_salary_result 补 locked guard。
  - flaky 测试修复：batch_no 纳秒时间戳 + 进程相关 4 位 hex 后缀（无重试循环）。
  - 付款批次凭证（salary_payment / reimbursement_payment）+ 作废联动。
  - 报销计提 + 发票费用凭证 + 补偿联动：soft_delete_invoice 重建 approved claim 计提；amount+tax≠total 跳过入账不阻断保存。
  - bank_manual 凭证 + 月结"记账凭证平衡"检查（0.005 容差）+ 匹配防双贷记：confirm 拒绝已入账流水匹配、auto_match 排除、重复生成中文报错。
- 修改文件：`src-tauri/src/accounting.rs`、`src-tauri/src/commands.rs`、`src-tauri/src/db.rs`、`src-tauri/src/models.rs`、`src-tauri/src/lib.rs`、`src-tauri/src/security_commands.rs`（fmt 清理）。
- 测试：cargo test --lib 110 通过。
- 未完成：无（批次收尾）。
- 提交：`99676f4..d1f6e65`（Task 5: d3e2460、Task 6: 02cb2ee、Task 7: 40b41dc/82fd1e0/ecc36c8、Task 8: 97a261c/ccab633、Task 9: 17843f4/d1f6e65）。

### 2026-08-15 批次三：三大报表计算引擎与导出（Task 10-11）

- 目标：科目余额、资产负债表、利润表、现金流量表计算与 Excel 导出。
- 完成：
  - 报表引擎（26 accounting 测试）：comparative 改 opening_raw（年初口径）；other_pl 兜底行 + cost_accounts 资产行（自定义科目不再消失/不平）。
  - 4 报表命令 + 3 Excel 导出 + cost_accounts comparative 修正（Σopening_raw），119 测试。
- 修改文件：`src-tauri/src/accounting.rs`、`src-tauri/src/commands.rs`、`src-tauri/src/excel.rs`、`src-tauri/src/lib.rs`、`src-tauri/src/models.rs`。
- 测试：cargo test --lib 119 通过。
- 未完成：现金流量表本年累计列恒 0（引擎无累计计算，brief 表头强制，spec 范围外）。
- 提交：`d1f6e65..0ecffa2`（Task 10: d00b486/8def7c5、Task 11: 0ecffa2）。

### 2026-08-15 批次四：前端页面与全量回归（Task 12-14）

- 目标：记账凭证页、财务报表页、银行流水生成凭证入口、操作日志映射、全量回归与第五阶段文档。
- 完成：
  - 记账凭证页面（`/vouchers`）：按月筛选、来源类型中文 Tag、分录明细抽屉、金额脱敏（Task 12，commit 0ecffa2..5d54951）。
  - 财务报表页面（`/reports`）：三 Tab、未归类现金流量提示、独立导出 Excel、other_pl 行序渲染时排到利润总额与净利润之间（Task 13，commit 5d54951..18a00b0）。
  - 银行流水页未匹配且未忽略行"生成凭证"入口：方向提示（支出选借方/收入选贷方，1002 固定另一侧）+ 科目 Select（is_active 过滤）+ 摘要 + message.success(凭证号) 后刷新。
  - api mock 模式补 create_bank_manual_voucher case；操作日志补 save_opening_balances/create_bank_manual_voucher/export_financial_report 中文映射。
  - FinancialReports renderAmount 修复：`<SensitiveText type="amount" value={fmtMoney(value)} />`（明文金额千分位）。
  - 文档四件套：CLAUDE.md 第五阶段段落 + Memory References、`.claude/memory/stage5-accounting.md`、本进度文件、spec 回写（2.5 泛化命名 / 3.1 规则 1 精化 / 3.2 已入账流水禁止匹配）。
- 修改文件：`src/pages/BankTransactions.tsx`、`src/pages/OperationLogs.tsx`、`src/pages/FinancialReports.tsx`、`src/api/index.ts`、`CLAUDE.md`、`.claude/memory/stage5-accounting.md`、`.claude/memory/MEMORY.md`、`docs/superpowers/plans/2026-08-15-stage5-progress.md`、`docs/superpowers/specs/2026-08-15-stage5-accounting-reports-design.md`。
- 测试：
  - `npx tsc --noEmit`：通过。
  - `npm run lint`：通过。
  - `npm run build`：通过；仍有既有 Vite chunk 体积提示。
  - `cd src-tauri && cargo fmt --check`：通过。
  - `cd src-tauri && cargo check`：通过；仍有既有 7 个 warning（unused/dead_code）。
  - `cd src-tauri && cargo test --lib`：通过，119 个测试。
  - `npm run tauri dev` 手工验收：**未做**（无 GUI 环境），科目表/凭证/报表三页与银行流水生成凭证入口待 Windows 手工验收。
- 未完成：Windows exe 下手工验收（三页面 + 流水生成凭证 + 月结检查）。
- 下轮入口：第五阶段主线完成；可进入手工验收、发版或增强池（现金流量本年累计、多级科目等）。
- 提交：见 Task 14 commit。
