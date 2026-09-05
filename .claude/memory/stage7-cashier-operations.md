---
name: stage7-cashier-operations
description: 第七阶段出纳运营闭环（资金账户、通用收付款、多对多对账、审批留痕、借款核销）已交付能力、已知边界与验收清单
---

# 第七阶段：出纳运营闭环（已交付）

设计：`docs/superpowers/specs/2026-08-30-stage7-cashier-operations-design.md`

实施计划：`docs/superpowers/plans/2026-08-30-stage7-cashier-operations.md`

进度：`docs/superpowers/plans/2026-08-30-stage7-progress.md`（Task 0-17 全部 complete）

## 定位

保持单公司、本地单机、出纳主导，不扩展为完整 ERP。已把资金账户、收付款、付款批次、银行流水、凭证辅助核算、日记账、对账和月结串成闭环，并补足流程留痕与员工借款核销。

## 已交付能力（按批次）

- **7A 基础底座**：BusinessMonthContext 全局月份贯通；资金账户/往来单位/操作人三类基础资料（引用保护+停用约束）；业务附件加密底座（add/删除挂实体状态门禁）；未选操作人时三层导航拦截。
- **7B 通用收付款**：`fund_documents` 全类型状态机（receipt/payment/transfer/advance/advance_settlement/reversal），状态流转与 approval_events 同事务留痕；maker_checker 经办复核；结算凭证 + 事务内红字冲正；vouchers 表安全重建；付款批次新增 general（三批次账户维度必选，逐单结算防双重贷记）；旧批次只读；付款凭证资金行补 `fund_account_id`。
- **7C 资金对账**：历史资金归集向导（preview/apply，apply 记审计+事务内刷新迁移标记）；银行流水账户化导入+预览；多对多核销引擎 `bank_reconciliation_allocations`（金额守恒、六因子评分、批量确认冲突消解）；旧三命令退役（写前拦截+预过滤+UI 移除）；资金日记账（voucher_lines 派生、滚动余额跨月滚入）；余额调节表（生成/确认/Excel/月结保护）。
- **7D 管理闭环**：借款核销 `advance_settlement_links`（现金归还/报销抵扣/工资扣回分录分流+累计核销上限+取消恢复）；借款台账（未清/逾期/账龄四桶+Excel）；报销审批治理（状态机命令化、直写通道物理删除、unapprove 联动）；月结 5 项新检查（待审批/已审批未付款/已结算无凭证 blocking、借款逾期 warning）+ 严格模式账户级开关 + 月结包新增日记账/调节表/借款台账 + 仪表盘资金卡 + 预算口径去重 + operator 后端筛选；月结严格检查 SQL 带 `account_type IN ('bank','third_party')` 兜底（防旧备份恢复 cash+strict 死锁）。

## 已知边界（重要口径）

1. **冲正严口径**：已结算单据不直接作废，只能红字冲正（原凭证 active + 反向凭证并存，净影响 0）；正式月结锁定期内的资金写操作被拒——**冲正/纠错须先反月结**。
2. **严格模式为账户级**：`fund_accounts.strict_reconciliation` 默认关，仅 bank/third_party 可开（应用层拦截 + 月结 SQL 兜底双保险）；开启后未确认调节表/部分核销由 warning 升级 blocking。
3. **前端类型检查必须用 `npx tsc -b`**：根 tsconfig 仅 refs + `files:[]`，`tsc --noEmit` 裸跑为空检查恒过（CLAUDE.md 已勘误）。
4. 独立凭证分录（manual 等无来源联动）`fund_account_id` 保持 NULL 属 spec 9.5 允许终态，其月结 warning 在归集向导执行后升级 blocking。
5. `unmatched_paid_batch_count` 月结口径仍查旧 matches 表（版本周期结束后切 allocation）。

## Windows exe 验收清单（待做）

- 旧库升级启动 → 建资金账户 → 历史归集向导 → 资金日记账核对（期初衔接）
- general 批次全流程：新建→审批→批次→付款→结算→凭证，异常路径（作废释放、跨月拦截）
- 冲正路径：已结算资金单冲正 → 凭证红字 → 日记账/调节表净影响 0
- 借款三方式核销 + 取消核销恢复 + 台账账龄展示
- 对账工作台双栏核销、余额调节表生成/确认、旧匹配迁移报告
- 月结 12 项检查、严格模式 blocking、月结包四件导出
- 锁屏/解锁与敏感数据脱敏（账号、金额）回归

## Minor 挂账（后续打磨，均不阻断）

- FundDocuments 编辑核销草稿会抹 settlement_mode/due_date（建议禁编或透传）
- bank_manual 账户下拉仅 bank/third_party（UX 收窄）；跨月冲正凭证口径仅隐式覆盖
- attachment_disk_stats O(n²)；预算 expense_type 为精确文本匹配（严格统计需下拉化）
- mock（浏览器预览模式）部分写操作仅默认返回，预览不支持冲正/取消
- 存量单填过 counter_account_code 升级后贷方回落 1221（极窄边角）

## 范围外

多用户账号、强权限、在线审批、云同步、银企直联、外币、多账套、采购库存销售、税务申报和完整行政 OA。

## 测试门槛

```bash
npx tsc -b
npm run lint
npm run build
cd src-tauri && cargo fmt --check
cd src-tauri && cargo check
cd src-tauri && cargo test --lib
```

旧库迁移和 Windows exe 主流程手工验收不可省略。
