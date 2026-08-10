# 第四阶段设计：安全配置

- 阶段：第四阶段
- 主题：应用访问安全 + 敏感数据加密 + 数据展示脱敏
- 起草日期：2026-08-10
- 状态：设计稿，待用户复核
- 主线方案：KEK 包裹 DEK 的双层密钥架构（方案 A）

## 1. 背景与目标

第三阶段已交付本地财务管理闭环（数据安全中心、正式月结、付款批次、银行流水匹配、预算异常）。第四阶段聚焦「安全配置」，在保持单机本地、轻量易用的前提下补齐：

1. **应用访问安全**：启动密码、闲置自动锁、手动锁屏、密码找回（恢复码 + 安全问题）。
2. **敏感数据加密**：发票图片、备份包（可选）、OCR token 加密落盘；DB 保持明文。
3. **数据展示脱敏**：身份证号、银行卡号、工资参数与明细、各类业务金额、个人联络信息默认脱敏，点击小眼睛需二次输入启动密码才能解锁明文。

不做范围：

- 多用户权限、在线审批、云端同步。
- 整库加密（SQLCipher）、字段级加密（保留查询能力）。
- 银行接口直连、税务申报集成。
- TPM/SE 硬件密钥绑定。

## 2. 总体架构：双层密钥 KEK + DEK

### 2.1 密钥层级

```
启动密码   ──Argon2id──▶ password_KEK ──┐
恢复码     ──Argon2id──▶ recovery_KEK  ─┤── AES-GCM-256 包裹 ──▶ wrapped_dek（×3 份）
安全问题答案 ──Argon2id──▶ question_KEK  ─┘

随机 DEK (256-bit) ──AES-GCM-256──▶ 发票图片 / 备份包 / OCR token
```

- **DEK**：随机 256-bit AES-GCM-256 数据加密密钥，应用启动后驻留内存（`OnceLock`），不写盘、不落日志。
- **三条 KEK 都包裹同一个 DEK**：改密码或找回密码只需重新包裹 DEK，不动已加密资源。
- **不绑机器**：备份包可跨机恢复（用户在新机输入启动密码即可解开）。

### 2.2 启动 / 解锁数据流

```
App.tsx 启动 → invoke('is_security_initialized')
  ├─ false（首次或迁移）→ SetupSecurity 向导
  │   → setup_security(password, recovery_code, question, answer)
  │   → 后端生成 DEK、派生三条 KEK、包裹 DEK 入库
  │   → 进入主界面（DEK 在内存）
  └─ true → 显示 LockScreen
      → unlock(password)
      → 后端校验 password_hash → 解 wrapped_dek → DEK 入内存
      → 失败：failed_attempts++；5 次错误 lock_until = now + 5min
      → 成功：进入主界面
```

### 2.3 三层密码体系

1. **启动密码**（第一层）：进入应用，≥8 位含字母+数字。
2. **锁屏**（第二层）：闲置 5 分钟（默认，可配置 1/5/15/30 分钟，可关）或手动锁屏 → 重新输启动密码；锁屏时清空敏感数据解锁状态。
3. **敏感数据解锁**（第三层）：默认全应用脱敏；任意位置点小眼睛 → 二次输启动密码 → 全局解锁 5 分钟；过期或锁屏后自动恢复脱敏。

## 3. 后端模块

### 3.1 新增依赖（src-tauri/Cargo.toml）

```toml
argon2 = "0.5"        # Argon2id 密码派生
aes-gcm = "0.10"      # AES-GCM-256 加密
rand = "0.8"          # DEK / nonce 随机源
zeroize = { version = "1", features = ["derive"] }  # 清除内存中敏感数据
hex = "0.4"           # 二进制↔十六进制（DB 字段存储）
```

### 3.2 新模块 `src-tauri/src/security.rs`

职责：密钥派生、DEK 包裹/解包、资源加解密、密码强度校验、安全状态机。

核心 API（概要）：

```rust
pub struct SecurityState {
    inner: OnceLock<Mutex<SecurityInner>>,
}

struct SecurityInner {
    dek: Option<Zeroizing<[u8; 32]>>,  // 启动/解锁后填充；锁屏时清空
    failed_attempts: u32,
    lock_until: Option<DateTime<Utc>>,
}

// 派生与包裹
fn derive_kek(secret: &str, salt: &[u8]) -> Result<[u8; 32]>;
fn wrap_dek(dek: &[u8; 32], kek: &[u8; 32]) -> Result<(Vec<u8>, [u8; 12])>;
fn unwrap_dek(wrapped: &[u8], kek: &[u8; 32], nonce: &[u8; 12]) -> Option<[u8; 32]>;

// 资源加解密
fn encrypt_file(src: &Path, dst: &Path, dek: &[u8; 32]) -> Result<()>;
fn decrypt_file(src: &Path, dst: &Path, dek: &[u8; 32]) -> Result<()>;
fn encrypt_bytes(plain: &[u8], dek: &[u8; 32]) -> Result<(Vec<u8>, [u8; 12])>;
fn decrypt_bytes(cipher: &[u8], nonce: &[u8; 12], dek: &[u8; 32]) -> Result<Vec<u8>>;

// 校验
fn validate_password_strength(p: &str) -> Result<()>;
fn is_initialized(conn: &Connection) -> bool;
```

Argon2id 参数：m_cost = 64 * 1024 KiB（64 MB）、t_cost = 3、p_cost = 1、output = 32 bytes。

AES-GCM-256：每文件/每字节串独立 12 字节随机 nonce，密文与认证 tag 一同存储。

### 3.3 新表与字段

`security_state`（单行，`id = 1`）：

| 字段 | 类型 | 用途 |
|------|------|------|
| `id` | INTEGER PK | 固定为 1 |
| `password_hash` | TEXT | Argon2id 密码校验哈希（独立 salt，不参与 KEK 派生） |
| `password_kek_salt` | TEXT（hex） | 启动密码 KEK 派生 salt |
| `wrapped_dek_by_password` | TEXT（hex） | KEK 包裹的 DEK |
| `wrapped_dek_by_password_nonce` | TEXT（hex） | 包裹用的 nonce |
| `recovery_kek_salt` | TEXT | 恢复码 KEK 派生 salt |
| `wrapped_dek_by_recovery` | TEXT | |
| `wrapped_dek_by_recovery_nonce` | TEXT | |
| `security_question` | TEXT | 安全问题明文（用于显示） |
| `question_kek_salt` | TEXT | 安全问题答案 KEK 派生 salt |
| `wrapped_dek_by_question` | TEXT | |
| `wrapped_dek_by_question_nonce` | TEXT | |
| `security_answer_hash` | TEXT | 安全答案 Argon2id 校验哈希 |
| `idle_timeout_seconds` | INTEGER | 默认 300 |
| `idle_lock_enabled` | INTEGER | 默认 1 |
| `sensitive_reveal_seconds` | INTEGER | 默认 300 |
| `failed_attempts` | INTEGER | 默认 0 |
| `lock_until` | TEXT | 锁定截止时间 |
| `created_at` / `updated_at` | TEXT | RFC3339 |

`invoices` 新增字段：`image_encrypted INTEGER NOT NULL DEFAULT 0`（0=legacy 明文、1=已加密）。

`app_settings` 新增 key：`baidu_access_token_enc`、`baidu_access_token_nonce`；迁移完成后删除旧 `baidu_access_token`。

新表 `legacy_migration_state`（单行）：

| 字段 | 类型 | 用途 |
|------|------|------|
| `id` | INTEGER PK | 固定为 1 |
| `status` | TEXT | `pending` / `in_progress` / `completed` / `failed` |
| `total_invoices` | INTEGER | 待迁移发票数 |
| `processed_invoices` | INTEGER | 已迁移 |
| `token_migrated` | INTEGER | OCR token 是否已加密 |
| `started_at` / `completed_at` | TEXT | |

### 3.4 Tauri 命令（snake_case）

| 命令 | 入参 | 出参 | 说明 |
|------|------|------|------|
| `is_security_initialized` | — | `bool` | 启动分流 |
| `setup_security` | `password, recovery_code, question, answer` | `()` | 首次或迁移初始化 |
| `unlock` | `password` | `UnlockResult` | 启动/锁屏后解锁；返回 `unlocked / failed_attempts / lock_until` |
| `lock` | — | `()` | 手动锁屏：清内存 DEK、清敏感解锁 |
| `get_security_status` | — | `SecurityStatus` | 当前是否初始化/锁定、失败次数、闲置与敏感解锁配置 |
| `change_password` | `old, new` | `()` | 校验 → 用旧密码解 DEK → 新密码派生 KEK → 重新包裹 DEK |
| `reset_password_by_recovery` | `code, new_password` | `()` | 恢复码路径 |
| `reset_password_by_question` | `answer, new_password` | `()` | 安全问题路径 |
| `update_idle_settings` | `enabled, seconds` | `()` | 闲置锁配置 |
| `update_sensitive_reveal_settings` | `seconds` | `()` | 敏感解锁有效期 |
| `reveal_sensitive_data` | `password` | `RevealResult { expires_at }` | 校验密码 → 返回过期时间戳 |
| `get_decrypted_invoice_url` | `invoice_id` | `string` | 解密发票图片到 temp_dir，返回 `convertFileSrc()` URL |
| `get_legacy_migration_status` | — | `MigrationStatus` | 迁移进度 |
| `migrate_legacy_resources` | — | `()` | 手动触发续传（崩溃后） |

所有命令注册到 `lib.rs` 的 `generate_handler!` 列表。

### 3.5 现有模块改造

- **`invoice.rs::save_invoice`**：归档图片时 `encrypt_file` 加密，写入 `invoices.image_encrypted=1`；保留原有 dedup 与归档目录结构（文件名不变，只是内容是密文）。
- **`invoice.rs::query_invoices`** 等：不动查询逻辑；图片预览走 `get_decrypted_invoice_url`。
- **`data_safety.rs`**：
  - `backup_database(target_dir, encrypt: bool)` 加密参数；加密备份整体 zip 后用 DEK AES-GCM 加密为 `.enc`，前 8 字节 magic = `SLRYSFE1`。
  - `restore_database(backup_path)` 按 magic byte 判断是否加密；加密备份需 DEK 在内存（即用户已解锁）才能恢复。
- **`ocr.rs`**：
  - `get_baidu_access_token` 读取 `baidu_access_token_enc` → DEK 解密后返回。
  - `set_baidu_access_token` 写入前 DEK 加密。
  - 110 错误清缓存逻辑不变，只是删除 `baidu_access_token_enc` 与 nonce。
- **`commands.rs`**：12+ 个新命令薄薄一层转发到 `security.rs`；现有 OCR/发票命令不变。

### 3.6 临时文件与清理

- 解密发票图片目录：`{temp_dir}/salary-desktop-preview/`。
- 文件名：`{invoice_id}_{timestamp}.jpg`。
- 清理时机：
  - 锁屏触发时清空整个目录。
  - 应用退出时（Tauri `RunEvent::Exit`）清空。
  - 单次预览 30 分钟后异步清理（避免目录无限增长）。

## 4. 前端模块

### 4.1 新页面 `src/pages/SecurityCenter.tsx`

- 安全状态卡片：当前是否初始化、上次改密时间、失败尝试次数、敏感解锁剩余时间。
- 改密码表单（旧密码 + 新密码 + 确认 + 强度提示）。
- 找回密码向导（两个 Tab：恢复码 / 安全问题）。
- 闲置锁定配置（开关 + 1/5/15/30 分钟单选）。
- 敏感解锁时长配置（1/5/15/30 分钟）。
- 锁屏按钮（即时锁定）。
- 迁移状态卡片（仅迁移用户可见；显示进度条）。

### 4.2 新组件

**`SetupSecurity.tsx`**：4 步向导
1. 设置启动密码 + 确认 + 强度提示。
2. 系统生成 24 字符恢复码（base32 编码，分 4 段）+ 强制勾选「我已抄写保存」。
3. 选择安全问题（下拉 5 选 1）+ 输入答案。
4. 确认完成 → 调 `setup_security`。

**`LockScreen.tsx`**：全屏 Modal
- `maskClosable=false`、`keyboard=false`（Esc 不关）、无 close 按钮。
- 密码输入框 + 解锁按钮 + 倒计时（锁定中）。
- 错误提示 + 剩余尝试次数。
- 小字「忘记密码？」链接 → 找回 Modal（恢复码 / 安全问题）。

**`SensitiveText.tsx`**：通用脱敏
- props：`type: 'id_card' | 'bank_card' | 'amount' | 'phone' | 'address' | 'raw'`、`value: string`、`revealable?: boolean = true`。
- 未解锁状态：显示脱敏 + 灰色 EyeOutlined 图标。
- 点击 EyeOutlined → 弹出二次密码 Modal → `revealSensitive(password)` → 全局解锁。
- 已解锁状态：显示明文 + EyeInvisibleOutlined 图标（点击收起，仅本组件收起；全局仍保持解锁）。
- 切页保持解锁；锁屏或解锁到期后自动恢复脱敏。

脱敏格式：

| type | 格式 |
|------|------|
| `id_card` | `110101********1234` |
| `bank_card` | `6225 **** **** 1234` |
| `amount` | `¥ ****` |
| `phone` | `138****1234` |
| `address` | `北京市朝阳区***` |
| `raw` | 默认 `****`，可自定义 mask 函数 |

### 4.3 React Context：`SecurityContext`

```ts
interface SecurityContextValue {
  isInitialized: boolean;
  isLocked: boolean;                       // 启动锁/锁屏状态
  isSensitiveRevealed: boolean;            // 敏感数据解锁状态
  sensitiveRevealExpiresAt: number | null;
  idleTimeoutSeconds: number;
  idleLockEnabled: boolean;
  unlock(password: string): Promise<void>;
  lock(): Promise<void>;
  revealSensitive(password: string): Promise<void>;
  clearSensitiveReveal(): void;
  refreshStatus(): Promise<void>;
}
```

提供者挂在 `App.tsx` 顶层，所有 page / component 可消费。

### 4.4 App.tsx 改造

```tsx
useEffect(() => {
  invoke('is_security_initialized').then(init => {
    setInitialized(init);
    if (init) setLocked(true);  // 启动后强制走一次解锁流程
  });
}, []);

useEffect(() => {
  if (!idleLockEnabled || !isLocked === false) return;
  let timer: number;
  const reset = () => {
    clearTimeout(timer);
    timer = window.setTimeout(() => lock(), idleTimeoutSeconds * 1000);
  };
  window.addEventListener('mousemove', reset);
  window.addEventListener('keydown', reset);
  window.addEventListener('click', reset);
  window.addEventListener('scroll', reset, true);
  reset();
  return () => {
    clearTimeout(timer);
    window.removeEventListener('mousemove', reset);
    window.removeEventListener('keydown', reset);
    window.removeEventListener('click', reset);
    window.removeEventListener('scroll', reset, true);
  };
}, [idleLockEnabled, idleTimeoutSeconds, isLocked]);
```

路由守卫：未解锁时 `<Routes>` 不渲染业务页面，仅渲染 `<LockScreen/>`。

### 4.5 脱敏改造（按字段映射）

| 页面 | 字段 | type |
|------|------|------|
| `Employees.tsx` | 身份证号、银行卡号、开户行、基本工资、岗位工资、补贴扣款项、手机号、住址、紧急联系人 | 多种 |
| `SalaryCalculate.tsx` | 应发、社保、公积金、个税、实发、调整项 | `amount` |
| `SalaryRules.tsx` | 起征点、税率表金额、扣款标准 | `amount` |
| `Reimbursements.tsx` | 报销金额、付款金额 | `amount` |
| `Payments.tsx` | 付款金额、收款账号、开户行 | `amount` / `bank_card` |
| `BankTransactions.tsx` | 收入、支出、余额、对方账号 | `amount` / `bank_card` |
| `FinancialAnalysis.tsx` | 预算金额、实际发生、执行率分子分母 | `amount` |
| `Invoices.tsx` | 价税合计、金额、税额 | `amount` |
| `Dashboard.tsx` | 应发合计、实发合计等汇总卡片 | `amount` |
| `OperationLogs.tsx` | 不脱敏（审计需要） | — |

详情抽屉、统计卡片、图表 tooltip、Excel 预览均复用 `SensitiveText`。

**Excel 导出不脱敏**：出纳对账需真实数据；如需脱敏导出可后续作为 P2 增强。

## 5. 错误处理

| 场景 | 文案 | 限制 |
|------|------|------|
| 密码错误 | 「密码错误，剩余 N 次尝试」 | 5 次失败 → lock_until = now + 5 分钟 |
| 锁定中 | 「尝试过多，请于 HH:mm 后重试」 | 倒计时显示 |
| 恢复码错误 | 「恢复码不正确」 | 3 次失败 → lock_until = now + 15 分钟 |
| 安全问题答案错误 | 「答案不正确」 | 3 次失败 → lock_until = now + 15 分钟 |
| 密码强度不足 | 「密码至少 8 位且同时包含字母和数字」 | 前端 + 后端双校验 |
| wrapped_dek 解包失败 | 「安全数据损坏，请从备份恢复」 | 安全失败 |
| 临时解密目录不可写 | 「无法生成预览，请检查磁盘空间」 | |
| 加密备份恢复但 DEK 未加载 | 「请先输入启动密码解锁应用」 | |
| 旧版明文备份恢复 | 「检测到旧版备份，将以明文方式恢复」 | 自动判断 |
| 敏感解锁密码错误 | 「密码错误，无法查看敏感数据」 | 复用 `failed_attempts`，5 次后联动锁定 |
| 未解锁调用 `get_decrypted_invoice_url` | 「请先解锁应用」 | |
| DEK 未加载调用加密相关命令 | 「请先解锁应用」 | |

错误次数与锁定状态写在 `security_state` 单行；敏感解锁失败次数复用 `failed_attempts`（避免绕过锁定）。

## 6. 测试

### 6.1 Rust 单元测试（约 15-20 个）

- Argon2id 派生确定性（同输入同输出）。
- DEK wrap/unwrap 对称性。
- 文件加解密对称性（含大文件、空文件、二进制）。
- 字节加解密对称性。
- 密码强度校验规则（短、缺字母、缺数字、合规）。
- 错误次数计数与 `lock_until` 计算。
- 错误密码 unwrap 返回 None（不抛 panic）。
- magic byte 检测加密备份。
- 脱敏格式化函数（id_card / bank_card / amount / phone / address）。

### 6.2 Rust 集成测试（约 8-10 个）

- 完整流程：`setup_security` → `unlock` → `change_password` → 新密码 `unlock`。
- 恢复码重置 → 新密码解锁。
- 安全问题重置 → 新密码解锁。
- 5 次密码错误 → 锁定 → 等待 → 解锁。
- 旧版迁移：明文发票 + 明文 token → 加密迁移。
- 加密备份 → 跨机恢复（新 setup + 解密恢复）。
- 发票加密归档 → 解密预览。
- 锁定中调用 `unlock` 拒绝。

### 6.3 前端

- `npx tsc --noEmit` 通过。
- `npm run lint` 通过。
- `npm run build` 通过（Vite chunk 体积提示可保留既有 warning）。
- LockScreen 渲染与错误状态。
- SensitiveText 渲染（脱敏/解锁/收起）。
- SecurityContext 状态机。

### 6.4 手工验收（Windows exe）

- 启动 → 初始化向导 → 关闭 → 重启 → 解锁 → 主界面。
- 闲置 5 分钟自动锁；手动锁屏按钮。
- 发票图片正常显示（加密归档 + 解密预览）。
- 改密码后发票仍可看（DEK 不变）。
- 忘记密码 → 恢复码 → 新密码 → 解锁。
- 忘记密码 + 丢失恢复码 → 安全问题 → 新密码 → 解锁。
- 加密备份 → 在另一台机器恢复成功。
- 不加密备份 → 恢复成功（明文兼容）。
- 旧版升级：旧 db（含发票、OCR token）→ 装新版 → 看迁移进度 → 完成后所有功能正常。
- 列表页脱敏：员工、工资核算、报销、付款、银行流水、财务分析、发票、Dashboard。
- 点小眼睛 → 二次密码 → 明文；切页保持解锁；5 分钟到期恢复脱敏；锁屏后恢复脱敏。
- 5 次密码错误锁定；3 次恢复码错误锁定。

## 7. 旧版迁移流程

检测：db 中 `security_state` 表无记录 + 已有 invoices/employees 数据。

流程：

1. 启动跳过 LockScreen，直接进入 `SetupSecurity` 向导（标题改为「为已有数据设置访问密码」）。
2. 完成密码设置后：
   - 创建 `legacy_migration_state (status='in_progress')`。
   - 后台线程启动迁移：
     - 遍历 `invoices` 所有 `image_encrypted=0` 的发票：读取明文文件 → DEK 加密 → 覆盖原文件 → 更新 `image_encrypted=1` → `processed_invoices++`。
     - 加密 OCR token（如存在），删除旧明文 token。
     - 完成后 `status='completed'`。
3. 前端显示进度条，禁止关闭应用（拦截 close 事件 + 弹窗提示）。
4. 迁移完成 → 进入主界面。
5. 中途崩溃 → 重启后检测 `status='in_progress'` → 弹「上次迁移未完成，是否继续」→ 续传。

迁移文件就地覆盖策略：先写入 `{filename}.tmp`（加密内容）→ 校验大小 → 重命名替换 → 删除原文件。崩溃时 `.tmp` 残留可识别。

## 8. 安全考量

- **密码派生**：Argon2id（m=64MB, t=3, p=1），抗 GPU/ASIC 暴力。
- **对称加密**：AES-GCM-256，每文件/字节串独立 12 字节随机 nonce，密文+认证 tag 一同存储；防篡改。
- **内存中的 DEK**：用 `Zeroizing<[u8; 32]>` 包装，锁屏与退出时主动 zeroize；不写盘、不输出日志。
- **密码校验哈希独立**：`password_hash` 用独立 salt 派生（不参与 KEK），避免通过 password_hash 反推 KEK。
- **失败次数限制**：密码 5/5min、恢复码 3/15min、安全问题 3/15min。
- **临时文件清理**：`{temp_dir}/salary-desktop-preview/` 在锁屏与退出时清空。
- **操作日志**：记录 `unlock_success` / `unlock_failed` / `change_password` / `reset_by_recovery` / `reset_by_question` / `lock` / `reveal_sensitive`；不记录密码、恢复码、答案本身。
- **Tauri 加固**（建议同步进行，作为本阶段附带项）：
  - `tauri.conf.json` 中 `security.csp` 设为严格 CSP（仅允许 self 与 `tauri://` 协议）。
  - `assetProtocol.scope` 收紧到 `$APPDATA/**` 与 `$TEMP/salary-desktop-preview/**`。
  - 生产构建关闭 devtools（`devtools = false`）。

## 9. 不在第四阶段范围

以下作为后续阶段或 P2 增强池：

- 整库加密（SQLCipher 替代 rusqlite）。
- 字段级加密（身份证号、银行卡号列加密存储）。
- TPM / SmartCard / YubiKey 硬件密钥绑定。
- 生物识别（指纹、人脸）解锁。
- 自动备份到云盘（OneDrive / 坚果云 WebDAV）。
- 加密导出 Excel（带密码保护的 .xlsx）。
- 操作日志哈希链防篡改。

## 10. 交付与验收门槛

第四阶段按用户要求「一批交付」。

回归门槛（开发完成后必须通过）：

```bash
npx tsc --noEmit
npm run lint
npm run build
cd src-tauri && cargo fmt --check
cd src-tauri && cargo check
cd src-tauri && cargo test --lib
npm run tauri build
```

提交建议（多 commit 拆分）：

- `feat(security): 新增 KEK+DEK 双层密钥与启动密码`
- `feat(security): 新增锁屏与闲置自动锁`
- `feat(security): 新增敏感数据脱敏与二次密码解锁`
- `feat(security): 新增发票图片与 OCR token 加密`
- `feat(security): 新增加密备份包`
- `feat(security): 新增旧版数据迁移`
- `feat(security): 新增 Tauri 安全配置收紧`

发版 tag：`v0.4.0 feat: 第四阶段安全配置`。

## 11. Subagent 协作建议

第四阶段涉及多模块，建议按文件不重叠拆分：

- **planner-security-core**：`src-tauri/src/security.rs` + `Cargo.toml` 新依赖 + `db.rs` schema 迁移 + 单元测试。
- **planner-security-command**：`src-tauri/src/commands.rs` 新命令 + `lib.rs` 注册 + 集成测试。
- **planner-security-resource**：`src-tauri/src/invoice.rs` / `data_safety.rs` / `ocr.rs` 改造。
- **planner-security-ui**：`src/pages/SecurityCenter.tsx` + `src/components/LockScreen.tsx` / `SetupSecurity.tsx` / `SensitiveText.tsx` + `SecurityContext`。
- **planner-security-mask**：所有 page 的脱敏改造（不重叠模块）。
- **planner-security-migration**：旧版迁移后端 + 前端进度。
- **planner-security-hardening**：`src-tauri/tauri.conf.json` CSP / assetProtocol scope / devtools 收紧，单独 PR 验证不影响现有功能。
- 主 agent 负责合并、统一测试、commit、push。

每个 subagent 必须读 `CLAUDE.md` + 本 spec + `docs/superpowers/plans/2026-08-10-stage3-progress.md`。

## 12. 风险与缓解

| 风险 | 影响 | 缓解 |
|------|------|------|
| 用户同时忘记密码 + 丢失恢复码 + 忘记安全问题答案 | 数据永久丢失 | UI 强制勾选「我已抄写保存恢复码」；SecurityCenter 反复提示抄写位置；加密备份包可独立用 DEK 解（如设置了独立备份密码，可后续作为增强） |
| 迁移中途崩溃 | 部分发票加密、部分明文 | `legacy_migration_state.status` 跟踪；`.tmp` 文件标记；续传 |
| Argon2id 性能（64MB）启动慢 | 启动延迟 0.5-1s | 可接受范围；如太慢降级 m=32MB |
| 锁屏时发票预览临时文件残留 | 信息泄露 | 锁屏清空 temp_dir |
| 改密后旧 `wrapped_dek` 残留 | 数据库取证攻击 | 改密时 overwrite 旧字段（不删除行，UPDATE 同列） |
| Excel 导出包含明文敏感数据 | 出纳主动导出后泄露 | 不在第四阶段范围；用户文档说明导出文件需自行保管 |
