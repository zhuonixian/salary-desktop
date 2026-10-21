# 第十四阶段设计：安全债——找回凭据换绑与自动备份

日期：2026-10-21
状态：待审阅
基线：v0.10.0 发版准备已完成（e98ef95，后端 397 测试）

## 1. 背景与目标

stage13 spec 第 5 节挂账的两项安全债（"找回密码命令补全与自动备份，独立排期"）：

1. **找回凭据无法换绑**：恢复码与安全问题在初始化向导设置后永久定死——`change_password` 只重包裹密码 KEK，全代码没有任何换绑命令。恢复码仅显示一次，抄丢后安全问题成为唯一且不可更换的救命绳；两条都丢则 DEK 永久锁死、发票图片不可解。
2. **没有自动备份**：只有手动备份（数据安全中心）+ 恢复前保险快照（auto-before-restore）+ 30 天提醒 nudge（reminders.rs 备份源）。定期自动备份不存在，单机 SQLite 的数据丢失风险面完全依赖用户自觉。

第十四阶段补齐：**换绑命令**（恢复码重发 / 安全问题修改）+ **自动备份**（每日首次解锁后与正式月结后自动产加密备份包，滚动保留）。

零外联不变；月结保护不涉及。

## 2. 找回凭据换绑（14A）

### 2.1 命令

- `rotate_recovery_code(password) -> { recovery_code }`：验证当前启动密码 → 后端生成新恢复码 → Argon2id 派生新 recovery_KEK → 用内存 DEK 重包裹 → 覆盖 security_state 的 recovery 三列 → 明文恢复码**仅本次返回**（前端展示一次）
- `update_security_question(password, new_question, new_answer)`：验证当前启动密码 → 新答案派生 question_KEK → 重包裹 → 覆盖 question 四列（question / q_salt / wrapped / nonce / security_answer_hash）

### 2.2 密钥语义（阻断项）

- 沿用 change_password 的验证模式：旧密码 unwrap 验证 → set_dek → 重包裹目标 KEK；**其余两条 wrap 一律不动**（改恢复码不影响密码/安全问题，反之亦然）
- 换绑成功后 `failed_attempts = 0, lock_until = NULL`（与 change_password 一致：用户已证明密码知识，旧的爆破累计作废）
- 换绑验证失败**不计入** failed_attempts / lock_until（对齐 change_password：登录态操作不污染找回路径的防爆破计数）
- 恢复码格式与向导/SetupSecurity 完全一致：32 字符表（去 O/0/I/1）取 24 位、每 4 位连字符分组；Rust 侧 `rand` 生成（crypto 级 thread_rng）

### 2.3 留痕与门禁

- 操作日志：`security_recovery_code_rotated` / `security_question_updated`，detail 仅时间戳，**不含任何密钥材料与明文答案**
- 前端入口仅安全中心页（登录态），锁屏态不可达；未初始化报错"安全配置未初始化"
- 展示一次的恢复码用 Modal 大字号等宽 + 复制按钮 + "我已抄写保存"确认后才可关闭

## 3. 自动备份（14B）

### 3.1 触发时机（设计决策）

- **每日首次解锁后**：前端解锁成功后 fire-and-forget 调 `run_auto_backup_if_due`（幂等，当日已备则跳过）。不选退出时触发（托盘常驻使"退出"语义模糊）、不选 Rust 定时器（锁屏/未初始化跳过逻辑天然归属解锁动作）
- **正式月结成功后**：月结是数据高点，前端月结成功回调同样调一次（当日去重兜底）
- 执行条件（不满足即静默跳过）：`auto_backup_enabled`（默认开）且账套非空（`onboarding::is_empty_ledger`，空账套零打扰）且安全已初始化且 DEK 已加载

### 3.2 备份产物与保留（设计决策）

- 复用现有 `backup_database(encrypt=true)`：整树打包 + 内存 DEK AES-GCM 加密为单 `.enc` 文件——恢复走既有"解锁后选加密备份包"路径，零新增加密面
- 存放 `app_data_dir/backups/`（与 auto-before-restore 同目录），文件名前缀 `auto-daily-`（区别于手动 salary-backup-）
- 滚动保留最近 `auto_backup_keep_count` 份（默认 5，范围 3-20），超出按文件名时间戳删最旧
- 成功后更新 `last_data_backup_at`（备份提醒 30 天口径随之静默）与 `last_auto_backup_at`（当日去重依据）

### 3.3 命令

- `get_auto_backup_settings() -> { enabled, keep_count, last_run_at, last_run_path }`
- `update_auto_backup_settings(enabled, keep_count)`：keep_count 裁剪到 3-20，留痕
- `run_auto_backup_if_due() -> { ran: bool, reason }`：reason ∈ created / disabled / empty_ledger / not_unlocked / already_today；成功留痕 `auto_backup_created`（detail 不含敏感）

### 3.4 锁与执行

- 在 DB 锁内执行（与手动备份同路径：VACUUM INTO 是在线操作，量级为本地 SQLite，既有手动备份同持锁可接受）；失败不弹窗、只留 console.warn 与操作日志，绝不阻塞解锁流程

## 4. 测试策略

- 换绑：正确密码成功且新恢复码可找回/旧恢复码失效；错误密码拒绝；换绑不影响密码与另一条找回路径（发票可解旁证）；未初始化拒绝；恢复码格式与向导一致（字符表/长度/分组）
- 自动备份：空账套跳过；DEK 未加载跳过；当日第二次跳过；产物为 .enc 且可经既有解密路径解开（BACKUP_MAGIC 头）；超出 keep_count 删旧留新；last_data_backup_at 更新（提醒静默）
- 回归：397 存量测试全绿

## 5. 不做范围（未来增强池）

- 自动备份外置目录/网盘目标（v1 存默认目录，提醒文案继续引导异机转移）
- 备份失败 OS 通知（留痕 + 数据安全页状态即可）
- 恢复码重新展示（生成后仅返回一次，不提供"再看一次"入口——保持与向导一致的"仅显示一次"契约）
- SQLCipher 整库加密、字段级加密（stage4 范围外沿用）
