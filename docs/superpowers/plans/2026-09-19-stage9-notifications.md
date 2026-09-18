# 第九阶段通知模块 Implementation Plan

> 实施时按 Task 顺序推进；每个 Task 独立测试、提交并更新 progress。涉及多模块开发时按 CLAUDE.md 使用 subagent 划分互不重叠的文件范围，由主 agent 统一集成、测试、提交和推送。

**Goal:** 通用通知底座（媒介 trait + SMTP 配置加密 + 发送留痕）与首个场景——员工工资条邮件（工资组成/个税/五险一金明细发送给员工本人）。

**Architecture:** 新增 `notification.rs`（NotifyChannel trait：EmailChannel 用 lettre blocking 实现、SmsChannel 占位；批量发送读数据后释放 DB 锁再逐封网络 IO；每封结果写 notification_logs）。SMTP 授权码 AES-GCM 整键加密入库（复用 OCR token 加密模式）。工资条 HTML 版式复用预览卡片。

**Tech Stack:** Rust + Tauri 2 + rusqlite + lettre（smtp-transport/builder/rustls-tls，blocking）、React 19 + AntD 6。

**Spec:** `docs/superpowers/specs/2026-09-19-stage9-notifications-design.md`（表结构/trait/流程以 spec 3-5 节为准）

## Global Constraints

- 发送流程不持 DB 锁：先读批次数据释放锁，再逐封网络 IO，结果回连写记录（沿发票 OCR 模式）。
- SMTP 授权码不落明文：smtp_config 整键 AES-GCM 加密入库；界面回显脱敏。
- 收件人仅员工本人邮箱，不群发；标题固定 `{YYYY-MM} 工资条`。
- 批量发送单封失败不中断；失败可重发（仅失败者）；取消剩余记 skipped。
- 工资条发送门禁：月份须已锁定；发送须敏感数据解锁态（前端禁用 + 后端 send 命令前置校验双保险）。
- lettre 仅启用 `smtp-transport` + `builder` + `rustls-tls` 特性，blocking 模式，30s 超时/封；不触资金路径。
- 短信仅接口预留，返回中文占位错误，不采集任何短信密钥。
- 中文 UI/错误、snake_case 命令、RFC3339 时间戳、操作人署名。
- 全量回归：`npx tsc -b`、`npm run lint`、`npm run build`、`cd src-tauri && cargo fmt --check`、`cargo check`、`cargo test --lib`（基线 324）。
- `docs/user-guide-v2.html` 为用户未跟踪文件，不得覆盖、删除或纳入提交。

---

## 9A：通知底座与设置页

### Task 1: 9A DDL 迁移与模型

**Files:**
- Modify: `src-tauri/src/db.rs`、`src-tauri/src/models.rs`
- Test: `src-tauri/src/db.rs`

**Interfaces（Produces）:**
- `pub fn migrate_stage9_schema(conn: &Connection) -> AppResult<()>`（挂入 create_tables 迁移链 stage8 之后：`ensure_column(employees, email, TEXT)` + notification_logs 表，DDL 逐字取 spec 3.1/3.2，事务+幂等+FK 检查）
- models.rs：`NotificationLog`、`NotificationStatus` 常量（sent/failed/skipped）、`NotificationChannel` 常量（email/sms）、`SmtpConfig`（host/port/encryption(starttls|ssl|none)/username/password/from_name，Serialize/Deserialize）

- [ ] 迁移测试：幂等（空库/v0.8.0 旧库二次执行）、notification_logs CHECK 约束生效（非法 status/channel 拒绝）、employees.email 列可写。
- [ ] 模型与常量落库。

**Acceptance:** 新旧库迁移幂等；324 基线 + 新增全过。

### Task 2: 通知底座（trait、配置加密、批量骨架）

**Files:**
- Add: `src-tauri/src/notification.rs`（lib.rs 模块注册由本任务完成）
- Modify: `src-tauri/src/models.rs`（如需补充类型）
- Test: `src-tauri/src/notification.rs`

**Interfaces:**
- Consumes: Task 1 表与模型；`security.rs` 加密函数（先看 ocr token / baidu_access_token 的 AES-GCM 加密入库实现，复用同一套 encrypt/decrypt——若其函数私有则 pub(crate) 化，不改算法）
- Produces:
  - `pub trait NotifyChannel { fn name(&self) -> &'static str; fn send(&self, message: &NotifyMessage) -> AppResult<()>; }`
  - `pub struct NotifyMessage { recipient, subject, html_body: Option<String>, text_body: Option<String> }`
  - `SmsChannel`（占位：send 返回 `AppError::General("短信通道未配置，请在设置中接入服务商")`）
  - `pub fn get_smtp_config(conn) -> AppResult<Option<SmtpConfig>>`（含密码）/ `pub fn get_smtp_config_masked(conn)`（密码脱敏 `****`+末 2 位，供命令层）
  - `pub fn set_smtp_config(conn, &SmtpConfig, operator) -> AppResult<()>`（整键 JSON 序列化后 AES-GCM 加密写 app_settings 键 `smtp_config`）
  - `pub fn batch_send(conn, channel: &dyn NotifyChannel, messages: Vec<(i64 employee_id, String recipient, String belong_month, String subject, NotifyMessage-body)>, operator) -> BatchSummary`——读记录连接与发送分离：先写 pending 不可，实现为逐封：channel.send 成功→写 sent，失败→写 failed+error；返回 `BatchSummary { sent, failed, skipped, failed_ids }`；`resend_failed(conn, channel, log_ids, operator)` 仅重发 failed 记录对应者（重建消息由调用方组装器给）
  - `pub fn insert_notification_log / get_notification_logs(conn, query)`（channel/belong_month/status 筛选）

- [ ] 测试（FakeChannel 注入，不真发）：配置加密存取（app_settings 原文为密文断言）、脱敏回显、SmsChannel 占位错误、batch_send 成功/失败双态记录、resend_failed 仅失败者、get_notification_logs 筛选。
- [ ] trait 出口与 lettre 解耦（本任务不引依赖）。

**Acceptance:** 底座全逻辑 FakeChannel 测试通过；零新外部依赖。

### Task 3: EmailChannel、命令与通知设置页

**Files:**
- Modify: `src-tauri/Cargo.toml`（lettre，特性 smtp-transport/builder/rustls-tls，锁定次版本）
- Modify: `src-tauri/src/notification.rs`（EmailChannel：lettre blocking，30s 超时，encryption 映射 starttls/ssl/none；SMTP 535 识别为授权码错误中文提示）
- Modify: `src-tauri/src/commands.rs`、`src-tauri/src/lib.rs`
- Modify: `src/types/index.ts`、`src/api/index.ts`（含 mock）
- Add: `src/pages/NotificationSettings.tsx`
- Modify: `src/App.tsx`、`src/pages/OperationLogs.tsx`

**Interfaces:**
- Produces: 命令 `get_smtp_config`（脱敏）/ `set_smtp_config(config)` / `send_test_email`（收件人=发件账号，走 EmailChannel 真发，结果写 notification_logs belong_month=NULL）/ `get_notification_logs(query)`；get 不记日志、set/send 记日志

- [ ] Cargo.toml 引 lettre 并验证 cargo check 无特性膨胀问题。
- [ ] EmailChannel 实现（535→"授权码错误，请检查邮箱设置（QQ/163 需使用授权码而非登录密码）"）。
- [ ] 设置页：SMTP 表单（预置一键填：QQ smtp.qq.com:465 SSL / 163 smtp.163.com:465 SSL / Outlook smtp.office365.com:587 STARTTLS）、授权码脱敏回显、「发测试邮件给自己」按钮即时反馈、短信只读占位区块、「发送记录」Tab（月份/状态/媒介筛选+失败原因列）。
- [ ] App.tsx 系统设置组 +「通知设置」；OperationLogs 映射；mock（send_test_email mock 返回成功）。

**Acceptance:** 设置页全流程可走（配置→测试→记录可见）；lettre 真发仅由测试按钮触发，不进单测。

### Task 4: 9A 批次收尾

**Files:**
- Add: `docs/superpowers/plans/2026-09-19-stage9-progress.md`（格式照 stage8-progress）
- Add: `.claude/memory/stage9-notifications.md`（骨架）

- [ ] 全量回归六命令过；批次记录（完成项/决策/风险/commit 区间）。
- [ ] Windows 验收清单初版（SMTP 配置→测试发送→记录核对）。

**Acceptance:** 批次可从 progress 完整恢复上下文。

---

## 9B：工资条向导与收尾

### Task 5: 工资条组装器与批量发送

**Files:**
- Modify: `src-tauri/src/notification.rs`
- Modify: `src-tauri/src/commands.rs`、`src-tauri/src/lib.rs`
- Modify: `src/types/index.ts`、`src/api/index.ts`（mock 占位，Task 6 完成前端）
- Test: `src-tauri/src/notification.rs`

**Interfaces:**
- Consumes: Task 2 `batch_send`/`resend_failed`；salary_results/employees 查询（db.rs 既有）
- Produces:
  - `pub fn payslip_html(conn, month, employee_id) -> AppResult<String>`——HTML 版式复用工资条预览卡片（发放项/扣款项/五险一金个人+单位两侧/个税/实发合计，表格行与 SalaryCalculate.tsx payslip 卡片一致）；标题 `{month} 工资条`
  - `pub fn build_payslip_batch(conn, month, employee_ids, operator) -> AppResult<PayslipBatchPlan>`（校验：月份已锁定、SMTP 已配置、敏感解锁态（查 security 模块 reveal 状态接口，若无查询函数则加 `is_sensitive_revealed(conn)`）；返回计划：per-employee recipient/subject/html + 无 email 跳过名单）
  - `pub fn send_payslip_batch(conn, channel: &dyn NotifyChannel, plan, operator) -> BatchSummary`（复用 batch_send；belong_month=month）
  - 命令：`preview_payslip_email(month, employee_id)`（只读，供前端预览 HTML）/ `send_payslip_emails(month, employee_ids)` / `resend_payslip_emails(log_ids)`

- [ ] 测试（FakeChannel）：HTML 组装断言（员工姓名变量、各金额项、两侧五险一金行）；未锁定月份拒；SMTP 未配置拒；敏感未解锁拒；无 email 跳过并计数；邮箱格式非法（不含 @）记 failed"邮箱地址无效"不阻断批量；批量 sent/failed 汇总与记录 belong_month；重发仅失败者。
- [ ] 命令注册+日志映射（preview 不记、send/resend 记，operator 署名）。

**Acceptance:** 组装与批量全逻辑 FakeChannel 测试通过。

### Task 6: 工资条向导前端与员工 email 录入

**Files:**
- Modify: `src/pages/SalaryCalculate.tsx`（「邮件发送工资条」按钮+三步 Modal：①选月勾员工（默认全选有 email、无 email 灰显+"缺邮箱 N 人"提示、未配置 SMTP 拦截引导设置页）②预览（dangerouslySetInnerHTML 渲染 preview_payslip_email，发送按钮须 isSensitiveRevealed 否则禁用+提示）③发送进度+汇总结果+失败勾选重发）
- Modify: `src/pages/Employees.tsx`（表单/详情加 email 字段；导入模板加"邮箱"列；导出清单加列）
- Modify: `src/api/index.ts` mock（向导三步演示）、`src/types/index.ts`
- Modify: `src/pages/OperationLogs.tsx`（Task 5 命令映射若未覆盖）

- [ ] 三步向导全流程 mock 走查（含未配置拦截、敏感门禁禁用态、失败重发）。
- [ ] 员工 email 录入/导入/导出三通道。
- [ ] tsc -b / lint / build 全过。

**Acceptance:** 浏览器 mock 可走完向导；桌面端命令清单齐全。

### Task 7: 全量回归、文档四件套与发版评估

**Files:**
- Modify: `CLAUDE.md`（第九阶段段落+Memory References+架构摘要 notification.rs）
- Modify: `.claude/memory/stage9-notifications.md`（完善为已交付+已知边界+Windows 验收清单）
- Modify: `docs/superpowers/plans/2026-09-19-stage9-progress.md`（9B 完成记录）
- Modify: `docs/user-guide.html`（新增"通知与工资条邮件"卡片：SMTP 配置→测试→向导三步；菜单速查+版本历史）

- [ ] 全量回归六命令 + graphify update。
- [ ] 导航核对（系统设置组 3 项）。
- [ ] 文档四件套；Windows 验收清单固化（配置 QQ 授权码→测试→批量 2 员工→记录→重发）。
- [ ] 发版评估汇报（建议 v0.9.0），经确认走发版流程。

**Acceptance:** 阶段闭环；文档与代码一致。
