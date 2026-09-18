//! 第九阶段通知底座（spec 4）：媒介抽象 + SMTP 配置加密 + 批量发送骨架。
//!
//! - 媒介抽象 [`NotifyChannel`]：领域函数统一收 `&dyn NotifyChannel`，单测注入
//!   FakeChannel 不真发；EmailChannel（lettre）由 Task 3 接入，本模块零新依赖。
//! - SMTP 配置：整键 JSON 序列化后 AES-GCM 加密写 `app_settings.smtp_config`
//!   （复用 OCR token 的 `security::encrypt_bytes/decrypt_bytes`，密文格式
//!   `base64(nonce):base64(cipher)`）；授权码永不明文落库、界面脱敏回显。
//! - 发送留痕：逐封 send → 写 notification_logs（sent/failed/skipped），
//!   单封失败不中断批量；resend_failed 仅重发 status='failed' 记录并写新记录。
//!
//! 注：本任务为底座，命令层（Task 3/5）接入后以下 pub 项才有调用方，
//! 故整模块暂时 allow(dead_code)。

#![allow(dead_code)]

use base64::Engine;
use chrono::Utc;
use rusqlite::{params, params_from_iter, Connection};
use serde::{Deserialize, Serialize};

use crate::db::{get_setting, log_operation, set_setting};
use crate::errors::{AppError, AppResult};
use crate::models::{
    NotificationLog, NotificationLogQuery, SmtpConfig, SmtpConfigMasked, NOTIFICATION_CHANNELS,
    NOTIFICATION_STATUSES, NOTIFICATION_STATUS_FAILED, NOTIFICATION_STATUS_SENT,
    NOTIFICATION_STATUS_SKIPPED,
};
use crate::security::SecurityState;

// ==================== 媒介抽象 ====================

/// 通知消息（spec 4.1）：recipient 为邮箱地址 / 手机号。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NotifyMessage {
    pub recipient: String,
    pub subject: String,
    pub html_body: Option<String>,
    pub text_body: Option<String>,
}

/// 通知媒介抽象（spec 4.1）：`name()` 返回 'email' | 'sms'（与
/// notification_logs.channel CHECK 一致）；send 为纯发送动作（网络 IO），
/// 由调用方负责发送结果落库。
pub trait NotifyChannel {
    fn name(&self) -> &'static str;
    fn send(&self, message: &NotifyMessage) -> AppResult<()>;
}

/// 短信通道占位（spec 2：接口预留，不采集密钥）：任何发送请求都显式报错，
/// 提示用户接入服务商，避免误以为已发出。
pub struct SmsChannel;

impl NotifyChannel for SmsChannel {
    fn name(&self) -> &'static str {
        "sms"
    }

    fn send(&self, _message: &NotifyMessage) -> AppResult<()> {
        Err(AppError::General(
            "短信通道未配置，请在设置中接入服务商".into(),
        ))
    }
}

/// 批量发送的单封消息项：留痕字段（employee_id/recipient/belong_month）+
/// 实际发送的消息体。组装方须保证 `recipient` 与 `message.recipient` 一致
/// （前者写留痕，后者供通道发送）。
#[derive(Debug, Clone, Default)]
pub struct MessageItem {
    pub employee_id: Option<i64>,
    pub recipient: String,
    pub belong_month: Option<String>,
    pub message: NotifyMessage,
}

/// 批量发送汇总（spec 4.2）：sent/failed 为发送结果计数，skipped 为空收件地址、
/// 重发请求中非 failed 记录等未发起发送的条数；failed_log_ids 为失败留痕 id
/// （供前端勾选重发）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BatchSummary {
    pub sent: usize,
    pub failed: usize,
    pub skipped: usize,
    pub failed_log_ids: Vec<i64>,
}

// ==================== SMTP 配置（app_settings 加密键 smtp_config） ====================

const SMTP_CONFIG_KEY: &str = "smtp_config";
/// 加密方式白名单（spec 3.3）
const SMTP_ENCRYPTION_OPTIONS: &[&str] = &["starttls", "ssl", "none"];

/// 密文打包格式：`base64(nonce):base64(cipher)`（base64 标准字母表不含 ':'，
/// split_once 安全）。
fn seal(plain: &str, dek: &[u8; 32]) -> AppResult<String> {
    let (cipher, nonce) = crate::security::encrypt_bytes(plain.as_bytes(), dek)?;
    Ok(format!(
        "{}:{}",
        base64::engine::general_purpose::STANDARD.encode(nonce),
        base64::engine::general_purpose::STANDARD.encode(cipher)
    ))
}

fn unseal(value: &str, dek: &[u8; 32]) -> AppResult<String> {
    let (nonce_part, cipher_part) = value
        .split_once(':')
        .ok_or_else(|| AppError::General("SMTP 配置密文格式异常，请重新保存通知设置".into()))?;
    let nonce_bytes = base64::engine::general_purpose::STANDARD
        .decode(nonce_part.as_bytes())
        .map_err(|e| AppError::General(format!("SMTP 配置 nonce base64 解码失败: {e}")))?;
    if nonce_bytes.len() != 12 {
        return Err(AppError::General("SMTP 配置 nonce 长度异常".into()));
    }
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&nonce_bytes);
    let cipher = base64::engine::general_purpose::STANDARD
        .decode(cipher_part.as_bytes())
        .map_err(|e| AppError::General(format!("SMTP 配置密文 base64 解码失败: {e}")))?;
    let plain = crate::security::decrypt_bytes(&cipher, &nonce, dek)?;
    String::from_utf8(plain).map_err(|e| AppError::General(e.to_string()))
}

/// 取内存中的 DEK；未初始化/未解锁时显式报错（SMTP 授权码属外联凭证，
/// spec 3.3 要求 AES-GCM 加密入库，故不提供明文兜底路径）。
fn require_dek(sec: &SecurityState) -> AppResult<[u8; 32]> {
    sec.dek().map(|zk| *zk).ok_or_else(|| {
        AppError::General("安全模块未解锁，请先完成启动密码验证再使用通知功能".into())
    })
}

/// 保存前校验（spec 3.3 + Task 1 挂账）：host/username 非空、port 1-65535、
/// encryption 白名单。port 为 u16，仅 0 非法，1-65535 天然满足。
fn validate_smtp_config(config: &SmtpConfig) -> AppResult<()> {
    if config.host.trim().is_empty() {
        return Err(AppError::InvalidParam("SMTP 服务器地址不能为空".into()));
    }
    if config.username.trim().is_empty() {
        return Err(AppError::InvalidParam("发件账号不能为空".into()));
    }
    if config.port == 0 {
        return Err(AppError::InvalidParam("SMTP 端口须在 1-65535 之间".into()));
    }
    if !SMTP_ENCRYPTION_OPTIONS.contains(&config.encryption.as_str()) {
        return Err(AppError::InvalidParam(
            "加密方式仅支持 starttls / ssl / none".into(),
        ));
    }
    Ok(())
}

/// 保存 SMTP 配置：整键 JSON 序列化后 AES-GCM 加密写 `app_settings.smtp_config`
/// （INSERT OR REPLACE 覆盖旧值），并写操作留痕（detail 仅含 host/port/encryption，
/// 不含授权码）。
pub fn set_smtp_config(
    conn: &Connection,
    config: &SmtpConfig,
    operator: &str,
    sec: &SecurityState,
) -> AppResult<()> {
    validate_smtp_config(config)?;
    let dek = require_dek(sec)?;
    let json = serde_json::to_string(config)?;
    let sealed = seal(&json, &dek)?;
    set_setting(conn, SMTP_CONFIG_KEY, &sealed)?;
    log_operation(
        conn,
        "smtp_config_update",
        "更新 SMTP 通知配置",
        operator,
        Some(&format!(
            "{}:{}:{}",
            config.host, config.port, config.encryption
        )),
    )?;
    Ok(())
}

/// 读取 SMTP 配置（含真实授权码，仅供发送通道使用，不回传前端）。
/// 未配置返回 None（无需解锁）；已配置但 DEK 未加载时报错（无法解密）。
pub fn get_smtp_config(conn: &Connection, sec: &SecurityState) -> AppResult<Option<SmtpConfig>> {
    let Some(value) = get_setting(conn, SMTP_CONFIG_KEY)? else {
        return Ok(None);
    };
    let dek = require_dek(sec)?;
    let json = unseal(&value, &dek)?;
    Ok(Some(serde_json::from_str(&json)?))
}

/// 读取 SMTP 配置（脱敏回显，供命令层/设置页）：授权码仅返回 `****`+末 2 位
/// （未设置时不带尾缀）。
pub fn get_smtp_config_masked(
    conn: &Connection,
    sec: &SecurityState,
) -> AppResult<Option<SmtpConfigMasked>> {
    match get_smtp_config(conn, sec)? {
        None => Ok(None),
        Some(config) => Ok(Some(SmtpConfigMasked {
            host: config.host,
            port: config.port,
            encryption: config.encryption,
            username: config.username,
            password_masked: mask_password(&config.password),
            from_name: config.from_name,
        })),
    }
}

/// 授权码脱敏：`****` + 末 2 位（不足 2 位取实际位数）；未设置（空串）时
/// 返回 `****` 不带尾缀（spec 3.3）。
fn mask_password(password: &str) -> String {
    let chars: Vec<char> = password.chars().collect();
    let start = chars.len().saturating_sub(2);
    format!("****{}", chars[start..].iter().collect::<String>())
}

// ==================== 发送留痕与批量发送 ====================

const NOTIFICATION_LOG_DEFAULT_LIMIT: i64 = 200;

/// 写一条通知发送留痕（notification_logs 只追加，spec 3.2），返回新记录 id。
/// channel/status 在应用层先行校验，给出比 CHECK 约束更明确的中文报错。
#[allow(clippy::too_many_arguments)]
pub fn insert_notification_log(
    conn: &Connection,
    channel: &str,
    employee_id: Option<i64>,
    recipient: &str,
    belong_month: Option<&str>,
    subject: &str,
    status: &str,
    error_msg: Option<&str>,
    operator: Option<&str>,
) -> AppResult<i64> {
    if !NOTIFICATION_CHANNELS.contains(&channel) {
        return Err(AppError::InvalidParam("通知渠道仅支持 email / sms".into()));
    }
    if !NOTIFICATION_STATUSES.contains(&status) {
        return Err(AppError::InvalidParam(
            "通知状态仅支持 sent / failed / skipped".into(),
        ));
    }
    conn.execute(
        "INSERT INTO notification_logs
            (channel, employee_id, recipient, belong_month, subject, status, error_msg, operator, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            channel,
            employee_id,
            recipient,
            belong_month,
            subject,
            status,
            error_msg,
            operator,
            Utc::now().to_rfc3339()
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 查询通知发送记录（spec 3.2 / Task 2）：channel/belong_month/status 可选筛选，
/// 按 id 倒序（最新在前），limit<=0 时取默认 200。
pub fn get_notification_logs(
    conn: &Connection,
    query: &NotificationLogQuery,
) -> AppResult<Vec<NotificationLog>> {
    let mut where_clauses: Vec<String> = Vec::new();
    let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    for (field, value) in [
        ("channel", query.channel.as_deref()),
        ("belong_month", query.belong_month.as_deref()),
        ("status", query.status.as_deref()),
    ] {
        if let Some(v) = value.filter(|s| !s.is_empty()) {
            params_vec.push(Box::new(v.to_string()));
            where_clauses.push(format!("{field} = ?{}", params_vec.len()));
        }
    }
    let where_sql = if where_clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", where_clauses.join(" AND "))
    };
    let limit = if query.limit == 0 {
        NOTIFICATION_LOG_DEFAULT_LIMIT
    } else {
        i64::from(query.limit)
    };
    let sql = format!(
        "SELECT id, channel, employee_id, recipient, belong_month, subject, status, error_msg,
                operator, created_at
         FROM notification_logs{where_sql} ORDER BY id DESC LIMIT {limit}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(
        params_from_iter(params_vec.iter().map(|b| b.as_ref())),
        |row| {
            Ok(NotificationLog {
                id: row.get(0)?,
                channel: row.get(1)?,
                employee_id: row.get(2)?,
                recipient: row.get(3)?,
                belong_month: row.get(4)?,
                subject: row.get(5)?,
                status: row.get(6)?,
                error_msg: row.get(7)?,
                operator: row.get(8)?,
                created_at: row.get(9)?,
            })
        },
    )?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// 逐封发送并留痕（spec 4.2）：send 成功 → 写 sent；失败 → 写 failed+error_msg
/// 且不中断后续；空收件地址 → 写 skipped。整批结束返回汇总。
///
/// 注意（DB 锁，spec 4.2）：本函数收 `&Connection`，命令层（Task 3/5）须避免
/// 持 `Mutex<Connection>` 守卫跨网络 IO——先取数释放锁，再以独立连接调用本函数。
pub fn batch_send(
    conn: &Connection,
    channel: &dyn NotifyChannel,
    batch: Vec<MessageItem>,
    operator: &str,
) -> AppResult<BatchSummary> {
    let mut summary = BatchSummary::default();
    for item in batch {
        let log_recipient = item.recipient.trim().to_string();
        if log_recipient.is_empty() || item.message.recipient.trim().is_empty() {
            insert_notification_log(
                conn,
                channel.name(),
                item.employee_id,
                &log_recipient,
                item.belong_month.as_deref(),
                &item.message.subject,
                NOTIFICATION_STATUS_SKIPPED,
                Some("收件地址为空，已跳过"),
                Some(operator),
            )?;
            summary.skipped += 1;
            continue;
        }
        match channel.send(&item.message) {
            Ok(()) => {
                insert_notification_log(
                    conn,
                    channel.name(),
                    item.employee_id,
                    &log_recipient,
                    item.belong_month.as_deref(),
                    &item.message.subject,
                    NOTIFICATION_STATUS_SENT,
                    None,
                    Some(operator),
                )?;
                summary.sent += 1;
            }
            Err(e) => {
                let id = insert_notification_log(
                    conn,
                    channel.name(),
                    item.employee_id,
                    &log_recipient,
                    item.belong_month.as_deref(),
                    &item.message.subject,
                    NOTIFICATION_STATUS_FAILED,
                    Some(&e.to_string()),
                    Some(operator),
                )?;
                summary.failed += 1;
                summary.failed_log_ids.push(id);
            }
        }
    }
    Ok(summary)
}

/// 重发失败通知（spec 4.2）：仅取 `status='failed'` 且渠道匹配的留痕记录，
/// 以留痕字段（收件人/主题）重建消息重发并**写新记录**（原记录不动，
/// 留痕表只追加）。正文不落库，重建消息不含正文——需要完整正文的场景
/// （工资条）由 Task 5 组装层重新生成后走 batch_send。
/// 请求 id 中非 failed / 渠道不匹配 / 不存在者计入 skipped。
pub fn resend_failed(
    conn: &Connection,
    channel: &dyn NotifyChannel,
    log_ids: &[i64],
    operator: &str,
) -> AppResult<BatchSummary> {
    let mut summary = BatchSummary::default();
    for &log_id in log_ids {
        let row = conn.query_row(
            "SELECT employee_id, recipient, belong_month, subject
             FROM notification_logs
             WHERE id = ?1 AND status = ?2 AND channel = ?3",
            params![log_id, NOTIFICATION_STATUS_FAILED, channel.name()],
            |r| {
                Ok((
                    r.get::<_, Option<i64>>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, String>(3)?,
                ))
            },
        );
        let (employee_id, recipient, belong_month, subject) = match row {
            Ok(v) => v,
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                summary.skipped += 1;
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        let message = NotifyMessage {
            recipient: recipient.clone(),
            subject,
            html_body: None,
            text_body: None,
        };
        match channel.send(&message) {
            Ok(()) => {
                insert_notification_log(
                    conn,
                    channel.name(),
                    employee_id,
                    &recipient,
                    belong_month.as_deref(),
                    &message.subject,
                    NOTIFICATION_STATUS_SENT,
                    None,
                    Some(operator),
                )?;
                summary.sent += 1;
            }
            Err(e) => {
                let new_id = insert_notification_log(
                    conn,
                    channel.name(),
                    employee_id,
                    &recipient,
                    belong_month.as_deref(),
                    &message.subject,
                    NOTIFICATION_STATUS_FAILED,
                    Some(&e.to_string()),
                    Some(operator),
                )?;
                summary.failed += 1;
                summary.failed_log_ids.push(new_id);
            }
        }
    }
    Ok(summary)
}

// ==================== 测试（FakeChannel 注入，不真发） ====================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{NOTIFICATION_STATUS_FAILED as FAILED, NOTIFICATION_STATUS_SENT as SENT};
    use std::collections::VecDeque;
    use std::sync::Mutex;

    /// 最小表结构：app_settings / operation_logs / notification_logs（DDL 与 db.rs 一致）
    fn setup_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "
            CREATE TABLE app_settings (key TEXT PRIMARY KEY, value TEXT);
            CREATE TABLE operation_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                operation_type TEXT NOT NULL,
                description TEXT,
                operator TEXT,
                detail TEXT,
                created_at TEXT
            );
            CREATE TABLE notification_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                channel TEXT NOT NULL CHECK (channel IN ('email','sms')),
                employee_id INTEGER,
                recipient TEXT NOT NULL,
                belong_month TEXT,
                subject TEXT NOT NULL,
                status TEXT NOT NULL CHECK (status IN ('sent','failed','skipped')),
                error_msg TEXT,
                operator TEXT,
                created_at TEXT NOT NULL
            );
            ",
        )
        .unwrap();
        conn
    }

    /// 已装载 DEK 的安全状态（经 cfg(test) 注入口，不依赖完整 setup 流程）
    fn sec_with_dek() -> SecurityState {
        let sec = SecurityState::new();
        sec.install_dek_for_test([7u8; 32]);
        sec
    }

    fn sample_config() -> SmtpConfig {
        SmtpConfig {
            host: "smtp.qq.com".into(),
            port: 465,
            encryption: "ssl".into(),
            username: "salary@example.com".into(),
            password: "AppPass-99x".into(),
            from_name: "工资专员".into(),
        }
    }

    fn msg(recipient: &str, subject: &str) -> NotifyMessage {
        NotifyMessage {
            recipient: recipient.into(),
            subject: subject.into(),
            html_body: Some("<p>body</p>".into()),
            text_body: None,
        }
    }

    fn item(employee_id: i64, recipient: &str, month: &str, subject: &str) -> MessageItem {
        MessageItem {
            employee_id: Some(employee_id),
            recipient: recipient.into(),
            belong_month: Some(month.into()),
            message: msg(recipient, subject),
        }
    }

    /// 脚本化假通道：按序弹出发送结果（耗尽后默认成功），并记录收到的消息
    struct FakeChannel {
        results: Mutex<VecDeque<AppResult<()>>>,
        sent: Mutex<Vec<NotifyMessage>>,
    }

    impl FakeChannel {
        fn new(results: Vec<AppResult<()>>) -> Self {
            Self {
                results: Mutex::new(results.into()),
                sent: Mutex::new(Vec::new()),
            }
        }

        fn sent_recipients(&self) -> Vec<String> {
            self.sent
                .lock()
                .unwrap()
                .iter()
                .map(|m| m.recipient.clone())
                .collect()
        }
    }

    impl NotifyChannel for FakeChannel {
        fn name(&self) -> &'static str {
            "email"
        }

        fn send(&self, message: &NotifyMessage) -> AppResult<()> {
            self.sent.lock().unwrap().push(message.clone());
            match self.results.lock().unwrap().pop_front() {
                Some(r) => r,
                None => Ok(()),
            }
        }
    }

    fn count_by_status(conn: &Connection, status: &str) -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM notification_logs WHERE status = ?1",
            params![status],
            |r| r.get(0),
        )
        .unwrap()
    }

    // ==================== SMTP 配置加密存取 ====================

    #[test]
    fn smtp_config_round_trip_and_ciphertext_at_rest() {
        let conn = setup_conn();
        let sec = sec_with_dek();
        let config = sample_config();

        set_smtp_config(&conn, &config, "管理员", &sec).unwrap();

        // app_settings 表内为密文：不含明文授权码，也不含明文 host（整键 JSON 加密）
        let raw = crate::db::get_setting(&conn, "smtp_config")
            .unwrap()
            .expect("smtp_config 键应存在");
        assert!(!raw.contains("AppPass-99x"), "落库值不得含明文授权码");
        assert!(!raw.contains("smtp.qq.com"), "落库值不得含明文 host");
        assert!(!raw.contains("salary@example.com"), "落库值不得含明文账号");

        // 读回解密与原配置一致
        let loaded = get_smtp_config(&conn, &sec).unwrap().expect("应能读回配置");
        assert_eq!(loaded.host, "smtp.qq.com");
        assert_eq!(loaded.port, 465);
        assert_eq!(loaded.encryption, "ssl");
        assert_eq!(loaded.username, "salary@example.com");
        assert_eq!(loaded.password, "AppPass-99x");
        assert_eq!(loaded.from_name, "工资专员");
    }

    #[test]
    fn smtp_config_missing_returns_none_without_dek() {
        let conn = setup_conn();
        let sec = SecurityState::new(); // 未装载 DEK
        assert!(get_smtp_config(&conn, &sec).unwrap().is_none());
        assert!(get_smtp_config_masked(&conn, &sec).unwrap().is_none());
    }

    #[test]
    fn smtp_config_requires_unlocked_dek() {
        let conn = setup_conn();
        let locked = SecurityState::new();
        let err = set_smtp_config(&conn, &sample_config(), "管理员", &locked).unwrap_err();
        assert!(err.to_string().contains("安全模块未解锁"));

        // 已落库但锁屏后（DEK 卸载）读取 → 显式报错而非明文/半截数据
        let sec = sec_with_dek();
        set_smtp_config(&conn, &sample_config(), "管理员", &sec).unwrap();
        let err = get_smtp_config(&conn, &locked).unwrap_err();
        assert!(err.to_string().contains("安全模块未解锁"));
    }

    #[test]
    fn smtp_config_masked_reply() {
        let conn = setup_conn();
        let sec = sec_with_dek();
        set_smtp_config(&conn, &sample_config(), "管理员", &sec).unwrap();

        let masked = get_smtp_config_masked(&conn, &sec)
            .unwrap()
            .expect("应有配置");
        assert_eq!(masked.password_masked, "****9x");
        assert_eq!(masked.host, "smtp.qq.com");
        assert_eq!(masked.username, "salary@example.com");
        // 脱敏结构不携带真实授权码
        let json = serde_json::to_string(&masked).unwrap();
        assert!(!json.contains("AppPass-99x"));

        // 未设置授权码（空串）→ `****` 不带尾缀
        let mut no_pass = sample_config();
        no_pass.password = String::new();
        set_smtp_config(&conn, &no_pass, "管理员", &sec).unwrap();
        let masked = get_smtp_config_masked(&conn, &sec).unwrap().unwrap();
        assert_eq!(masked.password_masked, "****");
    }

    #[test]
    fn smtp_config_validation_rejects_bad_input() {
        let conn = setup_conn();
        let sec = sec_with_dek();

        // port 超范围（u16 下仅 0 非法，1-65535 合法）
        let mut bad = sample_config();
        bad.port = 0;
        assert!(set_smtp_config(&conn, &bad, "管理员", &sec).is_err());

        // encryption 白名单外
        let mut bad = sample_config();
        bad.encryption = "tls".into();
        assert!(set_smtp_config(&conn, &bad, "管理员", &sec).is_err());

        // host / username 非空
        let mut bad = sample_config();
        bad.host = "  ".into();
        assert!(set_smtp_config(&conn, &bad, "管理员", &sec).is_err());
        let mut bad = sample_config();
        bad.username = String::new();
        assert!(set_smtp_config(&conn, &bad, "管理员", &sec).is_err());

        // 拒绝后不落任何配置
        assert!(crate::db::get_setting(&conn, "smtp_config")
            .unwrap()
            .is_none());
    }

    #[test]
    fn smtp_config_update_overwrites_and_logs_operation() {
        let conn = setup_conn();
        let sec = sec_with_dek();
        set_smtp_config(&conn, &sample_config(), "管理员", &sec).unwrap();
        let mut updated = sample_config();
        updated.host = "smtp.163.com".into();
        updated.port = 25;
        updated.encryption = "starttls".into();
        set_smtp_config(&conn, &updated, "管理员", &sec).unwrap();

        let loaded = get_smtp_config(&conn, &sec).unwrap().unwrap();
        assert_eq!(loaded.host, "smtp.163.com");
        assert_eq!(loaded.port, 25);

        let log_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM operation_logs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(log_count, 2, "两次保存各留痕一条");
        let (op_type, detail): (String, String) = conn
            .query_row(
                "SELECT operation_type, detail FROM operation_logs ORDER BY id DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(op_type, "smtp_config_update");
        assert_eq!(detail, "smtp.163.com:25:starttls");
        assert!(!detail.contains("AppPass"));
    }

    // ==================== 短信通道占位 ====================

    #[test]
    fn sms_channel_placeholder_error() {
        let channel = SmsChannel;
        assert_eq!(channel.name(), "sms");
        let err = channel.send(&msg("13800000000", "工资条")).unwrap_err();
        assert_eq!(err.to_string(), "短信通道未配置，请在设置中接入服务商");
    }

    // ==================== 批量发送与重发 ====================

    #[test]
    fn batch_send_mixed_results_and_failed_log_ids() {
        let conn = setup_conn();
        let channel = FakeChannel::new(vec![
            Ok(()),
            Err(AppError::General("SMTP 服务器拒绝连接".into())),
            Ok(()),
        ]);

        let batch = vec![
            item(1, "a@example.com", "2026-09", "9月工资条"),
            item(2, "b@example.com", "2026-09", "9月工资条"),
            item(3, "c@example.com", "2026-09", "9月工资条"),
        ];
        let summary = batch_send(&conn, &channel, batch, "管理员").unwrap();

        assert_eq!(summary.sent, 2);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.skipped, 0);
        assert_eq!(summary.failed_log_ids.len(), 1);
        assert_eq!(
            channel.sent_recipients(),
            vec![
                "a@example.com".to_string(),
                "b@example.com".to_string(),
                "c@example.com".to_string()
            ]
        );

        // 留痕：2 sent + 1 failed，失败记录带错误信息与失败 id
        assert_eq!(count_by_status(&conn, SENT), 2);
        assert_eq!(count_by_status(&conn, FAILED), 1);
        let failed: (String, String) = conn
            .query_row(
                "SELECT error_msg, recipient FROM notification_logs WHERE id = ?1",
                params![summary.failed_log_ids[0]],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(failed.0, "SMTP 服务器拒绝连接");
        assert_eq!(failed.1, "b@example.com");

        // 所有留痕署名操作人、渠道与账期正确
        let operator: String = conn
            .query_row(
                "SELECT operator FROM notification_logs WHERE id = ?1",
                params![summary.failed_log_ids[0]],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(operator, "管理员");
        let month: String = conn
            .query_row(
                "SELECT belong_month FROM notification_logs WHERE id = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(month, "2026-09");
    }

    #[test]
    fn batch_send_skips_blank_recipient() {
        let conn = setup_conn();
        let channel = FakeChannel::new(vec![]);
        let batch = vec![
            item(1, "a@example.com", "2026-09", "9月工资条"),
            MessageItem {
                employee_id: Some(2),
                recipient: "   ".into(),
                belong_month: Some("2026-09".into()),
                message: msg("", "9月工资条"),
            },
        ];
        let summary = batch_send(&conn, &channel, batch, "管理员").unwrap();

        assert_eq!(summary.sent, 1);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.skipped, 1);
        assert!(channel.sent_recipients().len() == 1, "空地址不得发起发送");
        assert_eq!(count_by_status(&conn, NOTIFICATION_STATUS_SKIPPED), 1);
        let error_msg: String = conn
            .query_row(
                "SELECT error_msg FROM notification_logs WHERE status = 'skipped'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(error_msg.contains("收件地址为空"));
    }

    #[test]
    fn batch_send_empty_batch_returns_zero_summary() {
        let conn = setup_conn();
        let channel = FakeChannel::new(vec![]);
        let summary = batch_send(&conn, &channel, Vec::new(), "管理员").unwrap();
        assert_eq!((summary.sent, summary.failed, summary.skipped), (0, 0, 0));
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM notification_logs", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn resend_failed_only_failed_and_keeps_sent_untouched() {
        let conn = setup_conn();
        // 首轮：1 成功 / 1 失败 / 1 成功
        let channel = FakeChannel::new(vec![Ok(()), Err(AppError::General("超时".into())), Ok(())]);
        let batch = vec![
            item(1, "a@example.com", "2026-09", "9月工资条"),
            item(2, "b@example.com", "2026-09", "9月工资条"),
            item(3, "c@example.com", "2026-09", "9月工资条"),
        ];
        let first = batch_send(&conn, &channel, batch, "管理员").unwrap();
        assert_eq!(first.failed_log_ids.len(), 1);
        let failed_id = first.failed_log_ids[0];
        let total_before: i64 = conn
            .query_row("SELECT COUNT(*) FROM notification_logs", [], |r| r.get(0))
            .unwrap();

        // 重发：混入成功记录 id 与不存在 id，仅 failed 者被重发
        let resend_channel = FakeChannel::new(vec![Ok(())]);
        let summary =
            resend_failed(&conn, &resend_channel, &[failed_id, 1, 999], "出纳小王").unwrap();
        assert_eq!(summary.sent, 1);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.skipped, 2, "sent 记录与不存在 id 应计入 skipped");
        // 重发消息按留痕字段重建（收件人/主题，无正文）
        let resent = resend_channel.sent.lock().unwrap();
        assert_eq!(resent.len(), 1);
        assert_eq!(resent[0].recipient, "b@example.com");
        assert_eq!(resent[0].subject, "9月工资条");
        assert_eq!(resent[0].html_body, None);
        drop(resent);

        // 留痕只追加：sent 记录不动，仅新增一条新 sent 记录
        let total_after: i64 = conn
            .query_row("SELECT COUNT(*) FROM notification_logs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total_after, total_before + 1);
        let sent_of_a: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM notification_logs WHERE recipient = 'a@example.com'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sent_of_a, 1, "成功记录不得被重发/改写");
        let new_row: (String, String, String) = conn
            .query_row(
                "SELECT status, recipient, operator FROM notification_logs
                 WHERE id > ?1 ORDER BY id DESC LIMIT 1",
                params![total_before],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(new_row.0, SENT);
        assert_eq!(new_row.1, "b@example.com");
        assert_eq!(new_row.2, "出纳小王", "重发新记录署名当次操作人");
    }

    #[test]
    fn resend_failed_channel_mismatch_is_skipped() {
        let conn = setup_conn();
        // 预置一条 sms 渠道的失败留痕
        let sms_log_id = insert_notification_log(
            &conn,
            "sms",
            Some(9),
            "13800000000",
            Some("2026-09"),
            "9月工资条",
            FAILED,
            Some("短信通道未配置，请在设置中接入服务商"),
            Some("管理员"),
        )
        .unwrap();

        let email_channel = FakeChannel::new(vec![]);
        let summary = resend_failed(&conn, &email_channel, &[sms_log_id], "管理员").unwrap();
        assert_eq!(
            (summary.sent, summary.failed, summary.skipped),
            (0, 0, 1),
            "渠道不匹配的失败记录不得经邮件通道重发"
        );
        assert!(email_channel.sent_recipients().is_empty());
    }

    #[test]
    fn resend_failed_second_failure_writes_new_failed_id() {
        let conn = setup_conn();
        let first_id = insert_notification_log(
            &conn,
            "email",
            Some(1),
            "a@example.com",
            Some("2026-09"),
            "9月工资条",
            FAILED,
            Some("超时"),
            Some("管理员"),
        )
        .unwrap();

        let channel = FakeChannel::new(vec![Err(AppError::General("再次失败".into()))]);
        let summary = resend_failed(&conn, &channel, &[first_id], "管理员").unwrap();
        assert_eq!(summary.failed, 1);
        assert_ne!(summary.failed_log_ids[0], first_id, "重发写新记录");
        let status: String = conn
            .query_row(
                "SELECT status FROM notification_logs WHERE id = ?1",
                params![summary.failed_log_ids[0]],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, FAILED);
    }

    // ==================== 留痕查询与校验 ====================

    #[test]
    fn notification_log_insert_validates_channel_and_status() {
        let conn = setup_conn();
        assert!(insert_notification_log(
            &conn,
            "fax",
            None,
            "x@example.com",
            None,
            "主题",
            SENT,
            None,
            None
        )
        .is_err());
        assert!(insert_notification_log(
            &conn,
            "email",
            None,
            "x@example.com",
            None,
            "主题",
            "pending",
            None,
            None
        )
        .is_err());
        let id = insert_notification_log(
            &conn,
            "email",
            None,
            "x@example.com",
            None,
            "主题",
            SENT,
            None,
            None,
        )
        .unwrap();
        assert!(id > 0);
    }

    #[test]
    fn get_notification_logs_filters_and_order() {
        let conn = setup_conn();
        // 依次写入 4 条：email/9月/sent、email/9月/failed、email/8月/sent、sms/9月/failed
        insert_notification_log(
            &conn,
            "email",
            Some(1),
            "a@example.com",
            Some("2026-09"),
            "9月工资条",
            SENT,
            None,
            Some("管理员"),
        )
        .unwrap();
        let failed_id = insert_notification_log(
            &conn,
            "email",
            Some(2),
            "b@example.com",
            Some("2026-09"),
            "9月工资条",
            FAILED,
            Some("超时"),
            Some("管理员"),
        )
        .unwrap();
        insert_notification_log(
            &conn,
            "email",
            Some(3),
            "c@example.com",
            Some("2026-08"),
            "8月工资条",
            SENT,
            None,
            Some("管理员"),
        )
        .unwrap();
        insert_notification_log(
            &conn,
            "sms",
            Some(4),
            "13800000000",
            Some("2026-09"),
            "9月工资条",
            FAILED,
            Some("短信通道未配置，请在设置中接入服务商"),
            Some("管理员"),
        )
        .unwrap();

        // 不筛选：全部 4 条，id 倒序
        let all = get_notification_logs(&conn, &NotificationLogQuery::default()).unwrap();
        assert_eq!(all.len(), 4);
        assert!(all.windows(2).all(|w| w[0].id > w[1].id), "应按 id 倒序");

        // channel 筛选
        let email_only = get_notification_logs(
            &conn,
            &NotificationLogQuery {
                channel: Some("email".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(email_only.len(), 3);
        assert!(email_only.iter().all(|l| l.channel == "email"));

        // status 筛选
        let failed_only = get_notification_logs(
            &conn,
            &NotificationLogQuery {
                status: Some(FAILED.into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(failed_only.len(), 2);
        assert!(failed_only.iter().all(|l| l.status == FAILED));
        assert_eq!(failed_only[0].id, 4, "倒序下最新失败记录（sms）在前");
        assert!(failed_only.iter().any(|l| l.id == failed_id));

        // belong_month 筛选
        let month_only = get_notification_logs(
            &conn,
            &NotificationLogQuery {
                belong_month: Some("2026-08".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(month_only.len(), 1);
        assert_eq!(month_only[0].recipient, "c@example.com");

        // 组合筛选：email + 2026-09 + failed
        let combined = get_notification_logs(
            &conn,
            &NotificationLogQuery {
                channel: Some("email".into()),
                belong_month: Some("2026-09".into()),
                status: Some(FAILED.into()),
                limit: 0,
            },
        )
        .unwrap();
        assert_eq!(combined.len(), 1);
        assert_eq!(combined[0].id, failed_id);
        assert_eq!(combined[0].error_msg.as_deref(), Some("超时"));

        // limit 生效
        let limited = get_notification_logs(
            &conn,
            &NotificationLogQuery {
                limit: 2,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(limited.len(), 2);
    }

    #[test]
    fn batch_send_writes_channel_name_from_trait() {
        let conn = setup_conn();
        let channel = FakeChannel::new(vec![Ok(())]);
        batch_send(
            &conn,
            &channel,
            vec![item(1, "a@example.com", "2026-09", "9月工资条")],
            "管理员",
        )
        .unwrap();
        let channel_name: String = conn
            .query_row(
                "SELECT channel FROM notification_logs WHERE id = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(channel_name, "email");
    }
}
