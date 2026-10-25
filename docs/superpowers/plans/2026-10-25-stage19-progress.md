# 第十九阶段进度：组件抽取重构

spec：`docs/superpowers/specs/2026-10-25-stage19-component-extraction-design.md`
路线图：`docs/superpowers/plans/2026-10-25-roadmap-v0.11-iterations.md`（迭代 5）
基线：第十八阶段已交付（后端 418 测试，本阶段零后端改动）
## Task 状态

| # | 任务 | 状态 |
|---|---|---|
| 1 | BankTransactions：对账工作台 + 余额调节表面板抽为组件，AMOUNT_TOLERANCE/fmtMoney/txSideAmount 沉入 bankTransactionsShared | ✅ |
| 2 | FundDocuments：单据详情抽屉（含附件预览）+ 新建/编辑表单 Modal 抽为组件，常量与 SharedOptions/FundDocFormValues 沉入 fundDocumentsShared | ✅ |
| 3 | NotesInstruments：票据详情抽屉 + 登记票据 Modal 抽为组件，常量与类型沉入 instrumentsShared | ✅ |
| 4 | FundAccounts：历史归集向导整体搬出（props 仅 open/onClose），errText/fmtAmount 沉入 fundAccountsShared | ✅ |
| 5 | SalaryCalculate：工资条邮件三步向导 + 个税年度汇总 Modal 抽为组件 | ✅ |
| 6 | 门禁：tsc/lint/build + src-tauri 零改动 + 418 存量回归 | ✅ |
| 7 | 文档四件套（spec/progress/memory/CLAUDE.md）+ 路线图勾销 + v0.10 验收清单第十一节补项 | ✅ |

## 实际行数变化（wc -l 实测）

| 页面 | 重构前（HEAD） | 重构后 | 降幅 |
|---|---|---|---|
| src/pages/BankTransactions.tsx | 1685 | 910 | −775（−46%） |
| src/pages/FundDocuments.tsx | 1505 | 924 | −581（−39%） |
| src/pages/NotesInstruments.tsx | 1444 | 1052 | −392（−27%） |
| src/pages/FundAccounts.tsx | 1308 | 1017 | −291（−22%） |
| src/pages/SalaryCalculate.tsx | 1118 | 582 | −536（−48%） |

合计 −2975 行；新建 12 个组件文件 + 4 个 shared 文件（清单见 `.claude/memory/stage19-component-extraction.md` 模块地图）。

## 门禁结果

- `npx tsc -b` 绿（exit 0）
- `npm run lint` 绿（exit 0）
- `npm run build` 绿（exit 0；既有 chunk >500kB 体积警告为存量提示，非错误）
- `git status --porcelain src-tauri` 空输出：后端零改动（纯前端重构）
- `cargo test --lib` 418 存量测试：主会话本地验证绿

## 实现决策与偏差记录

- **三轮复审制**：每单元抽取后不走单次自审，按三轮复审（抽取自查 → 交叉走查 diff 与原实现逐段对照 → 主会话终审）确认"纯搬家无行为漂移"后才并入门禁。
- **并行文件所有权约束**：五单元由 subagent 并行推进，严格按 spec 的"页面 + 专属新组件/shared 文件"所有权分工，无任何两单元触碰同一文件，合并零冲突。
- **沙箱 cargo 冷编译两次超时后改零改动校验**：子代理沙箱内 `cargo test` 冷编译两次超时，回归策略改为 `git status src-tauri` 零改动校验佐证后端不受影响，418 存量测试由主会话本地验证兜底。
- 行数为 wc -l 实测值，与 spec 预估（约 950/1000/1130/1020/620）为约数对照，实际均等于或低于预估。
- statusTag（返回 JSX）因 react-refresh/only-export-components 约束从组件文件迁回 fundDocumentsShared.tsx（.ts 不支持 JSX），见该文件头注释。

## 挂账

- 重构后五页 UI 人工目验（行为应无变化）：并入 v0.10 验收清单第十一节补项（`docs/superpowers/plans/2026-10-13-v0.10-release-acceptance.md`）
- futureSharedCandidates 七项留池（见 spec 第 4 节），本轮不做
