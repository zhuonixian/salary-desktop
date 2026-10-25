# 第十六阶段进度：增值税申报辅助——小规模纳税人季度申报表

spec：`docs/superpowers/specs/2026-10-25-stage16-vat-quarterly-report-design.md`
路线图：`docs/superpowers/plans/2026-10-25-roadmap-v0.11-iterations.md`（迭代 2）
基线：第十五阶段已交付（后端 410 测试 → 本阶段 414）

## Task 状态

| # | 任务 | 状态 |
|---|---|---|
| 1 | spec：取数口径/征收率分组/附加税费/勾稽提示设计 | ✅ |
| 2 | 后端：纯函数 + 聚合 + 双 sheet 导出 + 命令注册 | ✅ |
| 3 | 后端测试 6 项（区间映射/分组边界含浮点容差/聚合与勾稽与未月结/导出与空季度报错） | ✅ |
| 4 | 前端：导出中心第 4 卡（年份+季度）+ api 封装 | ✅ |
| 5 | 回归 + 文档四件套 + push | 🔄 |

## 实现决策与偏差记录

- rust_xlsxwriter 版本 API 为 `write_string_with_format`/`write_number_with_format`（4 参 write_string 不存在）——首版按新版 API 写错，全局替换修正。
- 0.4pp 容差边界（2.6/100×100）有浮点误差，实现加 1e-9 余量。
- `db::tests::setup_db` 是最小 schema（无 vouchers/month_closes/ar_receipt_allocations），测试改用 `init_db` 临时文件库（data_safety 测试同款）。
- month_closes 列名是 `month` 不是 `belong_month`（首版写错，对照 reopen_month 实现修正）；反月结置 status='reopened'，勾稽过滤 status='closed' 正确覆盖。

## 挂账

- Windows exe 手工验收：导出含数据的季度 → 核对分组/附加税费/勾稽提示行；并入 v0.10 验收清单第十一节（B 级）
- 一般纳税人申报表、附加费率配置：spec 第 5 节不做范围
