//! 安全模块的 Tauri 命令薄封装。所有底层逻辑在 `security.rs`，本文件只负责：
//! 1. 从 `State<Mutex<Connection>>` 取锁拿到 `&Connection`
//! 2. 透传给 `security::*` 同步函数
//! 3. 把 `AppResult` 暴露给前端（Tauri 自动序列化）
//!
//! 命令命名遵循 snake_case；命令参数顺序约定：业务参数在前，`state`、`sec` 在后。

use crate::errors::{AppError, AppResult};
use crate::models::{LegacyMigrationStatus, RevealResult, SecurityStatus, UnlockResult};
use crate::security::{self, SecurityState};
use chrono::Utc;
use rusqlite::Connection;
use std::sync::Mutex;
use tauri::State;

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
    security::unlock(&conn, &sec, &password)
}

/// 锁屏：清空内存中的 DEK。DB 中密文不动。
#[tauri::command]
pub fn lock(sec: State<'_, SecurityState>) -> AppResult<()> {
    security::lock(&sec);
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
    security::change_password(&conn, &sec, &old, &new)
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
    security::reset_password_by_recovery(&conn, &sec, &code, &new_password)
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
    security::reset_password_by_question(&conn, &sec, &answer, &new_password)
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
        return Err(AppError::InvalidParam("密码错误，无法查看敏感数据".into()));
    }
    let seconds = security::get_idle_settings(&conn)?.2;
    let expires_at = (Utc::now() + chrono::Duration::seconds(seconds as i64)).to_rfc3339();
    Ok(RevealResult { expires_at })
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
