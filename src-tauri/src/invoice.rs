use rusqlite::Connection;
use serde::Deserialize;
use std::sync::Mutex;

use crate::db;
use crate::errors::{AppError, AppResult};
use crate::models::*;
use crate::ocr;

const BAIDU_VAT_INVOICE_URL: &str = "https://aip.baidubce.com/rest/2.0/ocr/v1/vat_invoice";

// ==================== Baidu Response Types ====================

#[derive(Debug, Deserialize)]
pub(crate) struct BaiduVatInvoiceResponse {
    #[serde(default)]
    words_result: serde_json::Value,
    error_code: Option<i32>,
    error_msg: Option<String>,
    // vat_invoice 顶层可能有 InvoiceTypeLog/TotalAmount/etc.
    // 我们从 words_result 与顶层字段双路取值
    #[serde(flatten)]
    extra: serde_json::Value,
}

pub trait InvoiceOcrDbOps {
    fn get_baidu_access_token(&self) -> AppResult<String>;
    fn clear_baidu_access_token(&self) -> AppResult<()>;
    fn find_invoice_by_dedup_key(
        &self,
        code: Option<&str>,
        number: &str,
    ) -> AppResult<Option<Invoice>>;
}

impl InvoiceOcrDbOps for Connection {
    fn get_baidu_access_token(&self) -> AppResult<String> {
        ocr::get_baidu_access_token(self)
    }

    fn clear_baidu_access_token(&self) -> AppResult<()> {
        db::set_setting(self, "baidu_access_token", "")?;
        db::set_setting(self, "baidu_token_expires_at", "")?;
        Ok(())
    }

    fn find_invoice_by_dedup_key(
        &self,
        code: Option<&str>,
        number: &str,
    ) -> AppResult<Option<Invoice>> {
        db::find_invoice_by_dedup_key(self, code, number)
    }
}

impl InvoiceOcrDbOps for Mutex<Connection> {
    fn get_baidu_access_token(&self) -> AppResult<String> {
        let conn = self.lock().map_err(|e| AppError::General(e.to_string()))?;
        ocr::get_baidu_access_token(&conn)
    }

    fn clear_baidu_access_token(&self) -> AppResult<()> {
        let conn = self.lock().map_err(|e| AppError::General(e.to_string()))?;
        db::set_setting(&conn, "baidu_access_token", "")?;
        db::set_setting(&conn, "baidu_token_expires_at", "")?;
        Ok(())
    }

    fn find_invoice_by_dedup_key(
        &self,
        code: Option<&str>,
        number: &str,
    ) -> AppResult<Option<Invoice>> {
        let conn = self.lock().map_err(|e| AppError::General(e.to_string()))?;
        db::find_invoice_by_dedup_key(&conn, code, number)
    }
}

enum InvoiceOcrInnerError {
    TokenInvalid(AppError),
    Other(AppError),
}

impl From<AppError> for InvoiceOcrInnerError {
    fn from(error: AppError) -> Self {
        Self::Other(error)
    }
}

// ==================== OCR Entry ====================

/// 识别发票。PDF 直接走百度的 `pdf_file` 参数（不依赖系统 poppler），
/// 图片走 `image` 参数。token 失效（错误码 110）自动清缓存重试一次。
pub fn ocr_invoice<D: InvoiceOcrDbOps + ?Sized>(
    image_path: &str,
    db_ops: &D,
) -> AppResult<InvoiceOcrPreview> {
    match ocr_invoice_inner(image_path, db_ops) {
        Ok(preview) => Ok(preview),
        Err(InvoiceOcrInnerError::TokenInvalid(e)) => {
            // 110 = Access token invalid or no longer valid
            // 通常发生在用户重置 secret 后旧 token 还在 DB 缓存里
            db_ops.clear_baidu_access_token()?;
            ocr_invoice_inner(image_path, db_ops)
                .map_err(|retry_error| retry_error.into_app_error())
        }
        Err(InvoiceOcrInnerError::Other(e)) => Err(e),
    }
}

impl InvoiceOcrInnerError {
    fn into_app_error(self) -> AppError {
        match self {
            Self::TokenInvalid(error) | Self::Other(error) => error,
        }
    }
}

fn ocr_invoice_inner<D: InvoiceOcrDbOps + ?Sized>(
    image_path: &str,
    db_ops: &D,
) -> Result<InvoiceOcrPreview, InvoiceOcrInnerError> {
    let file_data = std::fs::read(image_path).map_err(AppError::Io)?;
    let file_b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &file_data);

    let token = db_ops.get_baidu_access_token()?;
    let url = format!("{BAIDU_VAT_INVOICE_URL}?access_token={token}");

    // PDF 用 pdf_file + pdf_file_num=1；图片用 image
    let is_pdf = image_path.to_lowercase().ends_with(".pdf");
    let form_data: Vec<(&str, &str)> = if is_pdf {
        vec![("pdf_file", file_b64.as_str()), ("pdf_file_num", "1")]
    } else {
        vec![("image", file_b64.as_str())]
    };

    let client = reqwest::blocking::Client::new();
    let response = client
        .post(&url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&form_data)
        .send()
        .map_err(|e| AppError::Network(format!("百度发票OCR请求失败: {e}")))?;

    let raw_text = response
        .text()
        .map_err(|e| AppError::Network(format!("读取响应失败: {e}")))?;

    let parsed: BaiduVatInvoiceResponse = serde_json::from_str(&raw_text)
        .map_err(|e| AppError::Ocr(format!("百度发票OCR响应解析失败: {e}")))?;

    if let Some(code) = parsed.error_code {
        let is_token_invalid = is_baidu_token_invalid(&parsed);
        let msg = parsed.error_msg.unwrap_or_default();
        let error = AppError::Ocr(translate_baidu_error(code, &msg));
        return if is_token_invalid {
            Err(InvoiceOcrInnerError::TokenInvalid(error))
        } else {
            Err(InvoiceOcrInnerError::Other(error))
        };
    }

    let mut preview = map_baidu_response(&parsed, &raw_text);

    // 查重（code 可空，支持全电票）
    if let Some(n) = preview.invoice_number.as_ref() {
        let code_ref = preview.invoice_code.as_deref();
        if let Some(existing) = db_ops.find_invoice_by_dedup_key(code_ref, n)? {
            preview.is_duplicate = true;
            preview.duplicate_invoice_id = Some(existing.id);
            preview.warnings.push(format!(
                "该发票已存在于系统（ID={}，录入时间={}）",
                existing.id,
                existing.created_at.unwrap_or_default()
            ));
        }
    } else {
        preview
            .warnings
            .push("未能识别发票号码，需手工补全".to_string());
    }

    Ok(preview)
}

// ==================== Pure Mapping Functions ====================

fn translate_baidu_error(code: i32, msg: &str) -> String {
    match code {
        18 => "百度OCR QPS超限，请稍后再试".to_string(),
        216201 => "图片不存在或格式错误".to_string(),
        216202 => "图片模糊，无法识别".to_string(),
        216678 => "发票类型不支持或图片非发票".to_string(),
        _ => format!("百度OCR错误({code}): {msg}"),
    }
}

fn is_baidu_token_invalid(resp: &BaiduVatInvoiceResponse) -> bool {
    resp.error_code == Some(110)
}

fn map_baidu_response(resp: &BaiduVatInvoiceResponse, raw_text: &str) -> InvoiceOcrPreview {
    let words = &resp.words_result;
    let extra = &resp.extra;

    InvoiceOcrPreview {
        invoice_code: pick_str(words, extra, "InvoiceCode"),
        invoice_number: pick_str(words, extra, "InvoiceNum"),
        invoice_type: pick_str(words, extra, "InvoiceType")
            .or_else(|| pick_str(words, extra, "InvoiceTypeLog")),
        issue_date: pick_str(words, extra, "IssueDate"),
        check_code: pick_str(words, extra, "CheckCode"),
        amount: parse_amount(&pick_str(words, extra, "TotalAmount")).unwrap_or(0.0),
        tax_amount: parse_amount(&pick_str(words, extra, "TotalTax")).unwrap_or(0.0),
        total_amount: parse_amount(&pick_str(words, extra, "AmountInFiguers")).unwrap_or(0.0),
        seller_name: pick_str(words, extra, "SellerName"),
        seller_tax_id: pick_str(words, extra, "SellerRegisterNum"),
        buyer_name: pick_str(words, extra, "PurchaserName"),
        buyer_tax_id: pick_str(words, extra, "PurchaserRegisterNum"),
        raw_ocr_json: raw_text.to_string(),
        warnings: Vec::new(),
        is_duplicate: false,
        duplicate_invoice_id: None,
    }
}

/// 从 words_result（对象，每个字段是 {word: "..."}）或 extra（顶层字段，直接字符串）取值
fn pick_str(words: &serde_json::Value, extra: &serde_json::Value, key: &str) -> Option<String> {
    if let Some(obj) = words.get(key).and_then(|v| v.as_object()) {
        if let Some(w) = obj.get("word").and_then(|v| v.as_str()) {
            return Some(w.trim().to_string()).filter(|s| !s.is_empty());
        }
    }
    extra
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 解析金额：去千分位逗号、去 ¥/￥/$ 符号、去「元」、解析为 f64
fn parse_amount(s: &Option<String>) -> Option<f64> {
    let s = s.as_ref()?;
    let cleaned: String = s
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    cleaned.parse::<f64>().ok()
}

// ==================== Business Layer ====================

pub fn save_invoice(
    input: &InvoiceInput,
    conn: &Connection,
    app_data_dir: &std::path::Path,
) -> AppResult<Invoice> {
    // 二次查重（发票号码必填；发票代码可空，支持全电票）
    let number = input.invoice_number.as_deref().unwrap_or("").trim();
    if number.is_empty() {
        return Err(AppError::InvalidParam("发票号码必填".into()));
    }
    let code_ref = input.invoice_code.as_deref();
    if let Some(existing) = db::find_invoice_by_dedup_key(conn, code_ref, number)? {
        let code_disp = code_ref.unwrap_or("");
        return Err(AppError::General(format!(
            "发票已存在：代码{code_disp} 号码{number}，记录ID={}",
            existing.id
        )));
    }

    // 复制原图到应用目录
    let target_path = match input.image_path.as_deref() {
        Some(src) if !src.is_empty() => Some(copy_image_to_app_dir(
            src,
            input.belong_month.as_deref(),
            app_data_dir,
        )?),
        _ => None,
    };

    let invoice = db::insert_invoice(conn, input, target_path.as_deref().unwrap_or(""))?;

    db::log_operation(
        conn,
        "save_invoice",
        &format!(
            "录入发票：代码{} 号码{} 价税合计{:.2}",
            input.invoice_code.as_deref().unwrap_or(""),
            input.invoice_number.as_deref().unwrap_or(""),
            input.total_amount.unwrap_or(0.0)
        ),
        "system",
        None,
    )?;

    Ok(invoice)
}

pub fn save_invoice_with_mutex(
    input: &InvoiceInput,
    state: &Mutex<Connection>,
    app_data_dir: &std::path::Path,
) -> AppResult<Invoice> {
    let number = input.invoice_number.as_deref().unwrap_or("").trim();
    if number.is_empty() {
        return Err(AppError::InvalidParam("发票号码必填".into()));
    }
    let code_ref = input.invoice_code.as_deref();
    {
        let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
        if let Some(existing) = db::find_invoice_by_dedup_key(&conn, code_ref, number)? {
            let code_disp = code_ref.unwrap_or("");
            return Err(AppError::General(format!(
                "发票已存在：代码{code_disp} 号码{number}，记录ID={}",
                existing.id
            )));
        }
    }

    let target_path = match input.image_path.as_deref() {
        Some(src) if !src.is_empty() => Some(copy_image_to_app_dir(
            src,
            input.belong_month.as_deref(),
            app_data_dir,
        )?),
        _ => None,
    };

    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let invoice = db::insert_invoice(&conn, input, target_path.as_deref().unwrap_or(""))?;

    db::log_operation(
        &conn,
        "save_invoice",
        &format!(
            "录入发票：代码{} 号码{} 价税合计{:.2}",
            input.invoice_code.as_deref().unwrap_or(""),
            input.invoice_number.as_deref().unwrap_or(""),
            input.total_amount.unwrap_or(0.0)
        ),
        "system",
        None,
    )?;

    Ok(invoice)
}

pub fn update_invoice(
    id: i64,
    input: &InvoiceInput,
    conn: &Connection,
    app_data_dir: &std::path::Path,
) -> AppResult<bool> {
    let existing = db::get_invoice(conn, id)?;
    let new_image_path = if let Some(new_src) = input.image_path.as_deref() {
        if !new_src.is_empty() && new_src != existing.image_path.as_deref().unwrap_or("") {
            // 用户换图，复制新图
            let copied = copy_image_to_app_dir(
                new_src,
                input
                    .belong_month
                    .as_deref()
                    .or(existing.belong_month.as_deref()),
                app_data_dir,
            )?;
            Some(copied)
        } else {
            None
        }
    } else {
        None
    };

    let result = db::update_invoice(conn, id, input, new_image_path.as_deref())?;

    if result {
        db::log_operation(
            conn,
            "update_invoice",
            &format!("更新发票ID={id}"),
            "system",
            None,
        )?;
    }

    Ok(result)
}

pub fn update_invoice_with_mutex(
    id: i64,
    input: &InvoiceInput,
    state: &Mutex<Connection>,
    app_data_dir: &std::path::Path,
) -> AppResult<bool> {
    let existing = {
        let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
        db::get_invoice(&conn, id)?
    };
    let new_image_path = if let Some(new_src) = input.image_path.as_deref() {
        if !new_src.is_empty() && new_src != existing.image_path.as_deref().unwrap_or("") {
            Some(copy_image_to_app_dir(
                new_src,
                input
                    .belong_month
                    .as_deref()
                    .or(existing.belong_month.as_deref()),
                app_data_dir,
            )?)
        } else {
            None
        }
    } else {
        None
    };

    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let result = db::update_invoice(&conn, id, input, new_image_path.as_deref())?;

    if result {
        db::log_operation(
            &conn,
            "update_invoice",
            &format!("更新发票ID={id}"),
            "system",
            None,
        )?;
    }

    Ok(result)
}

pub fn delete_invoice(id: i64, conn: &Connection) -> AppResult<bool> {
    let result = db::soft_delete_invoice(conn, id)?;
    if result {
        db::log_operation(
            conn,
            "delete_invoice",
            &format!("删除发票ID={id}"),
            "system",
            None,
        )?;
    }
    Ok(result)
}

/// 复制源文件到 {app_data_dir}/invoices/{belong_month}/{timestamp}_{filename}
pub(crate) fn copy_image_to_app_dir(
    src: &str,
    belong_month: Option<&str>,
    app_data_dir: &std::path::Path,
) -> AppResult<String> {
    let raw_month = belong_month.unwrap_or("unclassified");
    // Sanitize: reject path separators and parent traversal
    let sanitized: String = raw_month
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    let month = if sanitized.is_empty() {
        "unclassified"
    } else {
        sanitized.as_str()
    };

    let src_path = std::path::Path::new(src);
    let raw_filename = src_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "invoice.bin".to_string());
    let sanitized_filename = sanitize_invoice_filename(&raw_filename);
    let filename = if sanitized_filename.is_empty() {
        "invoice.bin"
    } else {
        sanitized_filename.as_str()
    };

    let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S");
    let target_name = format!("{timestamp}_{filename}");

    let target_dir = app_data_dir.join("invoices").join(month);
    std::fs::create_dir_all(&target_dir)?;

    let target_path = target_dir.join(target_name);
    std::fs::copy(src_path, &target_path)?;

    Ok(target_path.to_string_lossy().to_string())
}

fn sanitize_invoice_filename(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(*c, '.' | '-' | '_') || is_han_char(*c))
        .collect()
}

fn is_han_char(c: char) -> bool {
    ('\u{3400}'..='\u{4DBF}').contains(&c)
        || ('\u{4E00}'..='\u{9FFF}').contains(&c)
        || ('\u{F900}'..='\u{FAFF}').contains(&c)
}

// ==================== Unit Tests ====================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_response(json_value: serde_json::Value) -> BaiduVatInvoiceResponse {
        // 拆出 words_result 和 extra
        let words_result = json_value.get("words_result").cloned().unwrap_or(json!({}));
        let extra = {
            let mut e = json_value.clone();
            if e.get("words_result").is_some() {
                e.as_object_mut().map(|o| o.remove("words_result"));
            }
            if e.get("error_code").is_some() {
                e.as_object_mut().map(|o| o.remove("error_code"));
            }
            if e.get("error_msg").is_some() {
                e.as_object_mut().map(|o| o.remove("error_msg"));
            }
            e
        };
        BaiduVatInvoiceResponse {
            words_result,
            error_code: json_value
                .get("error_code")
                .and_then(|v| v.as_i64())
                .map(|v| v as i32),
            error_msg: json_value
                .get("error_msg")
                .and_then(|v| v.as_str())
                .map(String::from),
            extra,
        }
    }

    #[test]
    fn test_map_full_response() {
        let resp_json = json!({
            "words_result": {
                "InvoiceCode": {"word": "044001800211"},
                "InvoiceNum": {"word": "12345678"},
                "InvoiceType": {"word": "增值税普通发票"},
                "IssueDate": {"word": "2026-08-01"},
                "TotalAmount": {"word": "100.00"},
                "TotalTax": {"word": "6.00"},
                "AmountInFiguers": {"word": "￥106.00"},
                "SellerName": {"word": "测试销售方"},
                "SellerRegisterNum": {"word": "91XXXX"},
                "PurchaserName": {"word": "测试购买方"},
                "PurchaserRegisterNum": {"word": "92XXXX"},
            }
        });
        let resp = make_response(resp_json);
        let preview = map_baidu_response(&resp, "");
        assert_eq!(preview.invoice_code.as_deref(), Some("044001800211"));
        assert_eq!(preview.invoice_number.as_deref(), Some("12345678"));
        assert_eq!(preview.invoice_type.as_deref(), Some("增值税普通发票"));
        assert!((preview.amount - 100.0).abs() < 1e-6);
        assert!((preview.tax_amount - 6.0).abs() < 1e-6);
        assert!((preview.total_amount - 106.0).abs() < 1e-6);
    }

    #[test]
    fn test_map_partial_response() {
        let resp_json = json!({
            "words_result": {
                "InvoiceCode": {"word": "044001800211"},
                "InvoiceNum": {"word": "12345678"},
            }
        });
        let resp = make_response(resp_json);
        let preview = map_baidu_response(&resp, "");
        assert!(preview.invoice_type.is_none());
        assert!((preview.amount - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_parse_amount() {
        assert_eq!(parse_amount(&Some("100.00".into())), Some(100.0));
        assert_eq!(parse_amount(&Some("￥1,234.56".into())), Some(1234.56));
        assert_eq!(parse_amount(&Some("1,234.56元".into())), Some(1234.56));
        assert_eq!(parse_amount(&Some("".into())), None);
        assert_eq!(parse_amount(&Some("abc".into())), None);
        assert_eq!(parse_amount(&None), None);
    }

    #[test]
    fn test_translate_baidu_error() {
        assert!(translate_baidu_error(18, "").contains("QPS"));
        assert!(translate_baidu_error(216201, "").contains("图片"));
        assert!(translate_baidu_error(999, "raw msg").contains("raw msg"));
    }

    #[test]
    fn test_baidu_token_invalid_uses_error_code() {
        let token_invalid = make_response(json!({
            "error_code": 110,
            "error_msg": "anything"
        }));
        assert!(is_baidu_token_invalid(&token_invalid));

        let same_message_different_code = make_response(json!({
            "error_code": 111,
            "error_msg": "Access token invalid"
        }));
        assert!(!is_baidu_token_invalid(&same_message_different_code));
    }
}

#[cfg(test)]
mod business_tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("
            CREATE TABLE employees (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT);
            CREATE TABLE invoice_expense_types (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                code TEXT UNIQUE NOT NULL,
                name TEXT NOT NULL,
                sort_order INTEGER DEFAULT 0,
                enabled INTEGER DEFAULT 1,
                remark TEXT
            );
            CREATE TABLE invoices (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                invoice_code TEXT, invoice_number TEXT, invoice_type TEXT,
                issue_date TEXT, check_code TEXT,
                amount REAL DEFAULT 0, tax_amount REAL DEFAULT 0, total_amount REAL DEFAULT 0,
                seller_name TEXT, seller_tax_id TEXT, buyer_name TEXT, buyer_tax_id TEXT,
                expense_type_code TEXT, employee_id INTEGER, belong_month TEXT,
                status TEXT DEFAULT 'normal', remark TEXT,
                image_path TEXT, raw_ocr_json TEXT,
                created_at TEXT, updated_at TEXT
            );
            CREATE UNIQUE INDEX idx_invoices_code_number ON invoices(COALESCE(invoice_code, ''), invoice_number) WHERE status != 'void';
            CREATE TABLE operation_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                operation_type TEXT NOT NULL, description TEXT,
                operator TEXT, detail TEXT, created_at TEXT
            );
            INSERT INTO invoice_expense_types (code, name, sort_order) VALUES ('office', '办公费', 1);
            INSERT INTO employees (id, name) VALUES (1, '张三');
        ").unwrap();
        conn
    }

    fn sample_input() -> InvoiceInput {
        InvoiceInput {
            invoice_code: Some("12345".into()),
            invoice_number: Some("67890".into()),
            invoice_type: Some("普通发票".into()),
            issue_date: Some("2026-08-01".into()),
            check_code: None,
            amount: Some(100.0),
            tax_amount: Some(6.0),
            total_amount: Some(106.0),
            seller_name: Some("销售方".into()),
            seller_tax_id: Some("91X".into()),
            buyer_name: Some("购买方".into()),
            buyer_tax_id: Some("92X".into()),
            expense_type_code: Some("office".into()),
            employee_id: Some(1),
            belong_month: Some("2026-08".into()),
            remark: None,
            image_path: None,
            raw_ocr_json: Some("{}".into()),
        }
    }

    #[test]
    fn test_save_invoice_blocks_duplicate() {
        let conn = setup_db();
        let tmp = std::env::temp_dir();
        let input = sample_input();
        save_invoice(&input, &conn, &tmp).unwrap();
        let result = save_invoice(&input, &conn, &tmp);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("发票已存在"));
    }

    #[test]
    fn test_save_invoice_requires_number_only() {
        // 全电票无 code 也应能保存（只检查 number 必填）
        let conn = setup_db();
        let mut input = sample_input();
        input.invoice_code = None;
        let result = save_invoice(&input, &conn, &std::env::temp_dir());
        assert!(
            result.is_ok(),
            "missing code should be allowed for full-electronic invoices"
        );

        // 但 number 缺失要被拒绝
        let mut no_number = sample_input();
        no_number.invoice_number = None;
        let result = save_invoice(&no_number, &conn, &std::env::temp_dir());
        let err = result.unwrap_err();
        assert!(
            matches!(err, AppError::InvalidParam(_)),
            "expected InvalidParam for missing number, got {:?}",
            err
        );
    }

    #[test]
    fn test_save_invoice_blocks_duplicate_full_electronic() {
        // 两条全电票（无 code）同号应被拦截
        let conn = setup_db();
        let tmp = std::env::temp_dir();
        let mut a = sample_input();
        a.invoice_code = None;
        a.invoice_number = Some("FULL001".into());
        let mut b = sample_input();
        b.invoice_code = None;
        b.invoice_number = Some("FULL001".into());
        save_invoice(&a, &conn, &tmp).unwrap();
        let result = save_invoice(&b, &conn, &tmp);
        assert!(
            result.is_err(),
            "duplicate full-electronic (no code) should be blocked"
        );
        assert!(result.unwrap_err().to_string().contains("发票已存在"));
    }

    #[test]
    fn test_save_invoice_normalized_code_number_collide() {
        let conn = setup_db();
        let tmp = std::env::temp_dir();
        let mut a = sample_input();
        a.invoice_code = Some(" ABC123 ".into());
        a.invoice_number = Some(" 12345678 ".into());
        let mut b = sample_input();
        b.invoice_code = Some("abc123".into());
        b.invoice_number = Some("12345678".into());

        save_invoice(&a, &conn, &tmp).unwrap();
        let result = save_invoice(&b, &conn, &tmp);
        assert!(
            result.is_err(),
            "trimmed/lowercased dedup keys should collide"
        );
    }

    #[test]
    fn test_update_invoice_blocks_cross_record_collision() {
        let conn = setup_db();
        let tmp = std::env::temp_dir();
        // Insert two invoices with different codes
        let mut a = sample_input();
        a.invoice_code = Some("AAA".into());
        a.invoice_number = Some("001".into());
        let mut b = sample_input();
        b.invoice_code = Some("BBB".into());
        b.invoice_number = Some("002".into());
        let inv_a = save_invoice(&a, &conn, &tmp).unwrap();
        let _inv_b = save_invoice(&b, &conn, &tmp).unwrap();

        // Try to update A to use B's code+number — should fail
        let mut collision_input = a.clone();
        collision_input.invoice_code = Some("BBB".into());
        collision_input.invoice_number = Some("002".into());
        let result = update_invoice(inv_a.id, &collision_input, &conn, &tmp);
        assert!(
            result.is_err(),
            "updating to collide with another record should fail"
        );
    }

    #[test]
    fn test_save_invoice_logs_operation() {
        let conn = setup_db();
        save_invoice(&sample_input(), &conn, &std::env::temp_dir()).unwrap();
        let log_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM operation_logs WHERE operation_type = 'save_invoice'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(log_count, 1);
    }

    #[test]
    fn test_copy_image_to_app_dir() {
        let tmp = std::env::temp_dir();
        let src = tmp.join("test_invoice_src.txt");
        std::fs::write(&src, b"hello").unwrap();
        let dest = copy_image_to_app_dir(
            src.to_str().unwrap(),
            Some("2026-08"),
            &tmp.join("app_data"),
        )
        .unwrap();
        assert!(dest.contains("invoices/2026-08/"));
        let content = std::fs::read_to_string(&dest).unwrap();
        assert_eq!(content, "hello");
    }

    #[test]
    fn test_copy_image_to_app_dir_unclassified_month() {
        let tmp = std::env::temp_dir();
        let src = tmp.join("test_invoice_src2.txt");
        std::fs::write(&src, b"hello").unwrap();
        let dest =
            copy_image_to_app_dir(src.to_str().unwrap(), None, &tmp.join("app_data2")).unwrap();
        assert!(dest.contains("invoices/unclassified/"));
    }

    #[test]
    fn test_copy_image_to_app_dir_sanitizes_filename() {
        let tmp = std::env::temp_dir();
        let src = tmp.join("测/..\\bad\nname?.PDF".replace('/', ""));
        std::fs::write(&src, b"hello").unwrap();
        let dest = copy_image_to_app_dir(
            src.to_str().unwrap(),
            Some("2026-08"),
            &tmp.join("app_data3"),
        )
        .unwrap();
        let filename = std::path::Path::new(&dest)
            .file_name()
            .unwrap()
            .to_string_lossy();
        assert!(filename.contains("测"));
        assert!(filename.ends_with("_测..badname.PDF"));
        assert!(!filename.contains('\\'));
        assert!(!filename.contains('\n'));
        assert!(!filename.contains('?'));
    }
}
