# Salary Desktop - 工资核算助手

Tauri 2 + React 19 + SQLite 的中文桌面工资核算工具。出纳用，含员工管理、考勤 OCR、工资核算、发票 OCR 去重、Excel 导出。

## 核心命令

```bash
npm install                              # 安装前端依赖
npm run tauri dev                        # 开发模式（热重载）
npm run tauri build                      # 打包发布
cd src-tauri && cargo test --lib         # 后端单元测试
npx tsc --noEmit                         # 前端类型检查
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

后端单文件模块：`commands.rs`（tauri 命令入口）→ `db.rs`（CRUD）+ `invoice.rs`（业务层）+ `ocr.rs`（考勤 OCR）+ `salary.rs`（工资引擎）+ `excel.rs`（导入导出）。前端单页：`App.tsx` + 9 个 page。SQLite 单文件 `salary.db` 存于 `app_data_dir`。发票原图归档 `app_data_dir/invoices/{belong_month}/{timestamp}_{filename}`。

## 关键设计

- **DB 锁**：`Mutex<Connection>` 由 `tauri::State` 管理；发票 OCR 通过 `InvoiceOcrDbOps` trait 让 HTTP 调用不持锁
- **发票去重**：`(COALESCE(invoice_code,''), invoice_number)` partial unique index（`WHERE status != 'void'`）支持全电票无 code
- **OCR token**：`baidu_access_token` 缓存到 `app_settings`，110 错误自动清缓存重试一次
- **Tauri asset 协议**：`app.security.assetProtocol` 已启用，前端用 `convertFileSrc()` 渲染本地图片/PDF
- **未签名 Windows 构建**：SmartScreen 拦截，首次运行需手动绕过（见 `docs/windows-install-guide.md`）

## Memory References

详细上下文在 `.claude/memory/` 目录：

- [Tauri 约定](.claude/memory/tauri-conventions.md) — 命令命名、State/Mutex、assetProtocol、文件路径
- [发票模块](.claude/memory/invoice-module.md) — dedup key、OCR 调用、PDF 直传、图片归档
- [发版流程](.claude/memory/release-workflow.md) — git tag → GitHub Actions → gh release → SmartScreen
- [命令清单](.claude/memory/commands-reference.md) — 完整开发/测试/发版命令
- [架构模块](.claude/memory/architecture.md) — 后端模块职责 + 数据流

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
