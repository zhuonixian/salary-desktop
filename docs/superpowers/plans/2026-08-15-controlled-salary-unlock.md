# 受控解锁已锁定工资 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 工资锁定线开受控口子：启动密码 + 必填原因 + 强警告 + 完整审计日志的"受控解锁 → 修改 → 重新锁定"。

**Architecture:** 复用第四阶段 `security::unlock` 密码验证（失败计数与锁屏共享）与第五阶段 `db::unlock_salary_results`（同事务 void 计提凭证）；命令放在 `security_commands.rs`（有 `State<SecurityState>` 与 `lock_conn` 基建），核心逻辑抽成可单测的 `*_impl` 函数；重新锁定复用现有 `lock_salary_results` 命令。

**Tech Stack:** Tauri 2 + rusqlite + Argon2id（既有）；前端 React 19 + antd 6。

**Spec:** `docs/superpowers/specs/2026-08-15-controlled-salary-unlock-design.md`（注意：spec 2.1 写"commands.rs security 分区"，实现落位为其等价物 `security_commands.rs`——安全命令的既有聚集地，spec 此处按落位事实理解）

## Global Constraints

- 中文注释、中文 commit message、中文错误提示
- Tauri 命令 snake_case，前端 `invoke('snake_case_name')` 顶层参数 camelCase 自动映射
- 时间戳 `Utc::now().to_rfc3339()`；操作日志 `db::log_operation(conn, op_type, description, operator, Option<detail>)`
- 不跳过 hooks；测试用 `Connection::open_in_memory()`
- 保护线不动：正式月结月（`ensure_month_open`）、付款批次 guard（`active_payment_item_exists`）
- 原因门槛：`reason.trim()` 长度 ≥ 5

## 文件结构

| 文件 | 职责 |
|---|---|
| `src-tauri/src/db.rs`（改） | `unlock_salary_results` 返回值 bool→usize、0 行报错 |
| `src-tauri/src/security_commands.rs`（改） | 新命令 + 可单测 impl + 测试 |
| `src-tauri/src/lib.rs`（改） | 注册命令 |
| `src/api/index.ts`、`src/pages/SalaryCalculate.tsx`、`src/pages/OperationLogs.tsx`（改） | 前端接线与交互 |
| `.claude/memory/stage5-accounting.md`（改） | 已知边界更新（unlock 已暴露） |

3 个任务：Task 1 db 层 → Task 2 命令层 → Task 3 前端 + 回归 + 文档。

---

### Task 1: db 层签名调整

**Files:**
- Modify: `src-tauri/src/db.rs:1472-1485`（`unlock_salary_results`）
- Test: `src-tauri/src/db.rs` / `src-tauri/src/accounting.rs` 既有调用点（grep 定位）

**Interfaces:**
- Produces: `db::unlock_salary_results(&Connection, &str) -> AppResult<usize>`（返回作废凭证数；UPDATE 影响 0 行返回 `Err(AppError::InvalidParam("该月没有已锁定的工资结果"))`；其余逻辑不变：ensure_month_open + 解锁 UPDATE + void salary_accrual 同事务）

- [ ] **Step 1: 改造函数（含失败测试先行）**

先在 `accounting.rs` 测试模块追加（数据构造参考现有 `test_salary_accrual_voucher` 的工资行 INSERT）：

```rust
#[test]
fn test_unlock_salary_results_no_locked() {
    let conn = setup(); // 既有 helper：create_tables + seed_gl_accounts
    let err = db::unlock_salary_results(&conn, "2026-08").unwrap_err();
    assert!(err.to_string().contains("没有已锁定"));
}
```

运行确认失败：

```bash
cd src-tauri && cargo test --lib test_unlock_salary_results_no_locked
# Expected: FAIL —— 当前返回 Ok(false)，unwrap_err 会 panic
```

再改 `db.rs:1472` 实现：

```rust
pub fn unlock_salary_results(conn: &Connection, month: &str) -> AppResult<usize> {
    ensure_month_open(conn, month)?;
    // 解锁 UPDATE 与计提凭证作废放在同一事务：任一失败整体回滚
    let tx = conn.unchecked_transaction()?;
    let updated = tx.execute(
        "UPDATE salary_monthly_results SET locked = 0, status = 'reviewed', updated_at = ?1 WHERE salary_month = ?2 AND locked = 1",
        params![Utc::now().to_rfc3339(), month],
    )?;
    if updated == 0 {
        return Err(AppError::InvalidParam("该月没有已锁定的工资结果".into()));
    }
    let voided = crate::accounting::void_salary_accrual_vouchers(&tx, month)?;
    tx.commit()?;
    Ok(voided)
}
```

注意：`updated == 0` 时直接 return，`tx` drop 自动回滚（无写入，无副作用）。

- [ ] **Step 2: 修复既有调用点**

`grep -rn "unlock_salary_results" src-tauri/src/` 找到测试中的旧调用（返回值若被当 bool 用则改掉：`db::unlock_salary_results(&conn, "2026-08").unwrap();` 直接丢弃 usize 即可；若断言 `== true` 改为 `.is_ok()`）。

- [ ] **Step 3: 全量验证**

```bash
cd src-tauri && cargo test --lib && cargo fmt
# Expected: 全部通过（约 123 个，含新增 1 个）
```

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/db.rs src-tauri/src/accounting.rs
git commit -m "feat(unlock): unlock_salary_results 返回作废凭证数并校验无锁定"
```

### Task 2: 受控解锁命令

**Files:**
- Modify: `src-tauri/src/security_commands.rs`（新命令 + impl + 测试）、`src-tauri/src/lib.rs`（注册）
- Test: `src-tauri/src/security_commands.rs` 测试模块

**Interfaces:**
- Consumes: Task 1 的 `db::unlock_salary_results -> AppResult<usize>`；既有 `security::unlock(&Connection, &SecurityState, &str) -> AppResult<UnlockResult>`（字段 `unlocked: bool, failed_attempts: i64`）、`db::log_operation`、`db::ensure_month_open`
- Produces: 命令 `unlock_salary_results(password: String, month: String, reason: String) -> AppResult<bool>`（前端 `invoke('unlock_salary_results', { password, month, reason })`）；内部 `pub(crate) fn unlock_salary_results_impl(conn: &Connection, sec: &SecurityState, password: &str, month: &str, reason: &str) -> AppResult<bool>`

- [ ] **Step 1: 写失败测试**

在 `security_commands.rs` 测试模块追加。安全态构造复用 `security.rs` 测试模式（`security::setup(&conn, &state, "Abcd1234", "RC-AAAA", "Q", "A")` + `SecurityState::new()`，先看该文件是否已有测试模块与 `setup_db` 类 helper，有则复用，没有则新建并在注释说明）。工资/月结数据构造参考 `accounting.rs` 测试（INSERT salary_monthly_results 一行 2026-08 gross=10000；月结月测试另 INSERT `month_closes` 一行 status='closed'）：

```rust
#[test]
fn test_unlock_salary_results_impl() {
    // 1) 原因太短
    let (conn, sec) = sec_setup_with_salary(); // helper：建库+seed+security::setup+插入锁定工资行
    let err = unlock_salary_results_impl(&conn, &sec, "Abcd1234", "2026-08", "短").unwrap_err();
    assert!(err.to_string().contains("至少 5 个字"));
    // 2) 密码错误：仍锁定 + 日志
    let err = unlock_salary_results_impl(&conn, &sec, "Wrong123", "2026-08", "计算有误需要调整").unwrap_err();
    assert!(err.to_string().contains("密码错误"));
    let locked: i64 = conn.query_row(
        "SELECT locked FROM salary_monthly_results WHERE salary_month='2026-08'", [], |r| r.get(0)).unwrap();
    assert_eq!(locked, 1);
    let logs: i64 = conn.query_row(
        "SELECT COUNT(*) FROM operation_logs WHERE op_type='salary_unlock_failed'", [], |r| r.get(0)).unwrap();
    assert_eq!(logs, 1);
    // 3) 成功：解锁 + 凭证 void + 日志含原因
    //    （先 db::lock_salary_results 生成计提凭证，断言 voided_vouchers 进日志）
    db::lock_salary_results(&conn, "2026-08").unwrap();
    unlock_salary_results_impl(&conn, &sec, "Abcd1234", "2026-08", "社保基数算错需要调整").unwrap();
    let locked2: i64 = conn.query_row(
        "SELECT locked FROM salary_monthly_results WHERE salary_month='2026-08'", [], |r| r.get(0)).unwrap();
    assert_eq!(locked2, 0);
    let detail: String = conn.query_row(
        "SELECT COALESCE(detail,'') FROM operation_logs WHERE op_type='unlock_salary' ORDER BY id DESC LIMIT 1",
        [], |r| r.get(0)).unwrap();
    assert!(detail.contains("社保基数算错需要调整"));
    // 4) 月结月拒绝 + 5) 无锁定拒绝（构造 month_closes closed / 换月份，断言 Err 文案）
}
```

注意 `operation_logs` 列名以 `grep -n "CREATE TABLE IF NOT EXISTS operation_logs" -A 10 src-tauri/src/db.rs` 实测为准（detail 列可能叫 detail 或 detail_json，测试按实名调整；`op_type` 列同理，log_operation 的第 2 参列名以函数实现为准）。

运行确认编译失败（impl 不存在）：

```bash
cd src-tauri && cargo test --lib test_unlock_salary_results_impl
```

- [ ] **Step 2: 实现 impl 与命令**

`security_commands.rs` 追加（放在 `reveal_sensitive_data` 之后）：

```rust
/// 受控解锁已锁定工资：启动密码验证 + 必填原因 + 审计日志。
/// 只打开 locked 这条保护线；月结冻结与付款批次保护不变。
pub(crate) fn unlock_salary_results_impl(
    conn: &std::sync::MutexGuard<'_, rusqlite::Connection>,
    sec: &SecurityState,
    password: &str,
    month: &str,
    reason: &str,
) -> AppResult<bool> {
    if reason.trim().chars().count() < 5 {
        return Err(AppError::InvalidParam("请填写解锁原因（至少 5 个字）".into()));
    }
    let r = security::unlock(conn, sec, password)?;
    if !r.unlocked {
        let detail = format!("{{\"month\":\"{}\",\"failed_attempts\":{}}}", month, r.failed_attempts);
        let _ = db::log_operation(
            conn,
            "salary_unlock_failed",
            "受控解锁工资失败（密码错误）",
            SEC_OP_OPERATOR,
            Some(&detail),
        );
        return Err(AppError::InvalidParam("密码错误，无法解锁".into()));
    }
    let voided = db::unlock_salary_results(conn, month)?;
    let detail = format!(
        "{{\"month\":\"{}\",\"reason\":\"{}\",\"voided_vouchers\":{}}}",
        month,
        reason.trim().replace('\\', "\\\\").replace('"', "\\\""),
        voided
    );
    db::log_operation(
        conn,
        "unlock_salary",
        &format!("受控解锁{month}工资"),
        SEC_OP_OPERATOR,
        Some(&detail),
    )?;
    Ok(true)
}

#[tauri::command]
pub fn unlock_salary_results(
    password: String,
    month: String,
    reason: String,
    state: State<'_, Mutex<Connection>>,
    sec: State<'_, SecurityState>,
) -> AppResult<bool> {
    let conn = lock_conn(&state)?;
    unlock_salary_results_impl(&conn, &sec, &password, &month, &reason)
}
```

注意：`lock_conn` 返回的 MutexGuard 类型以该文件既有函数签名为准（看 `reveal_sensitive_data` 怎么写就怎么抄）；impl 参数直接用 `&Connection` 若 Guard 可以 deref 传递（`&conn` 强转），以能编译的最简写法为准，语义不变。`lib.rs` 的 `generate_handler![]` 追加 `security_commands::unlock_salary_results`（按该文件既有 security 命令注册分组位置）。

- [ ] **Step 3: 全量验证**

```bash
cd src-tauri && cargo test --lib && cargo fmt
# Expected: 全部通过（约 124+，含 5 个新断言场景）
```

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/security_commands.rs src-tauri/src/lib.rs
git commit -m "feat(unlock): 受控解锁工资命令（密码+原因+审计）"
```

### Task 3: 前端交互与收尾

**Files:**
- Modify: `src/api/index.ts`、`src/pages/SalaryCalculate.tsx`、`src/pages/OperationLogs.tsx`、`.claude/memory/stage5-accounting.md`

**Interfaces:**
- Consumes: Task 2 命令 `unlock_salary_results`（invoke 参数 `{ password, month, reason }`）；既有 `lockSalaryResults(month)`（重锁复用）

- [ ] **Step 1: api 接线**

`src/api/index.ts` 按现有风格追加（含浏览器 mock case）：

```typescript
export const unlockSalaryResults = (password: string, month: string, reason: string) =>
  invoke<boolean>('unlock_salary_results', { password, month, reason });
```

mock switch 补：`case 'unlock_salary_results': throw new Error('预览模式不支持该操作，请在桌面应用中操作');`

- [ ] **Step 2: SalaryCalculate.tsx 受控解锁交互**

先 `grep -n "lockSalaryResults\|锁定" src/pages/SalaryCalculate.tsx` 定位现有锁定按钮与月份锁定状态变量。改动：

1. 锁定状态下，锁定按钮旁新增红色按钮 **"受控解锁"**（`danger`）
2. 点击打开 Modal（新增局部 state `unlockModal`：`{ visible: boolean; password: string; reason: string; loading: boolean }`）：

```tsx
<Modal
  title="受控解锁工资"
  open={unlockModal.visible}
  confirmLoading={unlockModal.loading}
  okText="解锁"
  okButtonProps={{ danger: true }}
  cancelText="取消"
  onCancel={() => setUnlockModal({ visible: false, password: '', reason: '', loading: false })}
  onOk={handleUnlock}
>
  <Alert
    type="error"
    showIcon
    message="高风险操作"
    description="解锁后该月工资恢复可编辑，已有计提凭证将作废；需输入启动密码，操作将完整记录到操作日志。修改完成后请重新锁定。"
    style={{ marginBottom: 16 }}
  />
  <Form layout="vertical">
    <Form.Item label="启动密码" required>
      <Input.Password
        value={unlockModal.password}
        onChange={(e) => setUnlockModal((s) => ({ ...s, password: e.target.value }))}
      />
    </Form.Item>
    <Form.Item label="解锁原因" required extra="至少 5 个字，将记入操作日志">
      <Input.TextArea
        rows={3}
        value={unlockModal.reason}
        onChange={(e) => setUnlockModal((s) => ({ ...s, reason: e.target.value }))}
      />
    </Form.Item>
  </Form>
</Modal>
```

3. `handleUnlock`：前端先校验 `reason.trim().length >= 5`（不足 message.warning 后端同文案）→ `unlockSalaryResults(password, month, reason)` → 成功 `message.success('已受控解锁，修改完成后请重新锁定')` → 关 Modal → 刷新该月数据；失败 `message.error(String(err))` 展示后端中文
4. 解锁生效后页面顶部（锁定状态提示处）显示橙色 `<Tag color="orange">已受控解锁</Tag>`；原"锁定"按钮文案改为 **"重新锁定"**（仍调既有 `lockSalaryResults`）

- [ ] **Step 3: OperationLogs 映射**

`src/pages/OperationLogs.tsx` 中文映射追加：

```typescript
unlock_salary: '受控解锁工资',
salary_unlock_failed: '受控解锁工资失败',
```

- [ ] **Step 4: 文档更新**

`.claude/memory/stage5-accounting.md` 的"已知边界"第一条（`unlock_salary_results` 未暴露）改为：`unlock_salary_results 已通过 security_commands 以受控方式暴露（密码+原因+审计），仅打开 locked 线，月结/付款批次保护不变`。

- [ ] **Step 5: 回归**

```bash
npx tsc --noEmit
npm run lint
npm run build
cd src-tauri && cargo test --lib
```

Expected: 全部通过（既有 chunk 体积提示忽略）。

- [ ] **Step 6: Commit**

```bash
git add src/api/index.ts src/pages/SalaryCalculate.tsx src/pages/OperationLogs.tsx .claude/memory/stage5-accounting.md
git commit -m "feat(unlock): 工资页受控解锁交互与审计映射"
```

---

## Self-Review 记录

- **Spec 覆盖**：spec §2.1/2.2→Task 1+2；§2.3 重锁复用→Task 3 Step 2.4；§2.4 保护线不动→Task 1（ensure_month_open 保留）+ 付款 guard 未触碰；§3 前端→Task 3；§4 审计→Task 2/3；§5 错误处理→Task 2 impl；§6 测试→Task 1/2/3（GUI 验收 Windows）；§7 范围外未做。
- **占位符**：测试 helper `sec_setup_with_salary` 给出构成说明（建库+seed+security::setup+锁定工资行），数据构造给出参考来源；operation_logs 列名给出 grep 实测指令（既有表，不属于本计划新建范围）。
- **类型一致性**：`db::unlock_salary_results -> AppResult<usize>`（Task 1 产出 = Task 2 消费）；命令名 `unlock_salary_results` 前后端一致（invoke 参数 camelCase 顶层 password/month/reason 本身无大小写差异）。
