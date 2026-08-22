# 第六阶段进度同步

本文件用于第六阶段财务功能拓展（科目余额表、年末结转、社保台账、累计预扣、工资条、同期列）开发接力。每轮开发结束必须追加记录，避免上下文压缩、subagent 分工或新会话接手造成信息丢失。

## 当前基线

- 分支：`master`
- 阶段计划：`docs/superpowers/plans/2026-08-22-stage6-finance-extensions.md`
- spec：`docs/superpowers/specs/2026-08-22-stage6-finance-extensions-design.md`
- 长期摘要：`.claude/memory/stage6-finance-extensions.md`
- 目标交付：科目余额表与年末结转闭环、社保公积金台账与凭证联动、个税累计预扣与年度汇总、工资条打印、三大报表上年同期对比。

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

### 2026-08-22 批次一：科目余额表与年末结转闭环（Task 1-4）

- 目标：科目余额表（试算平衡）引擎/命令/Excel 导出/财务报表页 Tab；年末结转凭证生成与作废、报表口径排除 period_close、12 月月结挂接。
- 提交：`d1cbbee..f7686f0`（Task 1: 9a76fb1、Task 2: 0a263ff、Task 3: a4a4934、Task 4: f7686f0）。
- 测试：cargo test --lib 132 通过。

### 2026-08-22 批次 2a：社保公积金台账（Task 5-8）

- 目标：`social_insurance_profiles` 表与 CRUD/调基/上下限、6 个命令与社保台账页面、工资计算挂接台账（clamp/回退/单位部分落库）、计提凭证全额成本口径与代扣腿。
- 提交：`f7686f0..40c047c`（Task 5: b60a27a、Task 6: ca4f01a、Task 7: 6f4b0a3、Task 8: 40c047c）。
- 测试：cargo test --lib 137 通过。

### 2026-08-22 批次 2b：个税累计预扣（Task 9-10）

- 目标：`tax_rules.scope` + 累计 7 档、`calculate_cumulative_tax` 平滑切换；个税年度汇总查询与 Excel 导出。
- 提交：`40c047c..e7552af`（Task 9: 5130a1d、Task 10: e7552af）。
- 测试：cargo test --lib 140 通过。

### 2026-08-22 批次 2c：工资条打印（Task 11）

- 目标：工资核算页工资条预览与打印（明文金额，敏感解锁门槛）。
- 提交：`e7552af..f38773a`（Task 11: f38773a）。

### 2026-08-22 批次三：报表上年同期列（Task 12）

- 目标：三大报表上年同期对比列与导出（`has_prior_year` 标志）。
- 提交：`f38773a..7828586`（Task 12: 7828586）。
- 测试：cargo test --lib 141 通过。

### 2026-08-22 收尾：全量回归与文档（Task 13）

- 目标：全量回归 + 文档四件套（CLAUDE.md、memory 摘要、本进度文件、使用手册 v0.6.0）+ MEMORY.md 索引 + graphify 更新。
- 测试（全量回归，全部通过）：
  - `npx tsc --noEmit`：通过。
  - `npm run lint`：通过。
  - `npm run build`：通过；仍有既有 Vite chunk 体积提示。
  - `cd src-tauri && cargo fmt --check`：通过。
  - `cd src-tauri && cargo check`：通过；既有 5 个 dead_code/unused warning 保留。
  - `cd src-tauri && cargo test --lib`：141 个测试全部通过。
- 未完成：Windows exe 手工验收待做——科目余额表 Tab 与导出、12 月月结年末结转/反月结作废、社保台账页（录入/调基/上下限）、个税年度汇总、工资条预览打印、报表同期列展示。
- 下轮入口：第六阶段主线完成；进入主控最终全分支审查、Windows 手工验收、发版（v0.6.0）。
