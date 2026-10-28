# 第二十阶段进度：凭证打印与 Web E2E 基建

spec：`docs/superpowers/specs/2026-10-25-stage20-voucher-print-web-e2e-design.md`
路线图：`docs/superpowers/plans/2026-10-25-roadmap-v0.11-iterations.md`（迭代 6）
基线：第十九阶段已交付（后端 418 测试，本阶段零后端改动）

## Task 状态

| # | 任务 | 状态 |
|---|---|---|
| 1 | VoucherPrintModal 组件：中式记账凭证版式（抬头/分录表/补空行/合计行/署名区）+ 打印门禁 + 多张分页 | ✅ |
| 2 | 凭证页行级「打印」入口接入（openPrintModal，props 留多张扩展） | ✅ |
| 3 | 打印 CSS（voucher-print-* 屏显预览 + @media print 独立分页，与 payslip-* 不重叠） | ✅ |
| 4 | 预览 mock 演示凭证两张（记-YYYYMM-001/002，多行分录借贷平衡，E2E 可见） | ✅ |
| 5 | Web E2E 运行器 scripts/web_e2e.mjs（vite + 无头 Chrome CDP，三场景，输出契约，孤儿进程兜底清理） | ✅ |
| 6 | package.json scripts 加 `e2e:web`（本阶段唯一 package.json 改动） | ✅ |
| 7 | 门禁：tsc/lint/build + E2E 3/3 + src-tauri 零改动校验 | ✅ |
| 8 | 文档四件套（spec/progress/memory/CLAUDE.md）+ 路线图勾销 + v0.10 验收清单第十一节补项 | ✅ |

## 门禁结果（本轮实跑）

- `npx tsc -b` 绿（tsc-exit=0）
- `npm run lint` 绿（lint-exit=0）
- `npm run build` 绿（build-exit=0；bootstrap chunk 1967.27 kB >500kB 体积警告为存量提示，非错误）
- `node scripts/web_e2e.mjs` 两次运行均三场景全过（可重复跑通验证）：
  - 第一次：`E2E_SUMMARY|pass=3|fail=0`（A 2188ms / B 2139ms / C 928ms，exit 0）
  - 第二次：`E2E_SUMMARY|pass=3|fail=0`（A 2202ms / B 2116ms / C 936ms，exit 0）
  - 场景 C 断言凭证字号「记-202610-001」、借贷金额与合计行可见（金额处于脱敏态 ¥****，符合门禁预期）、弹层正常关闭
- `git status --porcelain src-tauri` 空输出：后端零改动

## 实现决策与偏差记录

- **并行双实施**：功能侧（打印组件/CSS/mock/页面接入）与 E2E 侧（web_e2e.mjs 运行器）文件所有权互不重叠，由 subagent 并行推进，主会话统一合并与门禁。
- **独立复审**：合并前对两侧分别独立复审（组件复审聚焦版式口径与门禁一致性；运行器复审聚焦进程清理与断言强度），E2E 作为最强门禁替代人工走查——场景 C 即是对打印功能的端到端断言。
- **沙箱 cargo 冷编译超时故用零改动校验**：沿用 stage19 先例，子代理沙箱内 `cargo test` 冷编译超时，后端回归策略为 `git status --porcelain src-tauri` 空输出佐证零改动，418 存量测试由主会话本地验证兜底。
- **E2E 断言强度取舍**：打印按钮只认「表格行内」按钮、预览弹层用「打印预览」+「凭证字号」双标识唯一锁定（区分手工凭证 Modal），拒绝宽松兜底匹配，防假通过。
- **环境净化坑**：spawn 净化 env（delete DISPLAY 与 DBUS_SESSION_BUS_ADDRESS）是运行器能否跑起来的第一拦路虎，已在 spec 第 3.2 节与 memory 固化。

## 挂账

- 凭证打印真机目验（打印预览版式与分页）+ `node scripts/web_e2e.mjs` 可重复跑通：并入 v0.10 验收清单第十一节补项（`docs/superpowers/plans/2026-10-13-v0.10-release-acceptance.md`）
- 多张批量勾选打印入口、打印模板配置、E2E 接 CI：留池（见 spec 第 4 节）
