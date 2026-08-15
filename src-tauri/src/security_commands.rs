//! 安全模块的 Tauri 命令薄封装。所有底层逻辑在 `security.rs`，本文件只负责：
//! 1. 从 `State<Mutex<Connection>>` 取锁拿到 `&Connection`
//! 2. 透传给 `security::*` 同步函数
//! 3. 把 `AppResult` 暴露给前端（Tauri 自动序列化）
//!
//! 命令命名遵循 snake_case；命令参数顺序约定：业务参数在前，`state`、`sec` 在后。

use crate::db;
use crate::errors::{AppError, AppResult};
use crate::models::{LegacyMigrationStatus, RevealResult, SecurityStatus, UnlockResult};
use crate::security::{self, SecurityState};
use chrono::Utc;
use rusqlite::Connection;
use std::sync::Mutex;
use tauri::State;

/// 安全事件的 operator 固定标识。出纳单人场景，没有账号体系。
const SEC_OP_OPERATOR: &str = "security";

/// 取连接锁的统一封装。两行写法比 macro 更直观，调用方拿到的就是 `MutexGuard`。
fn lock_conn<'a>(
    state: &'a State<'_, Mutex<Connection>>,
) -> AppResult<std::sync::MutexGuard<'a, Connection>> {
    state.lock().map_err(|e| AppError::General(e.to_string()))
}

/// 判断是否已执行过 setup。前端启动时据此决定展示 Setup 还是 Unlock。
#[tauri::command]
pub fn is_security_initialized(state: State<'_, Mutex<Connection>>) -> AppResult<bool> {
    let conn = lock_conn(&state)?;
    Ok(security::is_initialized(&conn))
}

/// 首次启用安全模块。密码 / 找回码 / 安全问题答案三者都会 wrap 同一份 DEK。
#[tauri::command]
pub fn setup_security(
    password: String,
    recovery_code: String,
    security_question: String,
    answer: String,
    state: State<'_, Mutex<Connection>>,
    sec: State<'_, SecurityState>,
) -> AppResult<()> {
    let conn = lock_conn(&state)?;
    security::setup(
        &conn,
        &sec,
        &password,
        &recovery_code,
        &security_question,
        &answer,
    )
}

/// 用密码解锁。成功 → DEK 载入内存；失败 → 失败计数 +1，达到上限写 lock_until。
#[tauri::command]
pub fn unlock(
    password: String,
    state: State<'_, Mutex<Connection>>,
    sec: State<'_, SecurityState>,
) -> AppResult<UnlockResult> {
    let conn = lock_conn(&state)?;
    let r = security::unlock(&conn, &sec, &password);
    match &r {
        Ok(out) => {
            let action = if out.unlocked {
                "unlock_success"
            } else {
                "unlock_failed"
            };
            let detail = format!(
                "{{\"unlocked\":{},\"failed_attempts\":{}}}",
                out.unlocked, out.failed_attempts
            );
            let desc = if out.unlocked {
                "密码解锁成功".to_string()
            } else {
                format!("密码解锁失败，累计失败 {} 次", out.failed_attempts)
            };
            let _ = db::log_operation(&conn, action, &desc, SEC_OP_OPERATOR, Some(&detail));
        }
        Err(e) => {
            let detail = format!("{{\"error\":\"{}\"}}", json_escape(&e.to_string()));
            let _ = db::log_operation(
                &conn,
                "unlock_failed",
                "解锁过程异常",
                SEC_OP_OPERATOR,
                Some(&detail),
            );
        }
    }
    r
}

/// 锁屏：清空内存中的 DEK。DB 中密文不动。
#[tauri::command]
pub fn lock(state: State<'_, Mutex<Connection>>, sec: State<'_, SecurityState>) -> AppResult<()> {
    let conn = lock_conn(&state)?;
    security::lock(&sec);
    let now = Utc::now().to_rfc3339();
    let detail = format!("{{\"at\":\"{}\"}}", now);
    let _ = db::log_operation(&conn, "lock", "手动锁屏", SEC_OP_OPERATOR, Some(&detail));
    Ok(())
}

/// 安全中心状态概览。
/// 未初始化 → initialized:false, locked:true（其他字段默认值，不报错）。
/// 已初始化但 DEK 未加载 → locked:true。
#[tauri::command]
pub fn get_security_status(
    state: State<'_, Mutex<Connection>>,
    sec: State<'_, SecurityState>,
) -> AppResult<SecurityStatus> {
    let conn = lock_conn(&state)?;
    let initialized = security::is_initialized(&conn);
    if !initialized {
        return Ok(SecurityStatus {
            initialized: false,
            locked: true,
            failed_attempts: 0,
            lock_until: None,
            idle_lock_enabled: true,
            idle_timeout_seconds: 300,
            sensitive_reveal_seconds: 300,
            migration_status: None,
        });
    }
    let (idle_lock_enabled, idle_timeout_seconds, sensitive_reveal_seconds) =
        security::get_idle_settings(&conn)?;
    let (failed_attempts, lock_until) = conn.query_row(
        "SELECT failed_attempts, lock_until FROM security_state WHERE id = 1",
        [],
        |r| Ok((r.get::<_, u32>(0)?, r.get::<_, Option<String>>(1)?)),
    )?;
    let migration_status = conn
        .query_row(
            "SELECT status FROM legacy_migration_state WHERE id = 1",
            [],
            |r| r.get::<_, String>(0),
        )
        .ok();
    Ok(SecurityStatus {
        initialized,
        locked: !sec.is_dek_loaded(),
        failed_attempts,
        lock_until,
        idle_lock_enabled,
        idle_timeout_seconds,
        sensitive_reveal_seconds,
        migration_status,
    })
}

/// 已登录用户改密。用旧密码 unwrap 验证后 rewrap，失败不影响旧密文。
#[tauri::command]
pub fn change_password(
    old: String,
    new: String,
    state: State<'_, Mutex<Connection>>,
    sec: State<'_, SecurityState>,
) -> AppResult<()> {
    let conn = lock_conn(&state)?;
    let r = security::change_password(&conn, &sec, &old, &new);
    match &r {
        Ok(()) => {
            let now = Utc::now().to_rfc3339();
            let detail = format!("{{\"at\":\"{}\"}}", now);
            let _ = db::log_operation(
                &conn,
                "change_password",
                "修改密码成功",
                SEC_OP_OPERATOR,
                Some(&detail),
            );
        }
        Err(e) => {
            // 旧密码错误等失败也记录，便于审计暴力猜测
            let detail = format!("{{\"error\":\"{}\"}}", json_escape(&e.to_string()));
            let _ = db::log_operation(
                &conn,
                "change_password_failed",
                "修改密码失败",
                SEC_OP_OPERATOR,
                Some(&detail),
            );
        }
    }
    r
}

/// 用恢复码重置密码。失败累计 3 次锁定 15 分钟。
#[tauri::command]
pub fn reset_password_by_recovery(
    code: String,
    new_password: String,
    state: State<'_, Mutex<Connection>>,
    sec: State<'_, SecurityState>,
) -> AppResult<()> {
    let conn = lock_conn(&state)?;
    let r = security::reset_password_by_recovery(&conn, &sec, &code, &new_password);
    match &r {
        Ok(()) => {
            let now = Utc::now().to_rfc3339();
            let detail = format!("{{\"at\":\"{}\"}}", now);
            let _ = db::log_operation(
                &conn,
                "reset_by_recovery",
                "通过恢复码重置密码成功",
                SEC_OP_OPERATOR,
                Some(&detail),
            );
        }
        Err(e) => {
            let detail = format!("{{\"error\":\"{}\"}}", json_escape(&e.to_string()));
            let _ = db::log_operation(
                &conn,
                "reset_by_recovery_failed",
                "通过恢复码重置密码失败",
                SEC_OP_OPERATOR,
                Some(&detail),
            );
        }
    }
    r
}

/// 用安全问题答案重置密码。失败累计 3 次锁定 15 分钟。
#[tauri::command]
pub fn reset_password_by_question(
    answer: String,
    new_password: String,
    state: State<'_, Mutex<Connection>>,
    sec: State<'_, SecurityState>,
) -> AppResult<()> {
    let conn = lock_conn(&state)?;
    let r = security::reset_password_by_question(&conn, &sec, &answer, &new_password);
    match &r {
        Ok(()) => {
            let now = Utc::now().to_rfc3339();
            let detail = format!("{{\"at\":\"{}\"}}", now);
            let _ = db::log_operation(
                &conn,
                "reset_by_question",
                "通过安全问题重置密码成功",
                SEC_OP_OPERATOR,
                Some(&detail),
            );
        }
        Err(e) => {
            let detail = format!("{{\"error\":\"{}\"}}", json_escape(&e.to_string()));
            let _ = db::log_operation(
                &conn,
                "reset_by_question_failed",
                "通过安全问题重置密码失败",
                SEC_OP_OPERATOR,
                Some(&detail),
            );
        }
    }
    r
}

/// 更新闲置锁定设置。
#[tauri::command]
pub fn update_idle_settings(
    enabled: bool,
    seconds: u32,
    state: State<'_, Mutex<Connection>>,
) -> AppResult<()> {
    let conn = lock_conn(&state)?;
    security::update_idle_settings(&conn, enabled, seconds)
}

/// 更新敏感字段回显时长（秒）。
#[tauri::command]
pub fn update_sensitive_reveal_settings(
    seconds: u32,
    state: State<'_, Mutex<Connection>>,
) -> AppResult<()> {
    let conn = lock_conn(&state)?;
    security::update_sensitive_reveal_settings(&conn, seconds)
}

/// 临时揭示敏感字段：用密码走 unlock 验证，成功则按 sensitive_reveal_seconds 计算到期时间。
/// 不改变全局 lock 状态（unlock 成功会顺带加载 DEK，符合"揭示期间可访问敏感字段"语义）。
#[tauri::command]
pub fn reveal_sensitive_data(
    password: String,
    state: State<'_, Mutex<Connection>>,
    sec: State<'_, SecurityState>,
) -> AppResult<RevealResult> {
    let conn = lock_conn(&state)?;
    let r = security::unlock(&conn, &sec, &password)?;
    if !r.unlocked {
        let detail = format!(
            "{{\"unlocked\":false,\"failed_attempts\":{}}}",
            r.failed_attempts
        );
        let _ = db::log_operation(
            &conn,
            "reveal_sensitive_failed",
            "敏感数据解锁失败（密码错误）",
            SEC_OP_OPERATOR,
            Some(&detail),
        );
        return Err(AppError::InvalidParam("密码错误，无法查看敏感数据".into()));
    }
    let seconds = security::get_idle_settings(&conn)?.2;
    let expires_at = (Utc::now() + chrono::Duration::seconds(seconds as i64)).to_rfc3339();
    let detail = format!(
        "{{\"expires_at\":\"{}\",\"seconds\":{}}}",
        expires_at, seconds
    );
    let _ = db::log_operation(
        &conn,
        "reveal_sensitive",
        "敏感数据解锁成功",
        SEC_OP_OPERATOR,
        Some(&detail),
    );
    Ok(RevealResult { expires_at })
}

/// 受控解锁已锁定工资：启动密码验证 + 必填原因 + 审计日志。
/// 只打开 locked 这条保护线；月结冻结与付款批次保护不变。
pub(crate) fn unlock_salary_results_impl(
    conn: &Connection,
    sec: &SecurityState,
    password: &str,
    month: &str,
    reason: &str,
) -> AppResult<bool> {
    if reason.trim().chars().count() < 5 {
        return Err(AppError::InvalidParam(
            "请填写解锁原因（至少 5 个字）".into(),
        ));
    }
    let r = security::unlock(conn, sec, password)?;
    if !r.unlocked {
        let detail = format!(
            "{{\"month\":\"{}\",\"failed_attempts\":{}}}",
            json_escape(month),
            r.failed_attempts
        );
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
        json_escape(month),
        json_escape(reason.trim()),
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

/// 受控解锁已锁定工资的 Tauri 命令入口。
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

/// 读取旧版迁移进度。表为空（首次启动）→ 视为已完成，避免老用户被强制迁移引导打断。
#[tauri::command]
pub fn get_legacy_migration_status(
    state: State<'_, Mutex<Connection>>,
) -> AppResult<LegacyMigrationStatus> {
    let conn = lock_conn(&state)?;
    let row = conn.query_row(
        "SELECT status, total_invoices, processed_invoices, token_migrated
         FROM legacy_migration_state WHERE id = 1",
        [],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)? != 0,
            ))
        },
    );
    match row {
        Ok((status, total, processed, token_migrated)) => Ok(LegacyMigrationStatus {
            status,
            total_invoices: total,
            processed_invoices: processed,
            token_migrated,
        }),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(LegacyMigrationStatus {
            status: "completed".into(),
            total_invoices: 0,
            processed_invoices: 0,
            token_migrated: true,
        }),
        Err(e) => Err(e.into()),
    }
}

/// 触发旧版迁移。Task 9 实现具体逻辑；当前 stub 直接返回 Ok。
#[tauri::command]
pub fn migrate_legacy_resources(
    state: State<'_, Mutex<Connection>>,
    sec: State<'_, SecurityState>,
    app: tauri::AppHandle,
) -> AppResult<()> {
    let conn = lock_conn(&state)?;
    crate::legacy_migration::run(&conn, &sec, &app)
}

/// 取发票原图的可预览 URL。
/// - `image_encrypted = 0`：直接复制原图到 preview 目录
/// - `image_encrypted = 1`：用 DEK 解密到 preview 目录；DEK 未加载返回 InvalidParam("请先解锁应用")
///
/// 返回 preview 目录中的绝对路径字符串（前端用 `convertFileSrc()` 包一层即可显示）。
/// 文件名格式 `{invoice_id}_{timestamp_millis}.{ext}`，每次调用都新建一份，
/// 让前端缓存策略不会拿到旧的解密副本。
#[tauri::command]
pub fn get_decrypted_invoice_url(
    invoice_id: i64,
    state: State<'_, Mutex<Connection>>,
    sec: State<'_, SecurityState>,
) -> AppResult<String> {
    use rusqlite::params;

    let (image_path, encrypted): (String, i64) = {
        let conn = lock_conn(&state)?;
        conn.query_row(
            "SELECT image_path, image_encrypted FROM invoices WHERE id = ?1",
            params![invoice_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
        )
        .map_err(|_| AppError::NotFound(format!("发票ID={invoice_id}未找到")))?
    };

    let preview_dir = std::env::temp_dir().join("salary-desktop-preview");
    std::fs::create_dir_all(&preview_dir)?;

    let src_path = std::path::Path::new(&image_path);
    let ext = src_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin");
    let dst = preview_dir.join(format!(
        "{}_{}.{}",
        invoice_id,
        Utc::now().timestamp_millis(),
        ext
    ));

    if encrypted == 1 {
        let dek = sec
            .dek()
            .ok_or_else(|| AppError::InvalidParam("请先解锁应用".into()))?;
        security::decrypt_file(src_path, &dst, &dek)
            .map_err(|_| AppError::InvalidParam("解密失败".into()))?;
    } else {
        std::fs::copy(src_path, &dst)?;
    }

    Ok(dst.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试 helper：建库（db::create_tables + seed_gl_accounts，含安全态/工资/凭证/月结/审计全部表）
    /// + security::setup 初始化安全配置 + 插入一行 2026-08 已锁定工资结果。
    /// 本文件此前没有测试模块，故新建；建库方式参考 accounting.rs 的 setup（需要完整财务表，
    /// 不能复用 security.rs 的 setup_db——那张表集只有发票/安全态，缺工资与凭证表）。
    fn sec_setup_with_salary() -> (Connection, SecurityState) {
        let conn = Connection::open_in_memory().unwrap();
        db::create_tables(&conn).unwrap();
        db::seed_gl_accounts(&conn).unwrap();
        let sec = SecurityState::new();
        security::setup(&conn, &sec, "Abcd1234", "RC-AAAA", "Q", "A").unwrap();
        // 工资行 INSERT 参考 accounting.rs 测试 test_salary_accrual_voucher
        conn.execute(
            "INSERT INTO salary_monthly_results
                (salary_month, employee_no, name, department, gross_salary, net_salary,
                 social_security_personal, housing_fund_personal, attendance_deduction,
                 tax_amount, other_deduction, status, locked, created_at, updated_at)
             VALUES ('2026-08', 'E001', '张三', '销售部', 10000, 7400, 1000, 800, 500, 200, 100,
                     'locked', 1, '2026-08-31', '2026-08-31')",
            [],
        )
        .unwrap();
        (conn, sec)
    }

    #[test]
    fn test_unlock_salary_results_impl() {
        // 1) 原因太短
        let (conn, sec) = sec_setup_with_salary();
        let err = unlock_salary_results_impl(&conn, &sec, "Abcd1234", "2026-08", "短").unwrap_err();
        assert!(err.to_string().contains("至少 5 个字"));
        // 2) 密码错误：仍锁定 + 日志
        let err =
            unlock_salary_results_impl(&conn, &sec, "Wrong123", "2026-08", "计算有误需要调整")
                .unwrap_err();
        assert!(err.to_string().contains("密码错误"));
        // 密码错误计入 security_state 共享失败计数（与启动解锁同一计数器）
        let failed: i64 = conn
            .query_row(
                "SELECT failed_attempts FROM security_state WHERE id = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(failed, 1);
        let locked: i64 = conn
            .query_row(
                "SELECT locked FROM salary_monthly_results WHERE salary_month='2026-08'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(locked, 1);
        // operation_logs 的列名以 db.rs 实测为准（operation_type，非 op_type）
        let logs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM operation_logs WHERE operation_type='salary_unlock_failed'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(logs, 1);
        // 3) 成功：解锁 + 凭证 void + 日志含原因
        //    （先回退到未锁定，再 db::lock_salary_results 生成计提凭证，断言 voided_vouchers 进日志）
        conn.execute(
            "UPDATE salary_monthly_results SET locked = 0, status = 'reviewed'
             WHERE salary_month = '2026-08'",
            [],
        )
        .unwrap();
        db::lock_salary_results(&conn, "2026-08").unwrap();
        unlock_salary_results_impl(&conn, &sec, "Abcd1234", "2026-08", "社保基数算错需要调整")
            .unwrap();
        let locked2: i64 = conn
            .query_row(
                "SELECT locked FROM salary_monthly_results WHERE salary_month='2026-08'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(locked2, 0);
        let detail: String = conn
            .query_row(
                "SELECT COALESCE(detail,'') FROM operation_logs
                 WHERE operation_type='unlock_salary' ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(detail.contains("社保基数算错需要调整"));
        assert!(detail.contains("\"voided_vouchers\":1"));
        // 4) 月结月拒绝
        conn.execute(
            "INSERT INTO month_closes (month, status, created_at, updated_at)
             VALUES ('2026-08', 'closed', '2026-08-31', '2026-08-31')",
            [],
        )
        .unwrap();
        let err =
            unlock_salary_results_impl(&conn, &sec, "Abcd1234", "2026-08", "重新核算需要调整")
                .unwrap_err();
        assert!(err.to_string().contains("已正式月结"));
        // 5) 无锁定拒绝（换月份）
        let err =
            unlock_salary_results_impl(&conn, &sec, "Abcd1234", "2026-09", "重新核算需要调整")
                .unwrap_err();
        assert!(err.to_string().contains("没有已锁定"));
    }
}

/// 把任意字符串转义成安全的 JSON 字符串内容（不含两侧引号）。
/// 用于把 AppError 文本塞进 detail JSON 的 `"error"` 字段，避免引号/反斜杠破坏 JSON 结构。
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}
