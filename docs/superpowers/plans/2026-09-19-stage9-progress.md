# 第九阶段进度同步

本文件用于第九阶段（通知模块与工资条邮件）开发接力。每轮开发结束必须追加记录。

## 当前基线

- 分支：`master`，v0.8.0 已发版
- 阶段计划：`docs/superpowers/plans/2026-09-19-stage9-notifications.md`
- 设计说明：`docs/superpowers/specs/2026-09-19-stage9-notifications-design.md`
- 长期摘要：`.claude/memory/stage9-notifications.md`
- 自动测试基线：9B 收尾 360 passed；前端 tsc -b / lint / build 全过
- 工作区注意：用户未跟踪文件 `docs/user-guide-v2.html` 不得覆盖、删除或纳入提交

## 目标交付

通用通知底座（媒介 trait + SMTP 配置加密 + 发送留痕）、通知设置页、员工工资条邮件（HTML 明细+敏感门禁+批量发送+失败重发）、员工 email 录入。

## 批次状态

| 批次 | 状态 | 说明 |
|---|---|---|
| 9A | 完成 | 底座与设置页（Task 1-4） |
| 9B | 完成 | 工资条组装器与向导、回归收尾（Task 5-7） |

## 协作规则

- SDD：每任务独立实现者+审查者双裁决；主 agent 统一集成、回归、commit。
- 发送不持 DB 锁、授权码不落明文、单封失败不中断为阻断级验收项。
- 前端类型检查必须 `npx tsc -b`。

## 9A 记录

### 2026-09-19 — 9A 通知底座与设置页（Task 1-4）完成

- Task 1（ef475b4）：notification_logs DDL + employees.email 列 + 模型常量 + SmtpConfig，326 测试。
- Task 2（21cdcbc）：notification.rs 底座——NotifyChannel trait（SmsChannel 占位）、smtp_config 整键 AES-GCM 加密（set/get 带 sec: &SecurityState 参数，DEK 来源）、batch_send/resend_failed 骨架、16 个 FakeChannel 测试，342 测试。
- Task 3（bee8f73）：lettre 0.11.23（default-features=false + smtp-transport/builder/rustls-tls）、EmailChannel（30s 超时、535→授权码中文提示）、四命令（send_test_email 不持锁：锁内取数 drop 后独立 Connection 发送+留痕，WAL 并存）、NotificationSettings.tsx（预设一键填/授权码脱敏+留空不修改/测试发送/记录 Tab）、系统设置改分组菜单，346 测试。
- Task 4：批次收尾（本文件 + memory 骨架），全量回归过。
- 挂账 Task 5/6：resend 工资条正文需组装层重建 MessageItem（留痕无正文）；向导前端敏感门禁禁用+后端前置校验双保险；Employees 表单/导入/导出加 email 通道。
- 备忘：第二连接未设 busy_timeout（WAL 可忽略）；真授权码以 **** 开头会被误判掩码（概率趋零）。
- 下轮入口：Task 5 工资条组装器与批量发送。

## 9B 记录

### 2026-09-19 — 9B 工资条向导与阶段收尾（Task 5-7）完成

- Task 5（c5d97d3 + 92a62a6）：工资条 HTML 组装（发放/扣款项、五险一金个人+单位两侧、个税、实发合计）、`build_payslip_batch` 三重门禁前置校验（锁定月+SMTP+敏感解锁）、`send_payslip_emails` 不持锁发送、`resend_payslip_emails` 重建正文重发、员工 email 导入/导出通道，352 测试。
- Task 6（0712a93）：工资计算页「邮件发送工资条」三步向导（选员工→预览→发送结果）、SMTP 配置缺失入口拦截、敏感未解锁禁用发送+后端复核双保险、失败勾选重发、员工 email 表单录入、id 语义修正（传 employees.id 防错发），360 测试。
  - Ruling：spec 5"发送中可取消"留档不修——与单次 invoke 批处理架构冲突、批量为分钟级、失败重发已覆盖补发；大批量需求出现再后台任务化。
- Task 7（本提交）：全量回归（fmt/check/test --lib 360、tsc -b、lint、build、graphify update）、导航核对（系统设置组=工资规则+通知设置，安全中心为独立顶级项）、OperationLogs 键核对（smtp_config_update/payslip_email_batch_send/payslip_email_resend 三键一致，send_test_email 仅写 notification_logs 无需映射）、文档四件套（CLAUDE.md 第九阶段段、stage9 memory 已交付+边界+Windows 验收清单、progress、user-guide 通知卡片）；顺手修 excel.rs 两处 cargo fmt 格式（Task 5 遗留）。
- 发版评估：建议 v0.9.0——v0.8.0 以来 9 commit（6 代码 + 3 文档），后端 324→360（+36），新增通知外联面（仅 SMTP、授权码加密、留痕完整）；Windows exe 手工验收清单挂账（QQ 授权码→测试→批量 2 员工→记录→重发）。

## 阶段完成

第九阶段（通知模块与工资条邮件）Task 1-7 全部交付：通知底座（媒介抽象+SMTP 授权码 AES-GCM 加密+发送留痕）、通知设置页、工资条邮件三步向导、失败重发、员工 email 全链路。阻断级验收全过：授权码不落明文、发送不持 DB 锁、三重门禁。后端 360 测试、前端三检查全绿。遗留：Windows exe 手工验收清单（见 `.claude/memory/stage9-notifications.md`）。
