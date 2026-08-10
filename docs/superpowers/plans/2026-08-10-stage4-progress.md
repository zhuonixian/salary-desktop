# 第四阶段进度同步

本文件用于第四阶段安全配置开发接力。每轮开发结束必须追加记录，避免上下文压缩、subagent 分工或新会话接手造成信息丢失。

## 当前基线

- 分支：`master`
- 阶段计划：`docs/superpowers/plans/2026-08-10-stage4-security-config.md`
- spec：`docs/superpowers/specs/2026-08-10-stage4-security-config-design.md`
- 长期摘要：`.claude/memory/stage4-security.md`
- 目标交付：启动密码、闲置自动锁、双层 KEK/DEK 加密、敏感字段默认脱敏、旧版数据迁移、Tauri 安全配置收紧。

## 协作规则

- 开发前先读 `CLAUDE.md`、本文件、阶段计划与 spec。
- subagent 只处理明确且互不重叠的文件范围。
- 主 agent 负责合并、测试、commit、push。
- 每轮结束补充"本轮记录"。

## 本轮记录模板

```md
### YYYY-MM-DD HH:mm

- 目标：
- 完成：
- 修改文件：
- 测试：
- 未完成：
- 下轮入口：
- 提交：
```

## 记录

### 2026-08-10 第四阶段 Task 1-2：加密原语与依赖

- 目标：补齐 Rust 侧加密能力。
- 完成：
  - 引入 `argon2` / `rand` `0.9` / `aes-gcm` / `hkdf` / `sha2` / `base64` 等依赖。
  - 新增 `src-tauri/src/security.rs`：Argon2id 派生 KEK、AES-GCM-256 加解密、HKDF 展开、`BACKUP_MAGIC`、wrap/unwrap DEK。
- 修改文件：`src-tauri/Cargo.toml`、`src-tauri/src/security.rs`。
- 测试：单元测试覆盖加解密往返、错误 KEK unwrap 失败、密码强度规则。
- 提交：`8601899 feat(security): 新增 KEK/DEK 派生与 AES-GCM 加解密原语`。

### 2026-08-10 第四阶段 Task 3：DB schema

- 目标：为安全状态、迁移状态、发票加密标记建表。
- 完成：
  - 新增 `security_state` 单行表（salt/verifier/keks/lock 状态/idle 配置等）。
  - 新增 `legacy_migration_state` 单行表，记录迁移阶段。
  - 给 `invoices` 表增加 `image_encrypted` 字段（兼容旧库）。
- 修改文件：`src-tauri/src/db.rs`。
- 测试：schema 存在性与默认值测试。
- 提交：`fdc982b feat(security): 新增 security_state/legacy_migration_state 表与 invoices.image_encrypted 字段`。

### 2026-08-10 第四阶段 Task 4：状态机

- 目标：实现 setup/unlock/lock 核心状态机。
- 完成：
  - `SecurityState` 内含 `Mutex<Inner>`：DEK / 失败计数 / lock_until。
  - setup：Argon2id 派生密码 KEK，wrap DEK 后写入三条 KEK 占位（密码 KEK）。
  - unlock：重派 KEK unwrap DEK，5 次失败后锁定 5 分钟。
  - lock：清空内存 DEK，不动 failed_attempts。
- 修改文件：`src-tauri/src/security.rs`。
- 测试：setup → unlock 往返、错误密码自增计数、5 次失败锁定。
- 提交：`8413057 feat(security): 新增 SecurityState 状态机与 setup/unlock/lock`。

### 2026-08-10 第四阶段 Task 5：改密与找回

- 目标：补齐改密、找回密码路径。
- 完成：
  - change_password：旧 KEK unwrap DEK → 新 KEK wrap。
  - reset_password_by_recovery / reset_password_by_question：使用恢复码 KEK / 问题答案 KEK unwrap DEK 后生成新密码 KEK。
  - 找回密码独立失败计数（3 次后锁定）。
  - idle / sensitive_reveal 配置读写命令。
- 修改文件：`src-tauri/src/security.rs`。
- 测试：改密保持 DEK 不变、找回成功后能用新密码 unlock、错误恢复码/答案失败。
- 提交：`2d3e762 feat(security): 新增改密与恢复码/安全问题找回`。

### 2026-08-10 第四阶段 Task 6：Tauri 命令

- 目标：把安全能力暴露为 13 个 invoke 命令。
- 完成：注册 `is_security_initialized` / `setup_security` / `unlock` / `lock` / `get_security_status` / `change_password` / `reset_password_by_recovery` / `reset_password_by_question` / `update_idle_settings` / `update_sensitive_reveal_settings` / `reveal_sensitive_data` / `get_legacy_migration_status` / `migrate_legacy_resources` / `get_decrypted_invoice_url`，并接入 `app.manage(SecurityState::new())`。
- 修改文件：`src-tauri/src/security_commands.rs`、`src-tauri/src/lib.rs`。
- 测试：编译通过；后续前端集成测试覆盖。
- 提交：`1cef4e9 feat(security): 注册 13 个安全相关 Tauri 命令`。

### 2026-08-10 第四阶段 Task 7：发票加密归档

- 目标：发票原图就地 AES-GCM 加密，预览时解密到临时目录。
- 完成：
  - `save_invoice` / `save_invoice_with_mutex` 在 DEK 可用时直接加密归档。
  - `encrypt_image_if_unlocked` 先写 `.enc.tmp` 再 rename 替换。
  - 新增 `get_decrypted_invoice_url` 命令：把加密原图解密到 `{temp}/salary-desktop-preview/{invoice_id}_{ts}.{ext}`。
  - DB 字段 `image_encrypted` 跟随写入。
- 修改文件：`src-tauri/src/invoice.rs`、`src-tauri/src/security_commands.rs`。
- 测试：发票保存加密往返测试（带 DEK 时）。
- 提交：`b6b23ee feat(security): 发票图片归档加密与解密预览命令`。

### 2026-08-10 第四阶段 Task 8：OCR token 与备份包加密

- 目标：扩展加密覆盖 OCR token 与数据库备份包。
- 完成：
  - `app_settings` 中 `baidu_access_token_enc` 存 AES-GCM 密文；DEK 不可用时回退明文 token。
  - `data_safety::backup_database` 增加 `encrypt: bool` 入参；加密备份带 `BACKUP_MAGIC` 头部。
  - `restore_database` 自动识别加密备份并要求 DEK。
- 修改文件：`src-tauri/src/ocr.rs`、`src-tauri/src/db.rs`、`src-tauri/src/data_safety.rs`、`src-tauri/src/commands.rs`。
- 测试：OCR token 加密往返、加密备份生成 + 解密还原、缺 DEK 时加密备份还原失败。
- 提交：`5608f16 feat(security): OCR token 与备份包加密`。

### 2026-08-10 第四阶段 Task 9：旧版迁移

- 目标：升级老库到加密形态。
- 完成：
  - `legacy_migration::migrate_legacy_resources`：发票未加密文件就地加密、明文 OCR token 加密入库。
  - `legacy_migration_state` 表记录当前阶段，避免重复迁移。
  - 仅在用户主动触发时执行；未初始化安全配置时返回错误。
- 修改文件：`src-tauri/src/legacy_migration.rs`。
- 测试：跳过已加密、加密明文发票、加密明文 token、状态表更新。
- 提交：`04e6e51 feat(security): 旧版发票图片与 OCR token 加密迁移` + `82b6b32 fix(security): 恢复码改用 CSPRNG 并收敛迁移触发条件`。

### 2026-08-10 第四阶段 Task 10：前端 types/api

- 目标：前端 TypeScript 侧补齐类型与 invoke 封装。
- 完成：新增 `SecurityStatus` / `UnlockResult` / `RevealResult` / `LegacyMigrationStatus` 类型；`api/index.ts` 增加 14 个安全相关 invoke 包装。
- 修改文件：`src/types/index.ts`、`src/api/index.ts`。
- 测试：`npx tsc --noEmit` 通过。
- 提交：`96f37ba feat(security): 前端类型与 invoke 封装`。

### 2026-08-10 第四阶段 Task 11：SecurityContext

- 目标：前端全局安全状态 Provider。
- 完成：`SecurityContext` 暴露 `isInitialized` / `isLocked` / `isSensitiveRevealed` 等；启动时探测，闲置计时由 LockScreen 接管；unlock 失败时根据后端 failed_attempts 抛错。
- 修改文件：`src/contexts/SecurityContext.tsx`。
- 测试：`npx tsc --noEmit` / `npm run lint` 通过。
- 提交：`8906acf feat(security): 新增 SecurityContext 与 Provider`。

### 2026-08-10 第四阶段 Task 12：LockScreen / SetupSecurity

- 目标：实现启动密码设置与锁屏页。
- 完成：
  - `SetupSecurity` 向导：密码强度校验、恢复码确认、安全问题、密码提示。
  - `LockScreen`：密码输入、错误提示、忘记密码入口（恢复码 / 安全问题双路径）、自动锁倒计时。
  - `App.tsx` 根据状态显示向导 / 锁屏 / 主界面。
- 修改文件：`src/components/SetupSecurity.tsx`、`src/components/LockScreen.tsx`、`src/components/RevealPasswordModal.tsx`、`src/App.tsx`。
- 测试：`npx tsc --noEmit` / `npm run lint` / `npm run build` 通过。
- 提交：`f26635d feat(security): 新增 LockScreen 与 SetupSecurity 向导`。

### 2026-08-10 第四阶段 Task 13：SensitiveText

- 目标：默认脱敏的通用文本展示组件。
- 完成：
  - `SensitiveText`：默认 `***`，点击眼睛图标触发 `RevealPasswordModal` 二次校验。
  - `SensitiveStatistic`：包裹 Ant Design Statistic，开启 reveal 时显示真实数字。
  - 5 分钟全局 reveal 窗口，由 `SecurityContext` 管理。
- 修改文件：`src/components/SensitiveText.tsx`、`src/components/SensitiveStatistic.tsx`、`src/components/RevealPasswordModal.tsx`。
- 测试：`npx tsc --noEmit` / `npm run lint` 通过。
- 提交：`3a011a0 feat(security): 新增 SensitiveText 脱敏组件与二次密码 Modal`。

### 2026-08-10 第四阶段 Task 14：安全中心

- 目标：集中暴露安全配置入口。
- 完成：`SecurityCenter.tsx` 含状态卡片、改密、找回密码、闲置与敏感解锁时长配置、迁移触发与进度；接入"输出审计"菜单。
- 修改文件：`src/pages/SecurityCenter.tsx`、`src/App.tsx`。
- 测试：`npx tsc --noEmit` / `npm run lint` 通过。
- 提交：`1fce2b4 feat(security): 新增安全中心页面`。

### 2026-08-10 第四阶段 Task 15：启动流程接入

- 目标：让 SecurityProvider 接管启动并实现闲置自动锁。
- 完成：
  - App 根包 `SecurityProvider`；启动探测初始化状态，已初始化则强制锁屏。
  - 闲置检测：5 分钟（可配）无键盘鼠标活动触发 `lock`。
  - 路由变更 / 系统对话框不计入活动。
- 修改文件：`src/App.tsx`、`src/contexts/SecurityContext.tsx`。
- 测试：`npx tsc --noEmit` / `npm run lint` 通过。
- 提交：`255babe feat(security): 启动流程接入 SecurityProvider 与闲置自动锁`。

### 2026-08-10 第四阶段 Task 16：脱敏第一批

- 目标：员工、工资、报销、付款页面敏感字段默认脱敏。
- 完成：身份证 / 银行账号 / 工资金额 / 报销金额 / 付款账号全部接入 `SensitiveText` / `SensitiveStatistic`；保持表格行内布局不破坏。
- 修改文件：`src/pages/Employees.tsx`、`src/pages/SalaryRules.tsx`、`src/pages/Reimbursement.tsx`、`src/pages/Payments.tsx`。
- 测试：`npx tsc --noEmit` / `npm run lint` / `npm run build` 通过。
- 提交：`02a31ce feat(security): 员工/工资/报销/付款页面敏感字段默认脱敏`。

### 2026-08-10 第四阶段 Task 17：脱敏第二批

- 目标：扩展脱敏到银行、财务分析、发票、Dashboard，并修复加密原图预览。
- 完成：
  - 银行交易对方账号 / 财务分析金额 / 发票价税合计 / Dashboard 工资总额默认脱敏。
  - `InvoiceImage` 通过 `convertFileSrc` 包装 `get_decrypted_invoice_url` 返回的临时路径，避免 WebView 直接读临时文件被拦。
  - `Statistic` value 类型修正、`matched_amount` undefined 兜底。
- 修改文件：`src/pages/BankTransactions.tsx`、`src/pages/FinancialAnalysis.tsx`、`src/pages/Invoices.tsx`、`src/pages/Dashboard.tsx`、`src/components/InvoiceImage.tsx`。
- 测试：`npx tsc --noEmit` / `npm run lint` / `npm run build` / `cargo test --lib` 通过。
- 提交：`ff40c2c feat(security): 银行/财务/发票/Dashboard 页面默认脱敏与加密预览` + `74f93c5 fix(security): InvoiceImage 用 convertFileSrc 包装加密预览路径` + `96b280f fix(security): 修复 Statistic value 类型错误与 matched_amount undefined 兜底`。

### 2026-08-10 第四阶段 Task 18：Tauri 安全配置

- 目标：在 `tauri.conf.json` 收紧能力面。
- 完成：
  - CSP 限制 `default-src 'self'`、`img-src` 限制 https/data/asset/convertFileSrc，禁用 `unsafe-eval`。
  - `assetProtocol` scope 限制 `$APPDATA` / `$TEMP/salary-desktop-preview`。
  - `withGlobalTauri=false`，关闭前端全局 Tauri 注入。
- 修改文件：`src-tauri/tauri.conf.json`。
- 测试：`npm run tauri build` 通过；加密发票预览路径仍可访问。
- 提交：`31aea72 feat(security): 收紧 CSP 与 assetProtocol scope`。

### 2026-08-10 第四阶段 Task 19：全量回归与 memory 更新

- 目标：完成最后回归、修若干累积 Minor、更新 memory 与 CLAUDE.md，push 到远程。
- 完成：
  - 修正 `SecurityContext.tsx` 注释里 `MAX_FAILED_ATTEMPTS` → `MAX_ATTEMPTS`（值未变）。
  - `lib.rs` 在 setup 启动时清理上次崩溃残留 preview 目录，并在 `builder.run()` 返回后再清理一次。
  - `cargo fmt` 顺带规整 security/data_safety/invoice 等模块格式。
  - 新增 `.claude/memory/stage4-security.md`，更新 `MEMORY.md`、`CLAUDE.md`，新增本进度文件。
  - 全量回归通过：`cargo fmt --check` / `cargo check` / `cargo test --lib`（87 passed）/ `npx tsc --noEmit` / `npm run lint` / `npm run build` / `npm run tauri build`。
- 修改文件：
  - `src/contexts/SecurityContext.tsx`
  - `src-tauri/src/lib.rs`
  - `src-tauri/src/{commands,data_safety,db,invoice,legacy_migration,security,security_commands}.rs`（fmt）
  - `.claude/memory/stage4-security.md`（新增）
  - `.claude/memory/MEMORY.md`
  - `CLAUDE.md`
  - `docs/superpowers/plans/2026-08-10-stage4-progress.md`（新增）
- 测试：
  - `cargo fmt --check`：通过。
  - `cargo check`：通过；仍有既有 dead_code warning（与第四阶段无关）。
  - `cargo test --lib`：通过，87 个测试。
  - `npx tsc --noEmit`：通过。
  - `npm run lint`：通过。
  - `npm run build`：通过；仍有既有 Vite chunk 体积提示。
  - `npm run tauri build`：通过，生成 Linux deb/rpm/AppImage。
- 未完成：
  - Windows exe 下启动密码设置、闲置自动锁、改密 / 找回密码、发票加密预览、迁移流程的手工验收。
  - 未来增强池：SQLCipher 整库加密、字段级加密、找回密码卡片完整功能、Employees 编辑 Modal / SalaryRules 字段脱敏。
- 下轮入口：第四阶段主线功能已完成，后续进入 Windows 手工验收或转入未来增强池。
- 提交：`4f69042 fix(security): 修复 Task 11/17 累积 Minor` + 文档/memory commit。
