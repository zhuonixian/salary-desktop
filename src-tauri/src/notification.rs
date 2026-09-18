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
use lettre::{Message, SmtpTransport, Transport};
use rusqlite::{params, params_from_iter, Connection};
use serde::{Deserialize, Serialize};

use crate::db::{get_setting, log_operation, set_setting};
use crate::errors::{AppError, AppResult};
use crate::models::{
    NotificationLog, NotificationLogQuery, SmtpConfig, SmtpConfigMasked, NOTIFICATION_CHANNELS,
    NOTIFICATION_CHANNEL_EMAIL, NOTIFICATION_STATUSES, NOTIFICATION_STATUS_FAILED,
    NOTIFICATION_STATUS_SENT, NOTIFICATION_STATUS_SKIPPED,
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

// ==================== EmailChannel（lettre blocking，Task 3） ====================

/// SMTP 发送超时（spec 4.2）：连接/整次会话 30s 封顶，避免界面长时间卡在网络 IO。
const SMTP_SEND_TIMEOUT_SECS: u64 = 30;

/// SMTP 535 = AUTH 凭证被拒：授权码错误，或误用登录密码（QQ/163 最常见）。
const SMTP_AUTH_DENIED_CODE: &str = "535";

/// 535 的中文业务提示（spec 7）
pub const SMTP_AUTH_DENIED_HINT: &str =
    "授权码错误，请检查邮箱设置（QQ/163 需使用授权码而非登录密码）";

/// 邮件通道（spec 4.1）：持有解密后的 SMTP 配置，lettre blocking 实现。
/// 加密方式映射：starttls→STARTTLS / ssl→隐式 TLS(465) / none→明文。
/// 真发不进单测（spec 8）：单测经 [`NotifyChannel`] 注入 FakeChannel。
pub struct EmailChannel {
    config: SmtpConfig,
}

impl EmailChannel {
    pub fn new(config: SmtpConfig) -> Self {
        Self { config }
    }
}

/// SMTP 错误 → 中文业务错误（纯函数可单测，不触网）：
/// 状态码或错误文本含 535 → 授权码错误提示；其余 → 网络错误透传原文
/// （spec 7：超时/网络错误 failed + 原因入档）。
fn smtp_error_to_app_error(status_code: Option<&str>, raw: &str) -> AppError {
    if status_code == Some(SMTP_AUTH_DENIED_CODE) || raw.contains(SMTP_AUTH_DENIED_CODE) {
        return AppError::General(SMTP_AUTH_DENIED_HINT.into());
    }
    AppError::Network(raw.to_string())
}

fn translate_smtp_error(error: lettre::transport::smtp::Error) -> AppError {
    smtp_error_to_app_error(
        error.status().map(|c| c.to_string()).as_deref(),
        &error.to_string(),
    )
}

impl NotifyChannel for EmailChannel {
    fn name(&self) -> &'static str {
        NOTIFICATION_CHANNEL_EMAIL
    }

    fn send(&self, message: &NotifyMessage) -> AppResult<()> {
        let recipient: lettre::message::Mailbox =
            message.recipient.trim().parse().map_err(|e| {
                AppError::InvalidParam(format!("邮箱地址无效 {}: {e}", message.recipient.trim()))
            })?;
        let from_mailbox: lettre::message::Mailbox = if self.config.from_name.trim().is_empty() {
            self.config
                .username
                .trim()
                .parse()
                .map_err(|e| AppError::InvalidParam(format!("发件账号邮箱地址无效: {e}")))?
        } else {
            let address: lettre::Address = self
                .config
                .username
                .trim()
                .parse()
                .map_err(|e| AppError::InvalidParam(format!("发件账号邮箱地址无效: {e}")))?;
            lettre::message::Mailbox::new(Some(self.config.from_name.trim().to_string()), address)
        };

        // 正文：html+text 双部分 / 仅 html / 纯文本；主题由 lettre 自动做 RFC 2047 编码
        let builder = Message::builder()
            .from(from_mailbox)
            .to(recipient)
            .subject(message.subject.as_str());
        let email = match (message.html_body.as_deref(), message.text_body.as_deref()) {
            (Some(html), Some(text)) if !html.trim().is_empty() && !text.trim().is_empty() => {
                builder.multipart(lettre::message::MultiPart::alternative_plain_html(
                    text.to_string(),
                    html.to_string(),
                ))
            }
            (Some(html), _) if !html.trim().is_empty() => builder
                .header(lettre::message::header::ContentType::TEXT_HTML)
                .body(html.to_string()),
            (Some(text), _) | (None, Some(text)) => builder.body(text.to_string()),
            (None, None) => {
                return Err(AppError::InvalidParam("邮件正文不能为空".into()));
            }
        }
        .map_err(|e| AppError::General(format!("邮件组装失败: {e}")))?;

        let credentials = lettre::transport::smtp::authentication::Credentials::new(
            self.config.username.trim().to_string(),
            self.config.password.clone(),
        );
        let host = self.config.host.trim();
        let transport_builder = match self.config.encryption.as_str() {
            "starttls" => SmtpTransport::starttls_relay(host)
                .map_err(|e| AppError::Network(format!("SMTP 服务器地址或 TLS 配置无效: {e}")))?,
            "ssl" => SmtpTransport::relay(host)
                .map_err(|e| AppError::Network(format!("SMTP 服务器地址或 TLS 配置无效: {e}")))?,
            _ => SmtpTransport::builder_dangerous(host),
        };
        let mailer = transport_builder
            .port(self.config.port)
            .credentials(credentials)
            .timeout(Some(std::time::Duration::from_secs(SMTP_SEND_TIMEOUT_SECS)));
        mailer
            .build()
            .send(&email)
            .map(|_| ())
            .map_err(translate_smtp_error)
    }
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

// ==================== 工资条组装与批量发送（Task 5，spec 5） ====================

/// 工资条邮件标题模板（spec 5：`{YYYY-MM} 工资条`，固定不新增配置项）
fn payslip_subject(month: &str) -> String {
    format!("{month} 工资条")
}

/// 敏感解锁门禁的中文报错（spec 5 步骤 2 原文案）
const PAYSLIP_REVEAL_REQUIRED_MSG: &str = "工资明细为明文，请先解锁敏感数据";

/// 敏感数据解锁门禁（spec 5 步骤 2）：读取 SecurityState 上的 reveal 到期
/// 时间戳（reveal_sensitive_data 成功后设置，lock 即失效）。preview/send/resend
/// 三个明文出口共用，避免只靠前端门禁。
pub fn require_sensitive_revealed(sec: &SecurityState) -> AppResult<()> {
    if sec.is_sensitive_revealed() {
        Ok(())
    } else {
        Err(AppError::General(PAYSLIP_REVEAL_REQUIRED_MSG.into()))
    }
}

/// 月份锁定门禁（spec 5 步骤 1）：当月工资结果须全部 locked=1。
fn ensure_payslip_month_locked(conn: &Connection, month: &str) -> AppResult<()> {
    let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM salary_monthly_results WHERE salary_month = ?1",
        params![month],
        |r| r.get(0),
    )?;
    if total == 0 {
        return Err(AppError::General(format!(
            "{month} 尚未生成工资结果，请先计算并锁定工资"
        )));
    }
    let unlocked: i64 = conn.query_row(
        "SELECT COUNT(*) FROM salary_monthly_results WHERE salary_month = ?1 AND locked != 1",
        params![month],
        |r| r.get(0),
    )?;
    if unlocked > 0 {
        return Err(AppError::General(format!(
            "{month} 工资尚未锁定，请先在工资计算页锁定后再发送工资条"
        )));
    }
    Ok(())
}

/// SMTP 已配置门禁（spec 4.3 入口拦截；与 commands::require_smtp_config 同文案）。
fn require_smtp_ready(conn: &Connection, sec: &SecurityState) -> AppResult<()> {
    if get_smtp_config(conn, sec)?.is_some() {
        Ok(())
    } else {
        Err(AppError::General(
            "尚未配置 SMTP，请先在通知设置中保存邮箱配置".into(),
        ))
    }
}

/// 工资条单员工明细行：salary_monthly_results 联查 employees（email/姓名）。
/// 金额字段与 SalaryCalculate.tsx payslip 卡片一一对应。
struct PayslipRow {
    employee_no: String,
    name: String,
    email: Option<String>,
    base_salary: f64,
    position_salary: f64,
    performance_salary: f64,
    overtime_salary: f64,
    meal_allowance: f64,
    transport_allowance: f64,
    gross_salary: f64,
    social_security_personal: f64,
    housing_fund_personal: f64,
    attendance_deduction: f64,
    tax_amount: f64,
    other_deduction: f64,
    net_salary: f64,
}

/// 按员工 id 取某月工资结果：salary_monthly_results.employee_no 关联
/// employees（employee_no UNIQUE），同一员工多行结果时取最早一条。
fn fetch_payslip_row(conn: &Connection, month: &str, employee_id: i64) -> AppResult<PayslipRow> {
    conn.query_row(
        "SELECT sr.employee_no, e.name, e.email,
                sr.base_salary, sr.position_salary, sr.performance_salary,
                sr.overtime_salary, sr.meal_allowance, sr.transport_allowance,
                sr.gross_salary, sr.social_security_personal, sr.housing_fund_personal,
                sr.attendance_deduction, sr.tax_amount, sr.other_deduction, sr.net_salary
         FROM salary_monthly_results sr
         JOIN employees e ON e.employee_no = sr.employee_no
         WHERE sr.salary_month = ?1 AND e.id = ?2
         ORDER BY sr.id
         LIMIT 1",
        params![month, employee_id],
        |r| {
            Ok(PayslipRow {
                employee_no: r.get(0)?,
                name: r.get(1)?,
                email: r.get(2)?,
                base_salary: r.get(3)?,
                position_salary: r.get(4)?,
                performance_salary: r.get(5)?,
                overtime_salary: r.get(6)?,
                meal_allowance: r.get(7)?,
                transport_allowance: r.get(8)?,
                gross_salary: r.get(9)?,
                social_security_personal: r.get(10)?,
                housing_fund_personal: r.get(11)?,
                attendance_deduction: r.get(12)?,
                tax_amount: r.get(13)?,
                other_deduction: r.get(14)?,
                net_salary: r.get(15)?,
            })
        },
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => AppError::NotFound(format!(
            "员工（ID={employee_id}）在 {month} 无工资结果，请先计算工资"
        )),
        other => other.into(),
    })
}

/// HTML 转义：员工姓名/工号来自用户输入，防止注入邮件 HTML。
fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// 金额格式化：`¥ 1,234.56`，负数 `¥ -1,234.56`（两位小数千分位）。
/// 扣款项为取负展示，0 取负后是 -0.0，须归一避免渲染出 `-0.00`。
fn format_money(value: f64) -> String {
    let value = if value == 0.0 { 0.0 } else { value };
    let fixed = format!("{value:.2}");
    let (sign, rest) = match fixed.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", fixed.as_str()),
    };
    let (int_part, dec_part) = rest.split_once('.').unwrap_or((rest, "00"));
    let mut grouped = String::with_capacity(int_part.len() + int_part.len() / 3);
    for (index, ch) in int_part.chars().enumerate() {
        if index > 0 && (int_part.len() - index) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    format!("¥ {sign}{grouped}.{dec_part}")
}

/// 渲染工资条 HTML：全内联样式（邮件客户端不加载外部 CSS），表格行序与
/// SalaryCalculate.tsx payslip 卡片一致——发放项 → 应发合计 → 五险一金个人
/// 与各项扣款（负数展示）→ 实发工资（加粗收尾）。
fn render_payslip_html(month: &str, row: &PayslipRow) -> String {
    let cell = "border:1px solid #d9d9d9;padding:4px 8px;";
    let amount_cell = "border:1px solid #d9d9d9;padding:4px 8px;text-align:right;";
    let rows: [(&str, f64); 12] = [
        ("基本工资", row.base_salary),
        ("岗位工资", row.position_salary),
        ("绩效工资", row.performance_salary),
        ("加班费", row.overtime_salary),
        ("餐补", row.meal_allowance),
        ("交通补贴", row.transport_allowance),
        ("应发合计", row.gross_salary),
        ("社保(个人)", -row.social_security_personal),
        ("公积金(个人)", -row.housing_fund_personal),
        ("考勤扣款", -row.attendance_deduction),
        ("个税", -row.tax_amount),
        ("其他扣款", -row.other_deduction),
    ];
    let mut body = String::new();
    for (label, value) in rows {
        body.push_str(&format!(
            "<tr><td style=\"{cell}\">{label}</td><td style=\"{amount_cell}\">{}</td></tr>",
            format_money(value)
        ));
    }
    body.push_str(&format!(
        "<tr><td style=\"border:1px solid #333;padding:6px 8px;font-weight:700;\">实发工资</td>\
         <td style=\"border:1px solid #333;padding:6px 8px;text-align:right;font-weight:700;\">{}</td></tr>",
        format_money(row.net_salary)
    ));
    format!(
        "<div style=\"max-width:640px;margin:0 auto;font-family:'Microsoft YaHei','PingFang SC',Arial,sans-serif;color:#333;\">\
         <h2 style=\"text-align:center;margin:0 0 4px;font-size:18px;\">{month} 工资条</h2>\
         <p style=\"text-align:center;margin:0 0 12px;color:#666;font-size:13px;\">{}（{}）</p>\
         <table style=\"width:100%;border-collapse:collapse;font-size:13px;\"><tbody>{body}</tbody></table>\
         <p style=\"color:#999;font-size:12px;margin-top:12px;\">本邮件由工资核算助手发送，工资明细仅供本人核对，请勿转发。</p>\
         </div>",
        escape_html(&row.name),
        escape_html(&row.employee_no),
    )
}

/// 组装工资条邮件 HTML（spec 5 步骤 1 预览来源）：只读，不写留痕。
pub fn payslip_html(conn: &Connection, month: &str, employee_id: i64) -> AppResult<String> {
    let row = fetch_payslip_row(conn, month, employee_id)?;
    Ok(render_payslip_html(month, &row))
}

/// 无邮箱被跳过的员工（向导「缺邮箱 N 人」名单来源；计划为内部结构，
/// 命令只回传 BatchSummary，前端缺邮箱名单由员工列表自行统计）
#[derive(Debug, Clone)]
pub struct PayslipSkippedEmployee {
    pub employee_id: i64,
    pub employee_name: String,
}

/// 工资条批量发送计划：per-employee MessageItem（recipient=employees.email，
/// belong_month=month）+ 无邮箱跳过名单。构建阶段全部在 DB 锁内完成。
#[derive(Debug, Clone)]
pub struct PayslipBatchPlan {
    pub month: String,
    pub items: Vec<MessageItem>,
    pub skipped: Vec<PayslipSkippedEmployee>,
}

/// 构建工资条批量发送计划（spec 5 步骤 1）：前置校验三连——月份已锁定、
/// SMTP 已配置、敏感解锁态，任一不过即中文报错；随后逐员工组装工资条
/// HTML（无 email 者列入跳过名单，不生成消息；重复 id 去重防重发）。
///
/// 操作留痕：写 operation_logs（payslip_email_batch_send，署名 operator，
/// detail 仅含月份与人数统计，不含工资数字）。
pub fn build_payslip_batch(
    conn: &Connection,
    sec: &SecurityState,
    month: &str,
    employee_ids: &[i64],
    operator: &str,
) -> AppResult<PayslipBatchPlan> {
    if employee_ids.is_empty() {
        return Err(AppError::InvalidParam("请先勾选要发送工资条的员工".into()));
    }
    ensure_payslip_month_locked(conn, month)?;
    require_smtp_ready(conn, sec)?;
    require_sensitive_revealed(sec)?;

    let mut items = Vec::new();
    let mut skipped = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for &employee_id in employee_ids {
        if !seen.insert(employee_id) {
            continue;
        }
        let row = fetch_payslip_row(conn, month, employee_id)?;
        let Some(email) = row
            .email
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            skipped.push(PayslipSkippedEmployee {
                employee_id,
                employee_name: row.name.clone(),
            });
            continue;
        };
        let html = render_payslip_html(month, &row);
        items.push(MessageItem {
            employee_id: Some(employee_id),
            recipient: email.to_string(),
            belong_month: Some(month.to_string()),
            message: NotifyMessage {
                recipient: email.to_string(),
                subject: payslip_subject(month),
                html_body: Some(html),
                text_body: None,
            },
        });
    }
    log_operation(
        conn,
        "payslip_email_batch_send",
        "发起工资条邮件批量发送",
        operator,
        Some(&format!(
            "month={month};selected={};with_email={};no_email={}",
            employee_ids.len(),
            items.len(),
            skipped.len()
        )),
    )?;
    Ok(PayslipBatchPlan {
        month: month.to_string(),
        items,
        skipped,
    })
}

/// 发送工资条批次（spec 5 步骤 3 / 4.2）：复用 [`batch_send`] 逐封发送并留痕。
/// - 发送前复核敏感解锁态（构建与发送之间可能锁屏/到期）
/// - 收件地址不含 '@' 记 failed「邮箱地址无效」，不阻断批量（spec 7）
/// - summary.skipped 含计划中的无邮箱人数；failed_log_ids 供前端勾选重发
///
/// 注意（DB 锁）：命令层须以独立连接调用本函数，不持主连接锁跨网络 IO。
pub fn send_payslip_batch(
    conn: &Connection,
    sec: &SecurityState,
    channel: &dyn NotifyChannel,
    plan: PayslipBatchPlan,
    operator: &str,
) -> AppResult<BatchSummary> {
    require_sensitive_revealed(sec)?;
    let mut summary = BatchSummary::default();
    summary.skipped += plan.skipped.len();
    let mut batch = Vec::with_capacity(plan.items.len());
    for item in plan.items {
        let recipient = item.recipient.trim();
        if !recipient.is_empty() && !recipient.contains('@') {
            let log_id = insert_notification_log(
                conn,
                channel.name(),
                item.employee_id,
                recipient,
                item.belong_month.as_deref(),
                &item.message.subject,
                NOTIFICATION_STATUS_FAILED,
                Some("邮箱地址无效"),
                Some(operator),
            )?;
            summary.failed += 1;
            summary.failed_log_ids.push(log_id);
            continue;
        }
        batch.push(item);
    }
    let sent_summary = batch_send(conn, channel, batch, operator)?;
    summary.sent += sent_summary.sent;
    summary.failed += sent_summary.failed;
    summary.skipped += sent_summary.skipped;
    summary.failed_log_ids.extend(sent_summary.failed_log_ids);
    Ok(summary)
}

/// 重发工资条（Task 2 挂账：留痕不存正文，重发须重建）：按失败留痕的
/// employee_id 重新组装 payslip_html 后仍走 [`send_payslip_batch`]（→ batch_send）。
/// 仅 status='failed' 且渠道=email 且 belong_month 匹配且带 employee_id 的留痕
/// 可重发；其余（已成功/不存在/月份不符/无 employee_id/工资结果已清理）计入
/// skipped。员工邮箱以 employees.email 当前值为准（首轮发送后可能已改动）。
pub fn resend_payslip_batch(
    conn: &Connection,
    sec: &SecurityState,
    channel: &dyn NotifyChannel,
    month: &str,
    log_ids: &[i64],
    operator: &str,
) -> AppResult<BatchSummary> {
    if log_ids.is_empty() {
        return Err(AppError::InvalidParam("请先勾选要重发的失败记录".into()));
    }
    ensure_payslip_month_locked(conn, month)?;
    require_sensitive_revealed(sec)?;

    let mut items = Vec::new();
    let mut rebuild_skipped = 0usize;
    for &log_id in log_ids {
        let row = conn.query_row(
            "SELECT employee_id FROM notification_logs
             WHERE id = ?1 AND status = ?2 AND channel = ?3 AND belong_month = ?4",
            params![
                log_id,
                NOTIFICATION_STATUS_FAILED,
                NOTIFICATION_CHANNEL_EMAIL,
                month
            ],
            |r| r.get::<_, Option<i64>>(0),
        );
        let employee_id = match row {
            Ok(v) => v,
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                rebuild_skipped += 1;
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        let Some(employee_id) = employee_id else {
            rebuild_skipped += 1;
            continue;
        };
        let payslip = match fetch_payslip_row(conn, month, employee_id) {
            Ok(p) => p,
            Err(AppError::NotFound(_)) => {
                rebuild_skipped += 1;
                continue;
            }
            Err(e) => return Err(e),
        };
        let email = payslip.email.as_deref().unwrap_or("").trim().to_string();
        let html = render_payslip_html(month, &payslip);
        items.push(MessageItem {
            employee_id: Some(employee_id),
            recipient: email.clone(),
            belong_month: Some(month.to_string()),
            message: NotifyMessage {
                recipient: email,
                subject: payslip_subject(month),
                html_body: Some(html),
                text_body: None,
            },
        });
    }
    log_operation(
        conn,
        "payslip_email_resend",
        "重发工资条邮件",
        operator,
        Some(&format!(
            "month={month};requested={};rebuilt={}",
            log_ids.len(),
            items.len()
        )),
    )?;
    let plan = PayslipBatchPlan {
        month: month.to_string(),
        items,
        skipped: Vec::new(),
    };
    let mut summary = send_payslip_batch(conn, sec, channel, plan, operator)?;
    summary.skipped += rebuild_skipped;
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

    // ==================== EmailChannel（错误映射，不触网） ====================

    #[test]
    fn smtp_error_535_maps_to_auth_hint() {
        // 结构化状态码命中
        let err = smtp_error_to_app_error(Some("535"), "permanent error (535)");
        assert_eq!(err.to_string(), SMTP_AUTH_DENIED_HINT);
        // 无结构化状态码时按错误文本兜底识别（兼容历史 lettre 行为）
        let err = smtp_error_to_app_error(None, "permanent error (535): auth failed");
        assert_eq!(err.to_string(), SMTP_AUTH_DENIED_HINT);
        // 非 535 的网络/服务器错误透传原文入档（spec 7）
        let err = smtp_error_to_app_error(Some("421"), "transient error (421)");
        assert!(matches!(err, AppError::Network(ref msg) if msg.contains("421")));
        let err = smtp_error_to_app_error(None, "connection refused");
        assert!(matches!(err, AppError::Network(ref msg) if msg.contains("connection refused")));
    }

    #[test]
    fn email_channel_name_matches_log_check() {
        let channel = EmailChannel::new(sample_config());
        assert_eq!(channel.name(), NOTIFICATION_CHANNEL_EMAIL);
        assert!(NOTIFICATION_CHANNELS.contains(&channel.name()));
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

    // ==================== 工资条组装与批量发送（Task 5） ====================

    /// 最小表结构：在底座表之上补 employees / salary_monthly_results
    /// （列集与 fetch_payslip_row 的 SELECT 对齐）
    fn setup_payslip_conn() -> Connection {
        let conn = setup_conn();
        conn.execute_batch(
            "
            CREATE TABLE employees (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                employee_no TEXT UNIQUE NOT NULL,
                name TEXT NOT NULL,
                email TEXT
            );
            CREATE TABLE salary_monthly_results (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                salary_month TEXT NOT NULL,
                employee_no TEXT NOT NULL,
                name TEXT,
                base_salary REAL DEFAULT 0,
                position_salary REAL DEFAULT 0,
                performance_salary REAL DEFAULT 0,
                overtime_salary REAL DEFAULT 0,
                meal_allowance REAL DEFAULT 0,
                transport_allowance REAL DEFAULT 0,
                other_allowance REAL DEFAULT 0,
                gross_salary REAL DEFAULT 0,
                social_security_personal REAL DEFAULT 0,
                housing_fund_personal REAL DEFAULT 0,
                attendance_deduction REAL DEFAULT 0,
                tax_amount REAL DEFAULT 0,
                other_deduction REAL DEFAULT 0,
                net_salary REAL DEFAULT 0,
                status TEXT DEFAULT 'draft',
                locked INTEGER DEFAULT 0
            );
            ",
        )
        .unwrap();
        conn
    }

    /// 预置：E001 张三 / E002 李四（有邮箱，2026-09 已锁定）；E003 王五（无邮箱）；
    /// 2026-08 存在未锁定结果（未锁定月份用例用）。
    fn seed_payslip_data(conn: &Connection) {
        conn.execute(
            "INSERT INTO employees (id, employee_no, name, email) VALUES (1, 'E001', '张三', 'zhangsan@example.com')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO employees (id, employee_no, name, email) VALUES (2, 'E002', '李四', 'lisi@example.com')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO employees (id, employee_no, name, email) VALUES (3, 'E003', '王五', NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO salary_monthly_results
                (salary_month, employee_no, name, base_salary, position_salary, performance_salary,
                 overtime_salary, meal_allowance, transport_allowance, gross_salary,
                 social_security_personal, housing_fund_personal, attendance_deduction,
                 tax_amount, other_deduction, net_salary, locked)
             VALUES ('2026-09', 'E001', '张三', 10000.0, 2000.0, 3000.0, 500.0, 300.0, 200.0, 16000.0,
                     800.0, 1200.0, 100.0, 300.25, 50.5, 13549.25, 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO salary_monthly_results
                (salary_month, employee_no, name, base_salary, gross_salary, net_salary, locked)
             VALUES ('2026-09', 'E002', '李四', 8000.0, 8000.0, 8000.0, 1)",
            [],
        )
        .unwrap();
        // 王五：在册且计薪，但无邮箱（发送时跳过名单用例）
        conn.execute(
            "INSERT INTO salary_monthly_results
                (salary_month, employee_no, name, base_salary, gross_salary, net_salary, locked)
             VALUES ('2026-09', 'E003', '王五', 5000.0, 5000.0, 5000.0, 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO salary_monthly_results (salary_month, employee_no, name, locked)
             VALUES ('2026-08', 'E001', '张三', 0)",
            [],
        )
        .unwrap();
    }

    /// DEK 已装载且敏感已解锁（ready 状态）
    fn ready_sec() -> SecurityState {
        let sec = sec_with_dek();
        sec.mark_sensitive_revealed(300);
        sec
    }

    fn ready_smtp(conn: &Connection, sec: &SecurityState) {
        set_smtp_config(conn, &sample_config(), "管理员", sec).unwrap();
    }

    #[test]
    fn payslip_html_renders_name_amounts_and_row_order() {
        let conn = setup_payslip_conn();
        seed_payslip_data(&conn);
        let html = payslip_html(&conn, "2026-09", 1).unwrap();

        // 标题（月份 + 员工姓名/工号）
        assert!(html.contains("2026-09 工资条"), "标题: {html}");
        assert!(html.contains("张三"));
        assert!(html.contains("E001"));

        // 金额两位小数千分位：正数 / 负数扣款 / 小数 / 实发
        assert!(html.contains("¥ 10,000.00"), "基本工资: {html}");
        assert!(html.contains("¥ 16,000.00"), "应发合计: {html}");
        assert!(html.contains("¥ -800.00"), "社保个人取负展示: {html}");
        assert!(html.contains("¥ -300.25"), "个税小数千分位: {html}");
        assert!(html.contains("¥ 13,549.25"), "实发工资: {html}");

        // 行序与 payslip 卡片一致
        let order = [
            "基本工资",
            "岗位工资",
            "绩效工资",
            "加班费",
            "餐补",
            "交通补贴",
            "应发合计",
            "社保(个人)",
            "公积金(个人)",
            "考勤扣款",
            "个税",
            "其他扣款",
            "实发工资",
        ];
        let mut last = 0usize;
        for label in order {
            let pos = html
                .find(label)
                .unwrap_or_else(|| panic!("html 缺少「{label}」行"));
            assert!(pos > last, "「{label}」行序不符");
            last = pos;
        }
    }

    #[test]
    fn payslip_html_renders_second_employee_and_missing_result_errors() {
        let conn = setup_payslip_conn();
        seed_payslip_data(&conn);
        let html = payslip_html(&conn, "2026-09", 2).unwrap();
        assert!(html.contains("李四"));
        assert!(html.contains("¥ 8,000.00"));

        let err = payslip_html(&conn, "2026-09", 99).unwrap_err();
        assert!(err.to_string().contains("无工资结果"));
    }

    #[test]
    fn payslip_html_escapes_employee_name() {
        let conn = setup_payslip_conn();
        conn.execute(
            "INSERT INTO employees (id, employee_no, name, email) VALUES (1, 'E<1>', '张<b>三', 'z@example.com')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO salary_monthly_results (salary_month, employee_no, name, base_salary, locked)
             VALUES ('2026-09', 'E<1>', '张<b>三', 1000.0, 1)",
            [],
        )
        .unwrap();
        let html = payslip_html(&conn, "2026-09", 1).unwrap();
        assert!(html.contains("张&lt;b&gt;三"));
        assert!(html.contains("E&lt;1&gt;"));
        assert!(!html.contains("张<b>三"));
    }

    #[test]
    fn build_payslip_batch_rejects_unlocked_or_missing_month() {
        let conn = setup_payslip_conn();
        seed_payslip_data(&conn);
        let sec = ready_sec();
        ready_smtp(&conn, &sec);

        let err = build_payslip_batch(&conn, &sec, "2026-08", &[1], "管理员").unwrap_err();
        assert!(err.to_string().contains("尚未锁定"), "err: {err}");
        let err = build_payslip_batch(&conn, &sec, "2026-07", &[1], "管理员").unwrap_err();
        assert!(err.to_string().contains("尚未生成工资结果"), "err: {err}");

        // 空勾选拒绝
        let err = build_payslip_batch(&conn, &sec, "2026-09", &[], "管理员").unwrap_err();
        assert!(err.to_string().contains("勾选"), "err: {err}");
    }

    #[test]
    fn build_payslip_batch_rejects_missing_smtp() {
        let conn = setup_payslip_conn();
        seed_payslip_data(&conn);
        let sec = ready_sec(); // 敏感已解锁，但未配置 SMTP
        let err = build_payslip_batch(&conn, &sec, "2026-09", &[1], "管理员").unwrap_err();
        assert!(err.to_string().contains("SMTP"), "err: {err}");

        // 门禁拒绝时不得写发起留痕
        let logs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM operation_logs WHERE operation_type = 'payslip_email_batch_send'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(logs, 0);
    }

    #[test]
    fn build_payslip_batch_rejects_sensitive_not_revealed() {
        let conn = setup_payslip_conn();
        seed_payslip_data(&conn);
        let sec = sec_with_dek();
        ready_smtp(&conn, &sec);
        // 未 mark_sensitive_revealed
        let err = build_payslip_batch(&conn, &sec, "2026-09", &[1], "管理员").unwrap_err();
        assert!(err.to_string().contains("解锁敏感数据"), "err: {err}");
    }

    #[test]
    fn build_payslip_batch_skips_no_email_and_dedupes() {
        let conn = setup_payslip_conn();
        seed_payslip_data(&conn);
        let sec = ready_sec();
        ready_smtp(&conn, &sec);

        // 重复勾选 3 号（无邮箱）与 1 号：去重后仅张三 1 条消息，3 号进跳过名单
        let plan = build_payslip_batch(&conn, &sec, "2026-09", &[1, 3, 3, 1], "管理员").unwrap();
        assert_eq!(plan.items.len(), 1, "无邮箱者不生成消息，重复 id 去重");
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(plan.skipped[0].employee_id, 3);
        assert_eq!(plan.skipped[0].employee_name, "王五");

        let zhang = plan
            .items
            .iter()
            .find(|i| i.employee_id == Some(1))
            .expect("张三应入计划");
        assert_eq!(zhang.recipient, "zhangsan@example.com");
        assert_eq!(zhang.message.recipient, "zhangsan@example.com");
        assert_eq!(zhang.belong_month.as_deref(), Some("2026-09"));
        assert_eq!(zhang.message.subject, "2026-09 工资条");
        assert!(zhang
            .message
            .html_body
            .as_deref()
            .unwrap_or("")
            .contains("张三"));

        // 发起留痕：署名操作人，detail 仅含统计不含金额
        let (op_type, operator, detail): (String, String, String) = conn
            .query_row(
                "SELECT operation_type, operator, detail FROM operation_logs ORDER BY id DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(op_type, "payslip_email_batch_send");
        assert_eq!(operator, "管理员");
        assert!(detail.contains("month=2026-09"));
        assert!(detail.contains("no_email=1"));
        assert!(!detail.contains("10,000"));
    }

    #[test]
    fn send_payslip_batch_invalid_email_fails_without_blocking() {
        let conn = setup_payslip_conn();
        let sec = ready_sec();
        let channel = FakeChannel::new(vec![Ok(())]); // 仅合法地址走到通道
        let plan = PayslipBatchPlan {
            month: "2026-09".into(),
            items: vec![
                item(1, "bad-email", "2026-09", "2026-09 工资条"),
                item(2, "a@example.com", "2026-09", "2026-09 工资条"),
            ],
            skipped: Vec::new(),
        };
        let summary = send_payslip_batch(&conn, &sec, &channel, plan, "管理员").unwrap();

        assert_eq!(summary.sent, 1, "合法地址不受无效地址影响");
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.skipped, 0);
        assert_eq!(channel.sent_recipients(), vec!["a@example.com".to_string()]);

        let (status, error): (String, String) = conn
            .query_row(
                "SELECT status, error_msg FROM notification_logs WHERE recipient = 'bad-email'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, FAILED);
        assert_eq!(error, "邮箱地址无效");
        assert_eq!(
            summary.failed_log_ids.len(),
            1,
            "无效地址留 failed id 供重发"
        );
    }

    #[test]
    fn send_payslip_batch_rechecks_sensitive_state() {
        let conn = setup_payslip_conn();
        seed_payslip_data(&conn);
        let sec = ready_sec();
        ready_smtp(&conn, &sec);
        let plan = build_payslip_batch(&conn, &sec, "2026-09", &[1], "管理员").unwrap();

        // 构建与发送之间锁屏/到期 → 发送拒绝且不产生任何留痕
        sec.clear_sensitive_reveal();
        let channel = FakeChannel::new(vec![]);
        let err = send_payslip_batch(&conn, &sec, &channel, plan, "管理员").unwrap_err();
        assert!(err.to_string().contains("解锁敏感数据"), "err: {err}");
        assert!(channel.sent_recipients().is_empty());
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM notification_logs", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn payslip_build_and_send_flow_summary_with_belong_month() {
        let conn = setup_payslip_conn();
        seed_payslip_data(&conn);
        let sec = ready_sec();
        ready_smtp(&conn, &sec);

        let plan = build_payslip_batch(&conn, &sec, "2026-09", &[1, 2, 3], "管理员").unwrap();
        // 通道脚本：张三成功、李四失败
        let channel = FakeChannel::new(vec![Ok(()), Err(AppError::General("SMTP 超时".into()))]);
        let summary = send_payslip_batch(&conn, &sec, &channel, plan, "管理员").unwrap();

        assert_eq!(summary.sent, 1);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.skipped, 1, "无邮箱员工计入 skipped");
        assert_eq!(summary.failed_log_ids.len(), 1);

        // 留痕 belong_month 均为当月、主题为标题模板、署名操作人
        let rows: Vec<(Option<String>, String, Option<String>)> = conn
            .prepare("SELECT belong_month, subject, operator FROM notification_logs")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(rows.len(), 2, "无邮箱员工不留痕（构建期已跳过）");
        assert!(rows.iter().all(|(m, s, o)| m.as_deref() == Some("2026-09")
            && s == "2026-09 工资条"
            && o.as_deref() == Some("管理员")));
    }

    #[test]
    fn resend_payslip_batch_rebuilds_html_for_failed_only() {
        let conn = setup_payslip_conn();
        seed_payslip_data(&conn);
        let sec = ready_sec();
        ready_smtp(&conn, &sec);

        let plan = build_payslip_batch(&conn, &sec, "2026-09", &[1, 2], "管理员").unwrap();
        let channel = FakeChannel::new(vec![Ok(()), Err(AppError::General("SMTP 超时".into()))]);
        let first = send_payslip_batch(&conn, &sec, &channel, plan, "管理员").unwrap();
        assert_eq!(first.failed, 1);
        let sent_log_id = 1_i64; // 第一封（张三）成功留痕
        let failed_log_id = first.failed_log_ids[0];
        let total_before: i64 = conn
            .query_row("SELECT COUNT(*) FROM notification_logs", [], |r| r.get(0))
            .unwrap();

        // 重发：混入成功记录 id 与不存在 id → 仅 failed 者重建重发
        let resend_channel = FakeChannel::new(vec![Ok(())]);
        let summary = resend_payslip_batch(
            &conn,
            &sec,
            &resend_channel,
            "2026-09",
            &[failed_log_id, sent_log_id, 999],
            "出纳小王",
        )
        .unwrap();
        assert_eq!(summary.sent, 1);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.skipped, 2, "成功记录与不存在 id 计入 skipped");

        // 重建正文：FakeChannel 收到含员工姓名与金额的 html（留痕不存正文，须重建）
        let resent = resend_channel.sent.lock().unwrap();
        assert_eq!(resent.len(), 1);
        assert_eq!(resent[0].recipient, "lisi@example.com");
        assert_eq!(resent[0].subject, "2026-09 工资条");
        let html = resent[0].html_body.as_deref().expect("重发须重建正文");
        assert!(html.contains("李四"), "重建正文含员工姓名: {html}");
        assert!(html.contains("¥ 8,000.00"), "重建正文含金额: {html}");
        drop(resent);

        // 留痕只追加：原失败记录不动，新增 1 条新留痕且署名当次操作人
        let total_after: i64 = conn
            .query_row("SELECT COUNT(*) FROM notification_logs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total_after, total_before + 1);
        let (status, recipient, operator): (String, String, Option<String>) = conn
            .query_row(
                "SELECT status, recipient, operator FROM notification_logs WHERE id = ?1",
                params![total_after],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(status, SENT);
        assert_eq!(recipient, "lisi@example.com");
        assert_eq!(operator.as_deref(), Some("出纳小王"));

        // 重发留痕 operation_logs
        let op_type: String = conn
            .query_row(
                "SELECT operation_type FROM operation_logs ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(op_type, "payslip_email_resend");
    }

    #[test]
    fn resend_payslip_batch_skips_unresendable_logs() {
        let conn = setup_payslip_conn();
        seed_payslip_data(&conn);
        let sec = ready_sec();
        ready_smtp(&conn, &sec);

        // 构造不可重发留痕：月份不匹配 / employee_id 缺失 / 员工已无工资结果
        let other_month = insert_notification_log(
            &conn,
            "email",
            Some(1),
            "zhangsan@example.com",
            Some("2026-12"),
            "2026-12 工资条",
            FAILED,
            Some("超时"),
            Some("管理员"),
        )
        .unwrap();
        let no_employee = insert_notification_log(
            &conn,
            "email",
            None,
            "x@example.com",
            Some("2026-09"),
            "2026-09 工资条",
            FAILED,
            Some("超时"),
            Some("管理员"),
        )
        .unwrap();
        let no_result = insert_notification_log(
            &conn,
            "email",
            Some(99),
            "gone@example.com",
            Some("2026-09"),
            "2026-09 工资条",
            FAILED,
            Some("超时"),
            Some("管理员"),
        )
        .unwrap();

        let channel = FakeChannel::new(vec![]);
        let summary = resend_payslip_batch(
            &conn,
            &sec,
            &channel,
            "2026-09",
            &[other_month, no_employee, no_result],
            "管理员",
        )
        .unwrap();
        assert_eq!((summary.sent, summary.failed, summary.skipped), (0, 0, 3));
        assert!(channel.sent_recipients().is_empty());

        // 空勾选拒绝
        let err =
            resend_payslip_batch(&conn, &sec, &channel, "2026-09", &[], "管理员").unwrap_err();
        assert!(err.to_string().contains("勾选"), "err: {err}");

        // 月份未锁定/无结果同样拒绝
        let err = resend_payslip_batch(&conn, &sec, &channel, "2026-08", &[other_month], "管理员")
            .unwrap_err();
        assert!(err.to_string().contains("尚未锁定"), "err: {err}");
    }
}
