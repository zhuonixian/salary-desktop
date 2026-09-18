---
name: stage9-notifications
description: 第九阶段（通知模块：SMTP 工资条邮件）已交付能力、关键口径、已知边界与 Windows 验收清单
---

# 第九阶段：通知模块（已交付）

设计：`docs/superpowers/specs/2026-09-19-stage9-notifications-design.md`

计划：`docs/superpowers/plans/2026-09-19-stage9-notifications.md`（9A/9B 共 7 任务）

进度：`docs/superpowers/plans/2026-09-19-stage9-progress.md`

## 已交付能力（v0.9.0 待发版）

- **通知底座**（`notification.rs`）：`NotifyChannel` trait（email/sms，短信为占位错误不采集密钥）、`NotifyMessage`、`notification_logs` 留痕表（sent/failed/skipped + 失败原因 + 操作人）
- **SMTP 配置加密**：`app_settings.smtp_config` 整键 JSON 序列化后 AES-GCM 加密入库；set/get 带 `sec: &SecurityState`（DEK 来源）；操作留痕 detail 仅含 host/port/encryption 不含授权码
- **EmailChannel**：lettre 0.11（仅 smtp-transport/builder/rustls-tls 特性，blocking），30s 超时/封；535 → "授权码错误，请检查邮箱设置（QQ/163 需使用授权码而非登录密码）"
- **通知设置页**（`NotificationSettings.tsx`，菜单 系统设置→通知设置）：QQ/163/Outlook 预设一键填、授权码回显脱敏（`****`+末 2 位，留空不修改防呆——`****` 开头判定为掩码）、测试发送（收件人=发件账号，成败均写 notification_logs）、发送记录 Tab（月份/状态/媒介筛选）
- **工资条邮件三步向导**（工资计算页「邮件发送工资条」，`SalaryCalculate.tsx`）：①选员工（仅已锁定月份，默认全选有 email 者，缺邮箱灰显列"缺邮箱 N 人"）→ ②预览 HTML 明细（发放项/扣款项/五险一金个人+单位两侧/个税/实发，须敏感解锁态）→ ③发送结果（逐封进度、失败勾选重发）
- **员工 email 全链路**：表单录入 + 导入模板列 + 导出清单列（`excel.rs`/`db.rs`）
- **命令层**：`get/set_smtp_config`、`send_test_email`、`get_notification_logs`、`preview_payslip_email`、`send_payslip_emails`、`resend_payslip_emails`；工资条命令传 `employees.id`（非日志 id）防错发

## 关键口径（阻断级）

- **授权码不落明文**：整键 AES-GCM 加密入库；前端只拿脱敏回显；操作留痕 detail 不含授权码
- **发送不持 DB 锁**：命令层锁内仅取数（配置解密/组计划/操作人/db 目录），drop 守卫后用独立 `Connection::open(salary.db)` 发送+写留痕（WAL 并存，同发票 OCR 先例）；第二连接未设 busy_timeout（WAL 下可忽略）
- **三重门禁**（build_payslip_batch 前置校验）：月份已锁定 + SMTP 已配置 + 敏感数据解锁；发送前复核解锁态（构建与发送之间可能锁屏/到期）
- 批量单封失败不中断；空收件地址记 skipped「邮箱地址无效」；重复 id 去重
- 重发仅 failed 记录且渠道匹配（其余计入 skipped）；**留痕无正文**——重发按 employee_id 重新组装 HTML

## 已知边界（Ruling 与口径）

- **发送中取消留档不修**（Task 6 Ruling）：spec 5"发送中可取消"与单次 invoke 批处理架构冲突、批量为分钟级、失败重发已覆盖补发场景，成本不成比例——发送期间向导关闭/按钮禁用防误操作，大批量中途不可停；若后续大批量需求出现，走后台任务化
- **resend 以当前 email 为准**：重发按 employees.email 当前值重组收件人（首轮发送后可能已改邮箱），recipient 以重建时为准
- **重启后需重新解锁敏感态**：DEK 在内存，应用重启后敏感态归零，发送工资条前须重新解锁（三重门禁兜底）
- 短信通道仅接口预留占位错误；定时发送、PDF 附件、模板自定义、企业微信/钉钉媒介均在范围外（spec 9）

## OperationLogs 对账（与 commands.rs/notification.rs 写入键一致）

- `smtp_config_update`（set_smtp_config）
- `payslip_email_batch_send`（build_payslip_batch，detail=月份+人数统计不含工资数字）
- `payslip_email_resend`（resend_payslip_batch）
- `send_test_email` 只写 notification_logs（通知设置页发送记录 Tab），不写 operation_logs——映射无需新增

## Windows exe 手工验收清单（挂账）

1. **配置 QQ 授权码**：QQ 邮箱设置→账户→开启 SMTP 得授权码 → 通知设置→QQ 预设一键填→填账号+授权码+发件人显示名→保存（回显应脱敏）
2. **测试发送**：点「发测试邮件给自己」→ 收件箱收到《工资条邮件发送测试》→ 通知设置发送记录出现 sent
3. **批量 2 员工**：员工管理给 2 名员工填 email → 工资计算页锁定月份 →「邮件发送工资条」→ 勾选 2 人→预览→发送 → 两封明细邮件到达
4. **记录核对**：向导结果页 2 sent；操作日志出现 payslip_email_batch_send（detail 不含工资数字）；通知设置发送记录 2 条
5. **重发**：改错一个邮箱使其发送失败（或断网重试）→ 结果页失败记录勾选重发 → 仅失败者重建重发成功；若期间改过邮箱，收件人按新邮箱

## Minor 留档（终审 triage 全部不阻断）

第二连接无 busy_timeout（WAL 可忽略，可一行加固）；真授权码 **** 开头误判掩码（概率趋零）；发送中不可取消（Ruling 见上）+长批次 UI 等待上限 N×30s 同源；重发以当前 email 为准；EmployeeInput.email 无 serde(default)（调用方已适配）；选中员工无工资结果整体报错（前端只列有结果者）；payslip 不含 other_allowance 行（并入应发照卡片）；535 文本兜底 contains 误命中（后果仅提示友好）；EHLO 默认 localhost（主流接受）；mock 未覆盖 SMTP 未配置路径；month 入口可加 `^\d{4}-\d{2}$` 正则加固（纯防御）
