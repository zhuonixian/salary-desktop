use rusqlite::Connection;
use serde::Deserialize;

use crate::db;
use crate::errors::{AppError, AppResult};
use crate::models::*;
use crate::ocr;

const BAIDU_VAT_INVOICE_URL: &str =
    "https://aip.baidubce.com/rest/2.0/ocr/v1/vat_invoice";

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

// ==================== OCR Entry ====================

pub fn ocr_invoice(image_path: &str, conn: &Connection) -> AppResult<InvoiceOcrPreview> {
    let image_data = std::fs::read(image_path)
        .map_err(|e| AppError::Io(e))?;
    let image_b64 = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD, &image_data
    );

    let token = ocr::get_baidu_access_token(conn)?;
    let url = format!("{BAIDU_VAT_INVOICE_URL}?access_token={token}");

    let client = reqwest::blocking::Client::new();
    let response = client
        .post(&url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&[("image", image_b64.as_str())])
        .send()
        .map_err(|e| AppError::Network(format!("百度发票OCR请求失败: {e}")))?;

    let raw_text = response.text()
        .map_err(|e| AppError::Network(format!("读取响应失败: {e}")))?;

    let parsed: BaiduVatInvoiceResponse = serde_json::from_str(&raw_text)
        .map_err(|e| AppError::Ocr(format!("百度发票OCR响应解析失败: {e}")))?;

    if let Some(code) = parsed.error_code {
        let msg = parsed.error_msg.unwrap_or_default();
        return Err(AppError::Ocr(translate_baidu_error(code, &msg)));
    }

    let mut preview = map_baidu_response(&parsed, &raw_text);

    // 查重
    if let (Some(c), Some(n)) = (preview.invoice_code.as_ref(), preview.invoice_number.as_ref()) {
        if let Some(existing) = db::find_invoice_by_code_number(conn, c, n)? {
            preview.is_duplicate = true;
            preview.duplicate_invoice_id = Some(existing.id);
            preview.warnings.push(format!(
                "该发票已存在于系统（ID={}，录入时间={}）",
                existing.id,
                existing.created_at.unwrap_or_default()
            ));
        }
    } else {
        preview.warnings.push("未能识别发票代码或号码，需手工补全".to_string());
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
        amount: parse_amount(&pick_str(words, extra, "TotalAmount"))
            .unwrap_or(0.0),
        tax_amount: parse_amount(&pick_str(words, extra, "TotalTax"))
            .unwrap_or(0.0),
        total_amount: parse_amount(&pick_str(words, extra, "AmountInFiguers"))
            .unwrap_or(0.0),
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
    extra.get(key).and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 解析金额：去千分位逗号、去 ¥/￥/$ 符号、去「元」、解析为 f64
fn parse_amount(s: &Option<String>) -> Option<f64> {
    let s = s.as_ref()?;
    let cleaned: String = s.chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    cleaned.parse::<f64>().ok()
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
            error_code: json_value.get("error_code").and_then(|v| v.as_i64()).map(|v| v as i32),
            error_msg: json_value.get("error_msg").and_then(|v| v.as_str()).map(String::from),
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
}
