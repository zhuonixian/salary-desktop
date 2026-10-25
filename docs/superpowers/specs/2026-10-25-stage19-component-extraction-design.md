# 第十九阶段设计：组件抽取重构

日期：2026-10-25
状态：待审阅
基线：第十八阶段已交付（后端 418 测试）
路线图：`docs/superpowers/plans/2026-10-25-roadmap-v0.11-iterations.md`（迭代 5；路线图自注"纯技术债无用户价值，继续蹭车做"，Top5 页面 1100-1553 行减脂）

## 1. 背景与问题

前端五个最大页面均长成"千行巨石"：页面骨架、多个 Modal/Drawer/面板 JSX、表单联动 watch、模块级常量与格式化工具全部挤在单文件（实测 1118-1685 行）。后果：

- 改一处需通读千行上下文，复审与并行协作困难（多 subagent 无法同时碰同一文件）
- 常量与工具在页面内私有，跨页近似实现各写一份（errText/fmtAmount/statusTag 三页各一份）
- 与项目"涉及多模块开发时用 subagent 按互不重叠文件范围协作"的惯例冲突

目标：**行为不变的纯搬家重构**，把五个页面拆出 12 个独立组件文件 + 4 个共享常量文件，页面降至约 620-1130 行。

## 2. 单元清单（五页，文件所有权互不重叠）

| # | 页面 | 抽取内容 | 预期行数 |
|---|---|---|---|
| 1 | src/pages/BankTransactions.tsx | 对账工作台双栏核销面板（原 L145-593 ReconciliationWorkbench）与余额调节表面板（原 L597-878 ReconciliationPeriodsPanel）连同三方共用常量（AMOUNT_TOLERANCE/fmtMoney/txSideAmount）抽为两个独立组件文件 + 一个共享常量文件 | 1685 → 约 950 |
| 2 | src/pages/FundDocuments.tsx | DocumentTab 内单据详情抽屉（原 L1142-1344，含附件预览 Modal L141-190 及仅服务它的 previewAtt 状态）与新建/编辑单据表单 Modal（原 L828-1024，Form.useWatch 随 JSX 移入）抽为独立组件；模块级常量与 SharedOptions/FundDocFormValues 类型沉入共享文件 | 1505 → 约 1000 |
| 3 | src/pages/NotesInstruments.tsx | InstrumentTab 内票据详情抽屉（原 L1194-1341 票面信息+背书链+状态轨迹）与登记票据 Modal（原 L726-885，watchRegType/watchRegDir 及派生布尔随 JSX 移入）抽为独立组件；模块常量与 SharedOptions/表单值类型沉入共享文件 | 1444 → 约 1130 |
| 4 | src/pages/FundAccounts.tsx | 完全自包含的历史资金归集向导 Modal（原 L103-380 FundMigrationWizardModal，props 仅 open/onClose，含 BATCH_TYPE_LABEL）整体搬出；errText/fmtAmount 沉入与页面共用的工具文件 | 1308 → 约 1020 |
| 5 | src/pages/SalaryCalculate.tsx | 工资条邮件三步向导（原状态 L57-70 + 逻辑 L189-352 + Modal JSX L869-1113，约 340 行；props: open/monthStr/resultEmployeeNos/onClose，useSecurity/useNavigate 组件内自理）与个税年度汇总 Modal（原 L48-53 + L75-165 + L776-813）抽为独立组件 | 1118 → 约 620 |

## 3. 纯搬家约束

- **只移动不改写**：JSX 结构、中文文案、handler 语义、接口调用与状态流转顺序保持原样；不趁机"顺手优化"
- **表单实例归属分两档**：
  - 与页面状态机耦合的表单（资金单据/登记票据）：`Form.useWatch` 与派生布尔随 JSX 移入组件、订阅页面持有的同一 form 实例（form 经 props 传入），提交逻辑留页面层
  - 完全自包含的 Modal（历史归集向导/工资条邮件向导/个税年度汇总）：状态、副作用与逻辑随组件整体搬出，props 仅 open/onClose（邮件向导加 monthStr/resultEmployeeNos 业务入参），useSecurity/useNavigate 在组件内自理
- **共享常量下沉本单元**：常量/类型/小工具沉入每页配套的 shared 文件，页面与组件从 shared 导入，杜绝页面↔组件循环依赖
- **文件所有权互不重叠**：每个单元独占"本页面 + 本单元新建组件/shared 文件"，五个单元可并行推进互不冲突；statusTag 等返回 JSX 的工具放 .tsx shared 文件（.ts 不支持 JSX）

## 4. 不做范围（留池候选 futureSharedCandidates）

- FundDocuments/NotesInstruments 两页重复的 SharedOptions 接口与基础资料下拉一次性加载逻辑（getBusinessPartners/getEmployees/getFundAccounts/getGlAccounts/getOperatorProfiles → shared 状态），可提炼为 useBaseDataOptions hook 供两页共用（需跨页面新建共享文件，本轮不做）
- errText/fmtAmount/statusTag 等小工具在 FundDocuments/NotesInstruments/FundAccounts 三页各有一份近似实现，本轮各自沉入本单元 shared 文件，后续可合并为 src/components/formatShared.ts 统一金额与错误文案格式化
- BankTransactions 导入流水 Modal（src/pages/BankTransactions.tsx:1504-1680 及其状态/handler L948-1022，约 250 行）体量大但与页面 fetchData 耦合，作为该页第二轮抽取候选
- NotesInstruments 七个流转 Modal（背书/贴现/托收/到账确认/兑付/作废/冲正，L887-1191）与状态机 handler 深耦合，后续可抽为一组哑 Modal 组件（form 实例由父持有、props 传入）
- FundAccounts 三个 Tab（FundAccountTab/PartnerTab/OperatorTab，L399-1286）各自 230-370 行，后续可各自搬为独立文件
- FundDocuments 审批意见/冲正两 Modal（COMMENT_META 与 L1027-1139）与状态机 handler 耦合，后续候选
- SalaryCalculate 工资条打印预览 Modal（L814-867）与受控解锁 Modal（L743-775）后续候选

## 5. 验收与测试策略

- 纯前端重构，无新功能、无行为变化，不新增测试用例
- 类型门禁：`npx tsc -b` + `npm run lint` + `npm run build` 全绿
- 后端零改动：`git status src-tauri` 空输出（代替 cargo 全量回归；418 存量测试主会话本地验证）
- 五页 UI 人工目验（行为应无变化），并入 v0.10 验收清单第十一节补项
- 无 E2E；行为保持靠类型门禁与人工走查兜底

## 6. 关联

- 路线图迭代 5（`docs/superpowers/plans/2026-10-25-roadmap-v0.11-iterations.md`）
- 进度：`docs/superpowers/plans/2026-10-25-stage19-progress.md`；memory：`.claude/memory/stage19-component-extraction.md`
- 各被抽组件的业务 spec：对账工作台/调节表见 stage7、资金单据见 stage7/12、票据台账见 stage8、历史归集向导见 stage7、工资条邮件向导见 stage9、个税年度汇总见 stage6
