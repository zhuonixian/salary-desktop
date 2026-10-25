# 第十九阶段：组件抽取重构（已交付）

spec：`docs/superpowers/specs/2026-10-25-stage19-component-extraction-design.md`
路线图：`docs/superpowers/plans/2026-10-25-roadmap-v0.11-iterations.md`（迭代 5）
progress：`docs/superpowers/plans/2026-10-25-stage19-progress.md`

## 模块地图

纯搬家重构：五页 1118-1685 行 → 582-1052 行（合计 −2975 行），后端零改动、418 测试不动。新建组件文件按单元分组（组内专属、所有权互不重叠）：

- **BankTransactions**（1685→910）：`src/components/BankReconciliationWorkbench.tsx`（对账工作台双栏核销）、`src/components/BankReconciliationPeriodsPanel.tsx`（余额调节表）、`src/components/bankTransactionsShared.ts`（AMOUNT_TOLERANCE/fmtMoney/txSideAmount 三方共用）
- **FundDocuments**（1505→924）：`src/components/FundDocumentDetailDrawer.tsx`（单据详情抽屉，含附件预览 Modal 与 previewAtt 状态）、`src/components/FundDocumentFormModal.tsx`（新建/编辑表单，Form.useWatch 随 JSX 移入）、`src/components/fundDocumentsShared.tsx`（常量 + SharedOptions/FundDocFormValues + errText/fmtAmount/statusTag）
- **NotesInstruments**（1444→1052）：`src/components/InstrumentDetailDrawer.tsx`（票面信息+背书链+状态轨迹）、`src/components/InstrumentRegisterModal.tsx`（登记票据，watchRegType/watchRegDir 及派生布尔随 JSX 移入）、`src/components/instrumentsShared.tsx`（常量 + SharedOptions/RegisterFormValues + 公共小件）
- **FundAccounts**（1308→1017）：`src/components/FundMigrationWizardModal.tsx`（历史归集向导整体搬出，props 仅 open/onClose，含 BATCH_TYPE_LABEL）、`src/components/fundAccountsShared.ts`（errText/fmtAmount）
- **SalaryCalculate**（1118→582）：`src/components/SalaryEmailWizardModal.tsx`（工资条邮件三步向导，props: open/monthStr/resultEmployeeNos/onClose，useSecurity/useNavigate 组件内自理，入口 SMTP 校验迁入组件内完成）、`src/components/SalaryAnnualTaxModal.tsx`（个税年度汇总，open/onClose 自包含）

## 关键设计

- **纯搬家约束**：JSX/文案/handler 语义/接口调用顺序原样搬移，不趁机优化；行为保持靠 tsc 类型门禁 + 三轮复审走查（无 E2E）
- **表单实例两档策略**：与页面状态机耦合的表单（资金单据/登记票据）由页面持 form 实例经 props 传入、useWatch 随 JSX 移入组件订阅同一实例；自包含 Modal（归集向导/邮件向导/年度汇总）状态与逻辑整体搬出、props 仅 open/onClose
- **shared 文件防循环依赖**：每页配套 shared 文件单向供页面与组件引用；返回 JSX 的 statusTag 等工具放 .tsx shared（.ts 不支持 JSX，且受 react-refresh/only-export-components 约束须与组件实现分离）
- **门禁替代**：src-tauri 零改动以 `git status` 校验代替 cargo 全量回归（沙箱冷编译超时）；418 存量测试主会话本地验证绿

## 已知边界

- 行为保持靠类型门禁与人工走查，无 E2E；五页 UI 人工目验挂账 v0.10 验收清单第十一节（stage19 补项）
- futureSharedCandidates 七项留池（useBaseDataOptions hook、formatShared.ts 合并、导入流水 Modal、七个流转 Modal、三 Tab 拆分、审批/冲正 Modal、打印预览/受控解锁 Modal），见 spec 第 4 节
