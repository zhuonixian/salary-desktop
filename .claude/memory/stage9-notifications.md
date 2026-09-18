---
name: stage9-notifications
description: 第九阶段（通知模块：SMTP 工资条邮件）设计要点、进度与已知边界
---

# 第九阶段：通知模块（进行中）

设计：`docs/superpowers/specs/2026-09-19-stage9-notifications-design.md`

计划：`docs/superpowers/plans/2026-09-19-stage9-notifications.md`（9A/9B 共 7 任务）

进度：`docs/superpowers/plans/2026-09-19-stage9-progress.md`

## 关键口径（阻断级）

- 发送不持 DB 锁：命令层锁内取数 drop 后用独立 `Connection::open(app_data_dir/salary.db)` 发送+写记录（WAL 并存，同发票 OCR 精神）
- smtp_config 整键 AES-GCM 加密入库；set/get 带 `sec: &SecurityState`（DEK 来源）；界面回显脱敏+留空不修改防呆（`****` 开头判定掩码）
- 批量单封失败不中断；resend 仅 failed 记录且渠道匹配；**留痕无正文——工资条重发须组装层重建 MessageItem**
- lettre 0.11 仅 smtp-transport/builder/rustls-tls 特性，blocking，30s 超时；535→"授权码错误"中文提示
- 短信仅接口预留占位错误，不采集密钥

## 状态

- 9A 已交付：底座（trait/加密/批量骨架）+ EmailChannel + 通知设置页（预设/测试发送/记录 Tab），346 测试
- 9B 待做：工资条组装器与向导（Task 5-7）
