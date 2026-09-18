# 第九阶段进度同步

本文件用于第九阶段（通知模块与工资条邮件）开发接力。每轮开发结束必须追加记录。

## 当前基线

- 分支：`master`，v0.8.0 已发版
- 阶段计划：`docs/superpowers/plans/2026-09-19-stage9-notifications.md`
- 设计说明：`docs/superpowers/specs/2026-09-19-stage9-notifications-design.md`
- 长期摘要：`.claude/memory/stage9-notifications.md`
- 自动测试基线：9A 收尾 346 passed；前端 tsc -b / lint / build 全过
- 工作区注意：用户未跟踪文件 `docs/user-guide-v2.html` 不得覆盖、删除或纳入提交

## 目标交付

通用通知底座（媒介 trait + SMTP 配置加密 + 发送留痕）、通知设置页、员工工资条邮件（HTML 明细+敏感门禁+批量发送+失败重发）、员工 email 录入。

## 批次状态

| 批次 | 状态 | 说明 |
|---|---|---|
| 9A | 完成 | 底座与设置页（Task 1-4） |
| 9B | 未开始 | 工资条组装器与向导、回归收尾（Task 5-7） |

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
