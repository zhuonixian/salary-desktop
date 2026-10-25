# 第十八阶段：现金流量表本年累计列修复（已交付）

spec：`docs/superpowers/specs/2026-10-25-stage18-cashflow-ytd-design.md`
路线图：`docs/superpowers/plans/2026-10-25-roadmap-v0.11-iterations.md`（迭代 4；stage3-6 增强池遗留）

## 模块地图

- `accounting.rs` build_cash_flow_statement：新增 ytd_sums（`get_vouchers_range(本年-01, 所选月)`，与上年同期同一 range 口径，sum_cash_flow 内排除 period_close）；七行 comparative 从硬编码 0.0 改填 YTD；净额公式复算 `net_increase_ytd`
- `models.rs`：CashFlowStatement 新增 `net_increase_ytd`
- `excel.rs` export_cash_flow_statement：净增加额 YTD 单元格由"各行 comparative 求和"（口径错误）改用 `net_increase_ytd`
- 前端 FinancialReports：现金流量表列名 "对比金额"→"本年累计"（对齐利润表/导出表头）；汇总行补 YTD 列；统计卡新增"现金净增加额（本年累计）"；mock 层补字段

## 关键设计

- YTD 无需启用月探测：range 查询天然只返回启用月起的凭证，启用月前为 0
- 1 月报表 YTD=当月（与利润表既有口径一致，测试对齐）
- 利润表/资产负债表口径不动

## 已知边界

- 间接法补充资料（净利润调节）不做
- 导出 Excel 与页面共用同一 statement，修复自动双向生效
