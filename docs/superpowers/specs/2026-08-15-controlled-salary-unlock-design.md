# 受控解锁已锁定工资设计

- 日期：2026-08-15
- 状态：已与用户确认
- 前置：第五阶段凭证引擎（锁定 guard / `db::unlock_salary_results` / 凭证作废联动）、第四阶段安全体系（启动密码 / `security::unlock` / 操作日志）

## 1. 背景与目标

工资锁定后目前完全不可调整/重算（第五阶段加的 locked guard）。真实场景中确实存在"算错需要改"的情况，完全堵死会逼用户走反月结等更重的路径。

目标：给工资锁定线开一个**受控口子**——密码验证 + 强警告 + 必填原因 + 完整审计日志，与"敏感数据解锁"同一安全模型。

### 已确认的关键决策

| 决策点 | 结论 |
|---|---|
| 口子边界 | **只开工资锁定线**：正式月结月份仍绝对冻结（ensure_month_open）；付款批次保护保留（已入有效批次的仍需先作废批次） |
| 交互形态 | 显式"受控解锁 → 修改 → 重新锁定"，与反月结交互模式一致 |
| 密码门槛 | 复用启动密码（`security::unlock`），失败计数与锁屏共享（5 次锁屏） |
| 审计 | 解锁必填原因（≥5 字）；密码错误/解锁成功/重新锁定全部记操作日志 |

## 2. 后端设计

### 2.1 新命令 `unlock_salary_results`（commands.rs，security 分区）

```
签名：fn unlock_salary_results(
    password: String, month: String, reason: String,
    state: State<Mutex<Connection>>, sec: State<SecurityState>,
) -> AppResult<bool>

流程：
1. reason.trim() 长度 < 5 → Err("请填写解锁原因（至少 5 个字）")
2. security::unlock(&conn, &sec, &password)
   失败 → log_operation("salary_unlock_failed",
             "受控解锁工资失败（密码错误）", JSON{failed_attempts})
          → Err("密码错误，无法解锁")（共享既有失败计数，满 5 次自动锁屏）
   成功 → 继续（顺带加载 DEK，与 reveal_sensitive_data 一致，无副作用）
3. db::unlock_salary_results(&conn, &month)  // 返回值改造：bool → usize（作废凭证数）
   内部已有：ensure_month_open（月结月拒绝）+ UPDATE locked=0/status='reviewed'
            + void 该月 salary_accrual 凭证（同一事务）
   UPDATE 影响行数为 0 时由 db 函数返回 Err("该月没有已锁定的工资结果")
4. （步骤 3 成功即继续，作废数可能为 0——例如计提金额为零的结果本就无凭证）
5. log_operation("unlock_salary", "受控解锁{month}工资",
     JSON{month, reason, voided_vouchers})
6. Ok(true)
```

### 2.2 `db::unlock_salary_results` 签名调整

`AppResult<bool>` → `AppResult<usize>`（返回作废凭证数；UPDATE 影响 0 行时返回 Err"该月没有已锁定的工资结果"），调用点（仅测试）同步更新。其余逻辑不变。

### 2.3 重新锁定

复用现有 `lock_salary_results` 命令：幂等重新生成计提凭证 + 既有日志（"锁定{month}工资"）。不新增代码。

### 2.4 不动的保护线

- 正式月结月份：`ensure_month_open` 继续拒绝（月结即关账不破）
- 付款批次保护：`save_salary_result` / `update_salary_result` 中的 `active_payment_item_exists` guard 原样保留——解锁只解决 locked，不解决批次

## 3. 前端设计

**页面**：`src/pages/SalaryCalculate.tsx`（工资计算页）

| 状态 | UI |
|---|---|
| 已锁定 | 原"锁定"按钮位置显示红色 **"受控解锁"** 按钮 |
| 受控解锁 Modal | 红色 Alert（"解锁后该月工资恢复可编辑，已有计提凭证将作废；操作需输入启动密码并将完整记录到操作日志，请谨慎操作"）+ 启动密码 `Input.Password` + 解锁原因 `TextArea`（必填，前端先校验 ≥5 字） |
| 解锁后 | message 成功提示 + 顶部橙色 Tag "已受控解锁" + 按钮变 **"重新锁定"**（调既有 `lock_salary_results`，恢复保护并重算凭证） |

**接线**：`src/api/index.ts` 加 `unlockSalaryResults(password, month, reason)`（含浏览器 mock case：抛"预览模式不支持"）；`src/types/index.ts` 无需新类型；`OperationLogs.tsx` 补中文映射 `unlock_salary` / `salary_unlock_failed`。

## 4. 审计事件

| 事件 | op_type | 日志内容 |
|---|---|---|
| 密码错误 | `salary_unlock_failed` | 受控解锁工资失败（密码错误）+ failed_attempts |
| 解锁成功 | `unlock_salary` | 受控解锁{month}工资 + JSON{month, reason, voided_vouchers} |
| 重新锁定 | `lock_salary`（复用现有） | 锁定{month}工资 |

## 5. 错误处理

全部中文提示：原因太短 / 密码错误 / 该月没有已锁定的工资结果 / 该月已正式月结（复用 ensure_month_open 既有文案）。

## 6. 测试计划

Rust（commands 层单测，参考既有 security 命令测试构造 SecurityState）：

1. 原因 < 5 字 → Err，不触密码验证
2. 密码错误 → Err + 日志含"密码错误" + 工资仍锁定
3. 已正式月结月份 → Err（月结锁优先于解锁）
4. 无锁定结果月份 → Err
5. 成功：locked=0、计提凭证全部 void、日志含 reason 与 voided_vouchers
6. 付款批次保护仍生效：解锁后修改已入有效批次的结果仍被拒

前端回归：`npx tsc --noEmit`、`npm run lint`、`npm run build`；`tauri dev` 手工验收（Windows）。

## 7. 范围外（明确不做）

- 付款批次保护的密码绕过
- 正式月结月份的密码绕过
- 独立第二密码
- 解锁有效期/自动重锁（保持显式重新锁定，简单可控）
