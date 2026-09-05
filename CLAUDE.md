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

后端模块：`commands.rs`（Tauri 命令入口）→ `db.rs`（schema/CRUD）+ `invoice.rs`（发票业务）+ `ocr.rs`（考勤 OCR）+ `salary.rs`（工资引擎）+ `accounting.rs`（凭证与报表）+ `cashier.rs`（资金出纳：账户/资金单/批次/核销/日记账/调节表/借款）+ `security*.rs`（安全）+ `data_safety.rs`（备份恢复）+ `excel.rs`（导入导出）。前端为 `App.tsx` + 25 个 page。SQLite 单文件 `salary.db` 存于 `app_data_dir`。发票原图归档 `app_data_dir/invoices/{belong_month}/{timestamp}_{filename}`。

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
