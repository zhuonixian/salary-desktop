# 第十六阶段：增值税申报辅助——小规模纳税人季度申报表（已交付）

spec：`docs/superpowers/specs/2026-10-25-stage16-vat-quarterly-report-design.md`
路线图：`docs/superpowers/plans/2026-10-25-roadmap-v0.11-iterations.md`（迭代 2）

## 模块地图

- `excel.rs`：`vat_quarter_range`（季度→月份区间）/ `classify_vat_rate`（征收率分组纯函数：1%/3%/5%/免税/其他，0.4pp 容差+浮点余量）/ `build_vat_quarter_summary`（销项聚合 + 2221 净贷方勾稽 + 未月结月份）/ `export_vat_quarterly_report`（Sheet1 汇总 + Sheet2 销项明细）
- `commands.rs` + `lib.rs`：`export_vat_quarterly_report(year, quarter, path)` 注册留痕
- 前端：`ExportCenter.tsx` 税务申报模板区第 4 卡（年份+季度选择）；`api/index.ts` exportVatQuarterlyReport

## 关键设计

- 取数口径：direction='output' 且 status!='void' 且 belong_month ∈ 季度三个月；红字负数票计入冲减
- 勾稽：2221 当季有效凭证 credit−debit vs 台账税额合计，差异>0.01 输出提示行（不阻断）
- 未月结提示：month_closes.status='closed' 之外的季度月份写入表尾（反月结 'reopened' 视同未结）；不设硬门禁（申报期早于月结节奏）
- 附加税费：法定口径参考（城建 7%/教育 3%/地方 2%），表头注明属地差异与"六税两费"减半需自行核对——不做费率配置避免伪精确
- 定位是辅助预填表（同 stage10 扣缴端模板口径），不冒充官方格式

## 已知边界

- 一般纳税人申报表 / 附加费率配置 / 免税销售额细分：spec 第 5 节不做范围
- 测试用 `init_db` 临时文件库（setup_db 最小 schema 缺 vouchers/month_closes/ar_receipt_allocations）
