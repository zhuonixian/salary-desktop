# 第十三阶段进度：记账完整性——固定资产与手工凭证

spec：`docs/superpowers/specs/2026-10-14-stage13-bookkeeping-completeness-design.md`
基线：第十二阶段已交付（后端 390 测试 → 本阶段 397）

## Task 状态

| # | 任务 | 状态 |
|---|---|---|
| 1 | db.rs：fixed_assets / fixed_asset_depreciations 建表 + vouchers CHECK 扩展 4 source_type（stage13 重建）| ✅ |
| 2 | accounting.rs：create_manual_voucher（收/付/转 + 资金辅助校验 + 平衡门禁）/ void_manual_voucher | ✅ |
| 3 | assets.rs：登记（购置凭证）/ 月度折旧批处理（聚合凭证 + 幂等 + 尾差补齐）/ 处置（清理凭证 + 净损益） | ✅（7 单测） |
| 4 | 台账查询 / 月结 asset_depreciation_missing warning / 资产台账导出 | ✅ |
| 5 | 命令注册 7 条 + 后端全量 397 测试 + 提交 | ✅ |
| 6 | 前端：固定资产页 + 凭证页手工凭证入口（subagent） | ✅（d8ad43c） |
| 7 | 集成回归 + 文档收尾 + push | ✅（全量 397 回归绿 e98ef95 复验；手册第 15 章已补 e98ef95；memory 已回写；已推送 origin/master） |

## 实现决策与偏差记录

- **手工凭证 source_id 用纳秒时戳**：vouchers 的 (source_type, source_id) WHERE active 唯一索引
  要求每张手工凭证 source_id 唯一，业务上无源 → 取时间纳秒；作废按凭证 id 直接 UPDATE
  （void_vouchers_for_source 按 source_id 会 miss）。
- **折旧购置当月不提**（次月起提，小企业准则惯例）；提满跳过；残值率边界无可提额跳过。
- **折旧凭证 source_id = yyyymm**：按月唯一可追溯，重复执行靠 fixed_asset_depreciations
  UNIQUE(asset_id, month) 幂等拦截（凭证不重复生成）。
- **处置凭证资金行固定 1002**（收款科目），净损益 借 6711 损失/贷 6301 收益。
- **资产已计提后锁定口径字段**：原值/年限/残值率/归属月不可改（防台账漂移），名称/费用科目可改。
- **月结 warning**：当月存在 in_use、购置月<当月、未提满、且无当月折旧记录的资产 →
  asset_depreciation_missing（不阻断，跳转 /fixed-assets）。

## 挂账

- Windows exe 手工验收（已并入 v0.10 验收清单第九节）：登记含购置凭证 → 月度折旧幂等 → 处置净损益凭证 → 手工凭证三式与作废
- 加速折旧法/减值/在建工程转固（spec 不做范围）
- 找回密码命令补全 + 自动备份（安全债，独立排期）
