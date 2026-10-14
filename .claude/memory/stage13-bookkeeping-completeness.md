# 第十三阶段：记账完整性——固定资产与手工凭证（已交付）

spec：`docs/superpowers/specs/2026-10-14-stage13-bookkeeping-completeness-design.md`
progress：`docs/superpowers/plans/2026-10-14-stage13-progress.md`

## 模块地图

- `accounting.rs`：create_manual_voucher（收/付/转 + 资金辅助校验）/ void_manual_voucher
- `assets.rs`（新）：fixed_assets 登记购置凭证、run_asset_depreciation 月度批处理（聚合凭证）、处置清理凭证、台账/导出
- `db.rs`：fixed_assets / fixed_asset_depreciations 建表 + vouchers CHECK 扩展 4 source_type（stage13 重建）+ 月结 asset_depreciation warning
- 前端：固定资产页（财务管理组）+ 记账凭证页手工凭证入口

## 关键设计

- 手工凭证资金辅助校验对齐 bank_manual spec 4.7：资金科目（1001/1002/1012）行必须带 fund_account_id；收/付凭证必须含资金科目行
- 折旧直线法：原值×(1−残值率)/年限，末月尾差补齐；按月聚合一张凭证（借按费用科目分组、贷 1602 合计）
- 处置净损益：借 6711 净损失 / 贷 6301 净收益；处置收入资金行须带辅助
- 月结保护贯穿：登记/折旧/处置/手工凭证作废均拦月结月

## 前端交付

- FixedAssets.tsx（/fixed-assets，财务管理组）：四汇总卡（SensitiveStatistic 脱敏）/筛选/台账表格/登记编辑 Modal（购置贷方资金科目联动资金账户）/月度折旧/处置（Popconfirm）/导出
- Vouchers.tsx：手工录入凭证 Modal（种类 Segmented + 分录行编辑 + 借贷合计实时校验，门禁与后端同口径）；manual_voucher 来源凭证作废入口
- VOUCHER_SOURCE_LABEL 补 4 来源（配色同步）

## 已知边界

- 折旧仅直线法；购置贷方科目由用户指定（应付类或资金科目）
- Windows exe 手工验收挂账（并入发版清单）
