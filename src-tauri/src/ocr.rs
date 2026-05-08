use std::process::Command;

use rusqlite::Connection;
use serde_json::Value;

use crate::db::*;
use crate::errors::{AppError, AppResult};
use crate::models::*;

/// Run OCR recognition on an image via python3 script.
/// Looks for python-ocr/main.py relative to the app executable or in the project structure.
pub fn ocr_recognize(image_path: &str, month: &str, conn: &Connection) -> AppResult<OcrResult> {
    // Try to locate the python OCR script
    let script_path = find_ocr_script()?;

    let output = Command::new("python3")
        .arg(&script_path)
        .arg("--image")
        .arg(image_path)
        .arg("--mode")
        .arg("attendance")
        .output()
        .map_err(|e| AppError::Ocr(format!("无法执行python3: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::Ocr(format!("OCR执行失败: {stderr}")));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let raw_text = stdout.trim().to_string();

    // Parse the JSON result from OCR script
    let records = parse_ocr_output(&raw_text)?;

    // Save OCR batch
    let parsed_json = serde_json::to_string(&records).unwrap_or_default();
    let batch = OcrBatch {
        id: 0,
        batch_name: Some(format!("OCR-{}", chrono::Utc::now().format("%Y%m%d%H%M%S"))),
        salary_month: Some(month.to_string()),
        image_path: Some(image_path.to_string()),
        raw_text: Some(raw_text.clone()),
        parsed_json: Some(parsed_json),
        status: "pending".to_string(),
        created_at: None,
    };

    let batch_id = save_ocr_batch(conn, &batch)?;

    let mut ocr_records = Vec::new();
    for mut r in records {
        r.salary_month = month.to_string();
        r.source_type = Some("ocr".to_string());
        r.ocr_batch_id = Some(batch_id);
        ocr_records.push(r);
    }

    Ok(OcrResult {
        batch_id,
        records: ocr_records,
        raw_text: Some(raw_text),
    })
}

fn find_ocr_script() -> AppResult<String> {
    // Try multiple possible locations
    let candidates = vec![
        "python-ocr/main.py",
        "../python-ocr/main.py",
        "../../python-ocr/main.py",
        "/opt/salary-ocr/main.py",
    ];

    for candidate in &candidates {
        if std::path::Path::new(candidate).exists() {
            return Ok(candidate.to_string());
        }
    }

    // Fall back to first candidate - will fail with a clear error if not found
    Ok("python-ocr/main.py".to_string())
}

fn parse_ocr_output(raw: &str) -> AppResult<Vec<AttendanceRecordInput>> {
    // Try to parse as JSON first
    if let Ok(json) = serde_json::from_str::<Value>(raw) {
        if let Some(arr) = json.as_array() {
            let mut records = Vec::new();
            for item in arr {
                let record = AttendanceRecordInput {
                    id: None,
                    salary_month: String::new(),
                    employee_no: item["employee_no"].as_str().unwrap_or("").to_string(),
                    name: item["name"].as_str().map(|s| s.to_string()),
                    expected_days: item["expected_days"].as_f64(),
                    actual_days: item["actual_days"].as_f64(),
                    late_count: item["late_count"].as_i64().map(|v| v as i32),
                    early_leave_count: item["early_leave_count"].as_i64().map(|v| v as i32),
                    personal_leave_days: item["personal_leave_days"].as_f64(),
                    sick_leave_days: item["sick_leave_days"].as_f64(),
                    absent_days: item["absent_days"].as_f64(),
                    overtime_hours: item["overtime_hours"].as_f64(),
                    source_type: None,
                    ocr_batch_id: None,
                    remark: item["remark"].as_str().map(|s| s.to_string()),
                };
                records.push(record);
            }
            return Ok(records);
        }
    }

    // If not JSON, return empty with error
    Err(AppError::Ocr("OCR输出格式无法解析".to_string()))
}

/// Confirm OCR results and save attendance records
pub fn confirm_ocr_results(
    batch_id: i64,
    records: &[AttendanceRecordInput],
    conn: &Connection,
) -> AppResult<bool> {
    for record in records {
        upsert_attendance_record(conn, record)?;
    }

    update_ocr_batch_status(conn, batch_id, "confirmed")?;

    log_operation(
        conn,
        "ocr_confirm",
        &format!("确认OCR批次{batch_id}，共{}条记录", records.len()),
        "system",
        None,
    )?;

    Ok(true)
}
