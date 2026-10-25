# Salary Desktop - 工资核算助手

Tauri 2 + React 19 + SQLite 的中文桌面工资与本地财务出纳工具。含员工考勤、薪酬社保个税、发票报销、付款与银行流水、自动凭证、财务报表、月结和数据安全。

## 核心命令

```bash
npm install                              # 安装前端依赖
npm run tauri dev                        # 开发模式（热重载）
npm run tauri build                      # 打包发布
cd src-tauri && cargo test --lib         # 后端单元测试
npx tsc -b                               # 前端类型检查（勿用 tsc --noEmit：根 tsconfig 仅 refs+files:[]，裸跑为空检查恒过）
npm run lint                             # ESLint
```

## 技术栈

| 层级 | 技术 |
|------|------|
| 前端 | React 19 + TypeScript + Ant Design 6 + Vite + react-router 7 |
| 后端 | Rust + Tauri 2 + rusqlite 0.31 (bundled) |
| Excel | rust_xlsxwriter (导出) / calamine (导入) |
| OCR | 百度 vat_invoice + 通用 OCR / 本地 PaddleOCR sidecar |
| HTTP | reqwest blocking + base64 |

## 架构摘要

后端模块：`commands.rs`（Tauri 命令入口）→ `db.rs`（schema/CRUD）+ `invoice.rs`（发票业务）+ `ocr.rs`（考勤 OCR）+ `salary.rs`（工资引擎）+ `accounting.rs`（凭证与报表）+ `cashier.rs`（资金出纳：账户/资金单/批次/核销/日记账/调节表/借款）+ `notes.rs`（票据台账：状态机/背书链/贴现/托收/冲正）+ `cash_count.rs`（现金盘点：面额明细/差异凭证）+ `notification.rs`（通知底座：媒介抽象/SMTP 配置加密/工资条邮件批量与重发）+ `security*.rs`（安全）+ `data_safety.rs`（备份恢复）+ `excel.rs`（导入导出）。前端为 `App.tsx` + 29 个 page。SQLite 单文件 `salary.db` 存于 `app_data_dir`。发票原图归档 `app_data_dir/invoices/{belong_month}/{timestamp}_{filename}`。

## 关键设计

- **DB 锁**：`Mutex<Connection>` 由 `tauri::State` 管理；发票 OCR 通过 `InvoiceOcrDbOps` trait 让 HTTP 调用不持锁
- **发票去重**：`(COALESCE(invoice_code,''), invoice_number)` partial unique index（`WHERE status != 'void'`）支持全电票无 code
- **OCR token**：`baidu_access_token` 缓存到 `app_settings`，110 错误自动清缓存重试一次
- **Tauri asset 协议**：`app.security.assetProtocol` 已启用，前端用 `convertFileSrc()` 渲染本地图片/PDF
- **未签名 Windows 构建**：SmartScreen 拦截，首次运行需手动绕过（见 `docs/windows-install-guide.md`）
- **Release 物料下载**：draft release 的普通下载可能卡住或拿到半截文件；优先按 `.claude/memory/release-workflow.md` 用 `gh api` 获取 asset 数值 ID 下载到 `dist/`，并用 SHA256 对 GitHub digest 校验。
- **启动密码**：Argon2id 派生 KEK，三条 KEK（密码/恢复码/安全问题答案）包裹同一 DEK；改密/找回只重包裹不动发票
- **资源加密**：发票图片就地 AES-GCM-256 加密归档（image_encrypted=1）；备份包可选加密（BACKUP_MAGIC + AES-GCM）；OCR token 加密入库
- **三层密码体系**：启动密码 → 锁屏（闲置 5min 可配）→ 敏感数据解锁（5min 全局）
- **Tauri 安全配置**：CSP 收紧、assetProtocol scope 限制 $APPDATA/$TEMP、withGlobalTauri=false

## Memory References

详细上下文在 `.claude/memory/` 目录：

- [Tauri 约定](.claude/memory/tauri-conventions.md) — 命令命名、State/Mutex、assetProtocol、文件路径
- [发票模块](.claude/memory/invoice-module.md) — dedup key、OCR 调用、PDF 直传、图片归档
- [发版流程](.claude/memory/release-workflow.md) — git tag → GitHub Actions → gh release → SmartScreen
- [命令清单](.claude/memory/commands-reference.md) — 完整开发/测试/发版命令
- [架构模块](.claude/memory/architecture.md) — 后端模块职责 + 数据流
- [第三阶段计划](.claude/memory/stage3-local-finance.md) — 本地数据安全、正式月结、付款批次、银行流水、预算异常；完整计划见 `docs/superpowers/plans/2026-08-10-stage3-local-finance.md`
- [第四阶段安全配置](.claude/memory/stage4-security.md) — 启动密码/锁屏/加密/脱敏/迁移；spec 见 `docs/superpowers/specs/2026-08-10-stage4-security-config-design.md`、plan 见 `docs/superpowers/plans/2026-08-10-stage4-security-config.md`
- [第五阶段财务专业功能](.claude/memory/stage5-accounting.md) — 科目表、自动凭证、三大报表、Excel 导出；spec 见 `docs/superpowers/specs/2026-08-15-stage5-accounting-reports-design.md`、plan 见 `docs/superpowers/plans/2026-08-15-stage5-accounting-reports.md`
- [第六阶段财务功能拓展](.claude/memory/stage6-finance-extensions.md) — 科目余额表、年末结转、社保台账、累计预扣、工资条、同期列；spec 见 `docs/superpowers/specs/2026-08-22-stage6-finance-extensions-design.md`、plan 见 `docs/superpowers/plans/2026-08-22-stage6-finance-extensions.md`
- [第七阶段出纳运营闭环](.claude/memory/stage7-cashier-operations.md) — 资金账户、通用收付款、审批留痕、多对多银行对账、资金日记账、借款核销；spec 见 `docs/superpowers/specs/2026-08-30-stage7-cashier-operations-design.md`、plan 见 `docs/superpowers/plans/2026-08-30-stage7-cashier-operations.md`
- [第八阶段票据与申报](.claude/memory/stage8-notes-tax-reports.md) — 票据台账、账期提醒、现金盘点、资金日报、个税扣缴申报表、进项台账；spec 见 `docs/superpowers/specs/2026-09-18-stage8-notes-tax-reports-design.md`、plan 见 `docs/superpowers/plans/2026-09-18-stage8-notes-tax-reports.md`
- [第九阶段通知模块](.claude/memory/stage9-notifications.md) — 通知底座（媒介抽象/SMTP 授权码加密/发送留痕）、工资条邮件三步向导、失败重发；spec 见 `docs/superpowers/specs/2026-09-19-stage9-notifications-design.md`、plan 见 `docs/superpowers/plans/2026-09-19-stage9-notifications.md`
- [第十阶段上手引导与办税落地](.claude/memory/stage10-onboarding-tax-filing.md) — 首启向导、示例账套 seed/clear、bootstrap_status、申报模板三件、征期日历；spec 见 `docs/superpowers/specs/2026-10-11-stage10-onboarding-tax-filing-design.md`、plan 见 `docs/superpowers/plans/2026-10-11-stage10-onboarding-tax-filing.md`
- [第十一阶段提醒触达](.claude/memory/stage11-reminder-delivery.md) — reminders.rs 四源聚合、OS 系统通知（每日一次去重）、托盘常驻（badge/菜单/关窗到托盘）；spec 见 `docs/superpowers/specs/2026-10-12-stage11-reminder-delivery-design.md`、plan 见 `docs/superpowers/plans/2026-10-12-stage11-reminder-delivery.md`
- [第十二阶段收入侧闭环](.claude/memory/stage12-revenue-cycle.md) — 销项登记（invoices.direction+收入确认凭证）、进项门禁回扫、应收核销/台账/账龄、ar_overdue 提醒、月结 warning、两导出；spec 见 `docs/superpowers/specs/2026-10-13-stage12-revenue-cycle-design.md`、progress 见 `docs/superpowers/plans/2026-10-13-stage12-progress.md`
- [第十三阶段记账完整性](.claude/memory/stage13-bookkeeping-completeness.md) — 手工凭证（收/付/转+资金辅助）、固定资产登记/折旧/处置三凭证、台账与月结 warning；spec 见 `docs/superpowers/specs/2026-10-14-stage13-bookkeeping-completeness-design.md`、progress 见 `docs/superpowers/plans/2026-10-14-stage13-progress.md`
- [第十四阶段安全债](.claude/memory/stage14-security-debt.md) — 找回凭据换绑（rotate_recovery_code/update_security_question，仅重包裹目标 KEK）、自动备份（解锁/月结后触发、auto-daily 加密包滚动保留）；spec 见 `docs/superpowers/specs/2026-10-21-stage14-security-debt-design.md`、progress 见 `docs/superpowers/plans/2026-10-21-stage14-progress.md`
- [第十五阶段上手体验](.claude/memory/stage15-ux-navigation-tour.md) — 导航收敛（资金出纳 9 项拆为资金收付/资金台账、票据台账入票据与报销，9 组每组 ≤4）与 PageTour 页面引导（银行对账/收付款单/票据台账/记账凭证四页）；spec 见 `docs/superpowers/specs/2026-10-25-stage15-ux-navigation-tour-design.md`；v0.11 迭代路线图见 `docs/superpowers/plans/2026-10-25-roadmap-v0.11-iterations.md`
- [第十六阶段增值税申报辅助](.claude/memory/stage16-vat-quarterly-report.md) — 小规模纳税人季度申报辅助表（销项按征收率分组 1%/3%/5%/免税、附加税费法定口径参考、2221 净贷方勾稽、未月结提示，导出中心第 4 卡）；spec 见 `docs/superpowers/specs/2026-10-25-stage16-vat-quarterly-report-design.md`、progress 见 `docs/superpowers/plans/2026-10-25-stage16-progress.md`
- [第十七阶段银行流水导入增强](.claude/memory/stage17-bank-import-mapping.md) — 列识别诊断（BankColumnHint 四态）+ 手工映射全链路（preview/import 同一映射）+ 单列金额兜底 + 别名扩充（贷方发生额/交易时间等）；旧签名 None 委托零破坏；spec 见 `docs/superpowers/specs/2026-10-25-stage17-bank-import-mapping-design.md`、progress 见 `docs/superpowers/plans/2026-10-25-stage17-progress.md`
- [v0.10.0 发版验收清单](docs/superpowers/plans/2026-10-13-v0.10-release-acceptance.md) — 阶段 7-11 Windows/macOS 手工验收汇编（含 stage12/13 两节），打 tag 前按清单内 A/B/C 分级执行；发版准备两项（版本 bump、全绿验证）已于 e98ef95 完成

## 第三阶段开发

第三阶段以本地轻量财务管理为目标，优先做数据安全中心和正式月结，再推进付款批次、银行流水匹配、预算异常。开发时先读 `.claude/memory/stage3-local-finance.md` 和 `docs/superpowers/plans/2026-08-10-stage3-progress.md`；涉及多模块开发时用 subagent 按互不重叠文件范围协作，由主 agent 统一合并、测试、commit、push。

## 第四阶段开发

第四阶段以本地单机应用访问安全为目标，按 KEK/DEK 加密 → 状态机 → 命令 → 前端 → 脱敏改造 → Tauri 配置收紧 → 全量回归顺序推进。开发时先读 `.claude/memory/stage4-security.md` 和 `docs/superpowers/plans/2026-08-10-stage4-progress.md`；spec 在 `docs/superpowers/specs/2026-08-10-stage4-security-config-design.md`。涉及多模块开发时用 subagent 按互不重叠文件范围协作，由主 agent 统一合并、测试、commit、push。

## 第五阶段开发

第五阶段以财务专业能力（科目表与三大报表）为目标，按凭证落库（事件驱动物化凭证）路线分四批推进：科目与期初 → 凭证引擎与业务挂接 → 报表与导出 → 前端页面与全量回归。开发时先读 `.claude/memory/stage5-accounting.md` 和 `docs/superpowers/plans/2026-08-15-stage5-progress.md`；spec 在 `docs/superpowers/specs/2026-08-15-stage5-accounting-reports-design.md`。涉及多模块开发时用 subagent 按互不重叠文件范围协作，由主 agent 统一合并、测试、commit、push。

## 第六阶段开发

第六阶段以财务功能拓展（账簿与结账闭环、社保公积金全链路、个税累计预扣）为目标，按批次推进：科目余额表与年末结转 → 社保台账与凭证联动 → 个税累计预扣与年度汇总 → 工资条打印 → 报表同期列 → 收尾回归。开发时先读 `.claude/memory/stage6-finance-extensions.md` 和 `docs/superpowers/plans/2026-08-22-stage6-progress.md`；spec 在 `docs/superpowers/specs/2026-08-22-stage6-finance-extensions-design.md`。涉及多模块开发时用 subagent 按互不重叠文件范围协作，由主 agent 统一合并、测试、commit、push。

## 第七阶段开发

第七阶段以出纳运营闭环为目标，按 Gate 0 → 7A 基础底座 → 7B 通用收付款与付款 → 7C 资金日记账与多对多银行对账 → 7D 借款/报销治理/月结收尾推进，Task 0-17 已全部交付（0-16 走 SDD review，收尾 Task 17 含导航核对、cash 严格开关隐藏、月结检查 account_type 兜底与文档四件套）。开发前先读 `.claude/memory/stage7-cashier-operations.md`、`docs/superpowers/plans/2026-08-30-stage7-progress.md` 和 spec；旧库迁移、资金金额守恒、状态机、月结保护为阻断项，Windows exe 手工验收仍待做。涉及多模块开发时用 subagent 按互不重叠文件范围协作，由主 agent 统一集成、测试、commit、push。

## 第八阶段开发

第八阶段以票据台账与申报台账为目标，按 8A 票据台账与账期提醒 → 8B 现金盘点与资金日报 → 8C 个税扣缴申报表/进项台账/Minor 12 项/收尾推进，Task 1-12 已全部交付（收尾含导航核对、OperationLogs 映射核对、全量回归与文档四件套）。开发前先读 `.claude/memory/stage8-notes-tax-reports.md`、`docs/superpowers/plans/2026-09-18-stage8-progress.md` 和 spec（4.1 已勘误：2201=应付票据、2202=应付账款）；票据状态机与凭证分录、冲正恢复前置状态、盘点差异凭证、月结双查（登记月+操作月）为阻断项，Windows exe 手工验收（票据全流程+冲正、盘点差异凭证、提醒卡、三导出）挂账待做。涉及多模块开发时用 subagent 按互不重叠文件范围协作，由主 agent 统一集成、测试、commit、push。

## 第九阶段开发

第九阶段以可选外联的通知能力为目标（仅 SMTP 外联，其余模块维持零外联），按 9A 通知底座与设置页 → 9B 工资条组装器/邮件向导/回归收尾推进，Task 1-7 已全部交付（通知底座、SMTP 授权码 AES-GCM 加密入库、通知设置页、工资条邮件三步向导、失败重发、员工 email 录入）。开发前先读 `.claude/memory/stage9-notifications.md`、`docs/superpowers/plans/2026-09-19-stage9-progress.md` 和 spec；阻断项：授权码不落明文（整键加密+回显脱敏+留空不修改）、发送不持 DB 锁（锁内取数后独立连接发送+留痕）、工资条发送三重门禁（月份已锁定+SMTP 已配置+敏感数据解锁）。Windows exe 手工验收（QQ 授权码配置→测试发送→批量 2 员工→记录核对→失败重发）挂账待做。涉及多模块开发时用 subagent 按互不重叠文件范围协作，由主 agent 统一集成、测试、commit、push。

## 第十阶段开发

第十阶段以上手引导与办税落地辅助为目标（零外联原则不变），按 10A 上手引导（bootstrap_status → 首启向导 → 示例账套 seed/clear → 首页检查清单）→ 10B 申报模板三件（扣缴端人员/工资薪金导入模板、社保基数申报表）→ 10C 征期日历（规则常量 + 完成留痕 + Dashboard 征期卡）推进。Task 1-11 已全部交付（含手册第十二章）。开发前先读 `.claude/memory/stage10-onboarding-tax-filing.md`、`docs/superpowers/plans/2026-10-11-stage10-progress.md` 和 spec；阻断项：示例账套双门禁（seed 仅核心业务表全 0、clear 校验数量一致且无月结后整表清理）、申报模板仅锁定月份、征期不内置节假日顺延。Windows exe 手工验收（向导全流程、示例体验与清除、扣缴端模板人工导入核对列序）挂账待做。涉及多模块开发时用 subagent 按互不重叠文件范围协作，由主 agent 统一集成、测试、commit、push。

## 第十一阶段开发

第十一阶段以提醒触达为目标（OS 本地通知 + 托盘常驻，零外联原则不变），按 11A 通知聚合（reminders.rs 四源：征期/账期/月结/备份）→ 11B OS 通知（tauri-plugin-notification + 前端 ReminderChecker：启动 +5s/每 30 分钟/托盘事件触发，每日一次去重，单轮 ≤4 条）→ 11C 托盘（tray-icon：菜单打开/立即检查/退出、tooltip 待办数、关窗到托盘）推进。开发前先读 `.claude/memory/stage11-reminder-delivery.md`、`docs/superpowers/plans/2026-10-12-stage11-progress.md` 和 spec；阻断项：通知文本不含金额（stage9 留痕口径）、征期窗口须从上月起算（否则次月 15 日申报期次整月漏报）、空账套零打扰、锁屏/未初始化跳过检查。托盘与 OS 通知手工验收（Windows/macOS）挂账待做。涉及多模块开发时用 subagent 按互不重叠文件范围协作，由主 agent 统一集成、测试、commit、push。

## 第十二阶段开发

第十二阶段以收入侧闭环为目标（销项登记→应收挂账→回款核销→账龄监控，零外联不变），按 12A 销项登记（direction 列+进项门禁回扫+收入确认凭证 借1122/贷6001/贷2221）→ 12B 应收核销（ar_receipt_allocations 多对多部分核销/取消留痕/月结保护）与台账页 → 12C 账龄分桶/ar_overdue 提醒与 OS 通知/月结 warning/两导出推进。开发前先读 `.claude/memory/stage12-revenue-cycle.md`、`docs/superpowers/plans/2026-10-13-stage12-progress.md` 和 spec；阻断项：销项票不得混入报销/进项台账（门禁回扫）、发票方向创建后不可改、核销金额不超未核销余额、月结保护双向（发票月+收款单月）。Windows exe 手工验收（销项录入→凭证→核销→账龄提醒→导出）挂账。

## 第十四阶段开发

第十四阶段以安全债为目标（找回凭据换绑 + 自动备份，零外联不变），14A 换绑（verify_password_and_load_dek 公共前置 / rotate_recovery_code 明文仅返回一次 / update_security_question，均只重包裹目标 KEK、成功清零防爆破计数）→ 14B 自动备份（run_auto_backup_if_due 幂等：enabled→非空账套→DEK 已加载→当日未备；auto-daily-*.enc 复用手动加密路径；滚动保留 3-20 份默认 5；成功写 last_data_backup_at 静默 30 天提醒）→ 前端（安全中心找回凭据维护卡、数据安全自动备份卡、SecurityContext 解锁与 MonthClose 月结 fire-and-forget 触发）。开发前先读 `.claude/memory/stage14-security-debt.md` 和 spec；阻断项：换绑不得影响其余两条 wrap（测试互斥覆盖）、自动备份失败不阻塞解锁/月结流程、日志不含密钥材料与答案明文。后端 410 测试。Windows exe 手工验收（换绑全流程/两触发点/滚动清理）见 v0.10 验收清单第十节。

## 编码约定

- 中文 UI 字符串、中文 commit message（可中英混合）
- Tauri 命令：`#[tauri::command]` + snake_case，前端 `invoke('snake_case_name')`
- 时间戳：`Utc::now().to_rfc3339()`
- 测试：单元测试在 `#[cfg(test)] mod tests`，用 `Connection::open_in_memory()`
- 不跳过 hooks，不 `--no-verify`

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community structure, and cross-file relationships.

Rules:
- For codebase questions, first run `graphify query "<question>"` when graphify-out/graph.json exists. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. These return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw grep output.
- If graphify-out/wiki/index.md exists, use it for broad navigation instead of raw source browsing.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost).
