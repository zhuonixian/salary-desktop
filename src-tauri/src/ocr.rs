use std::path::PathBuf;
use std::process::{Command, Output};

use rusqlite::Connection;
use serde_json::Value;

use crate::db::*;
use crate::errors::{AppError, AppResult};
use crate::models::*;

/// Decode bytes from Python output: try UTF-8 first, fallback to GBK (Windows Chinese)
fn decode_bytes(data: &[u8]) -> String {
    // Try UTF-8 first (works when PYTHONUTF8=1 is set)
    if let Ok(s) = String::from_utf8(data.to_vec()) {
        return s;
    }
    // Fallback: try GBK/GB2312 for Chinese Windows
    let (decoded, _, _) = encoding_rs::GBK.decode(data);
    let decoded = decoded.to_string();
    if !decoded.contains('\u{fffd}') {
        return decoded;
    }
    // Last resort: lossy UTF-8
    String::from_utf8_lossy(data).to_string()
}

/// Run OCR recognition on an image via python3 script.
/// resource_dir: Tauri resource directory (from app.path().resource_dir()), or None for dev mode.
pub fn ocr_recognize(image_path: &str, month: &str, conn: &Connection, resource_dir: Option<&std::path::Path>) -> AppResult<OcrResult> {
    // Try to locate the python OCR script
    let script_path = find_ocr_script(resource_dir)?;

    let (python_cmd, output) = run_ocr_script(&script_path, image_path)?;

    if !output.status.success() {
        let stderr = decode_bytes(&output.stderr);
        let stdout = decode_bytes(&output.stdout);
        let detail = extract_ocr_error(&stdout)
            .or_else(|| extract_ocr_error(&stderr))
            .unwrap_or_else(|| {
                let combined = format!("stdout: {}; stderr: {}", stdout.trim(), stderr.trim());
                combined.trim().to_string()
            });
        return Err(AppError::Ocr(format!(
            "OCR执行失败: {detail} (python={python_cmd}, script={script_path})"
        )));
    }

    let stdout = decode_bytes(&output.stdout);
    let output_text = stdout.trim().to_string();

    // Parse the JSON result from OCR script
    let (records, raw_text) = parse_ocr_output(&output_text)?;

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

fn find_ocr_script(resource_dir: Option<&std::path::Path>) -> AppResult<String> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    // 1. Tauri resource directory (production: resources are bundled here)
    if let Some(rdir) = resource_dir {
        candidates.push(rdir.join("python-ocr/main.py"));
        // Also try flat structure
        candidates.push(rdir.join("main.py"));
    }

    // 2. Dev mode: relative to project root / cwd
    candidates.push(PathBuf::from("python-ocr/main.py"));
    candidates.push(PathBuf::from("../python-ocr/main.py"));
    candidates.push(PathBuf::from("../../python-ocr/main.py"));

    // 3. Relative to executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            candidates.push(exe_dir.join("python-ocr/main.py"));
            candidates.push(exe_dir.join("resources/python-ocr/main.py"));
            // macOS .app bundle
            candidates.push(exe_dir.join("../Resources/python-ocr/main.py"));
        }
    }

    for candidate in &candidates {
        if candidate.exists() {
            return Ok(candidate.to_string_lossy().to_string());
        }
    }

    // Build detailed error with all attempted paths
    let tried: Vec<String> = candidates.iter().map(|p| p.to_string_lossy().to_string()).collect();
    let resource_info = match resource_dir {
        Some(r) => format!("resource_dir={}", r.display()),
        None => "resource_dir=None".to_string(),
    };
    let cwd = std::env::current_dir().map(|d| d.display().to_string()).unwrap_or_default();
    let exe = std::env::current_exe().map(|d| d.display().to_string()).unwrap_or_default();

    Err(AppError::Ocr(format!(
        "OCR脚本未找到。{resource_info}, cwd={cwd}, exe={exe}\n已尝试路径: {}",
        tried.join("; ")
    )))
}

fn run_ocr_script(script_path: &str, image_path: &str) -> AppResult<(String, Output)> {
    let candidates: &[&[&str]] = if cfg!(target_os = "windows") {
        &[&["python"], &["py", "-3"], &["python3"]]
    } else {
        &[&["python3"], &["python"]]
    };

    let mut spawn_errors = Vec::new();
    for candidate in candidates {
        let mut command = Command::new(candidate[0]);
        for arg in &candidate[1..] {
            command.arg(arg);
        }

        // Force UTF-8 encoding for Python on Windows (fixes Chinese path handling)
        command.env("PYTHONIOENCODING", "utf-8");
        command.env("PYTHONUTF8", "1");

        match command
            .arg(script_path)
            .arg("--image")
            .arg(image_path)
            .arg("--mode")
            .arg("attendance")
            .output()
        {
            Ok(output) => return Ok((candidate.join(" "), output)),
            Err(e) => spawn_errors.push(format!("{}: {e}", candidate.join(" "))),
        }
    }

    Err(AppError::Ocr(format!(
        "无法执行 Python。已尝试: {}",
        spawn_errors.join("; ")
    )))
}

fn extract_ocr_error(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    serde_json::from_str::<Value>(trimmed)
        .ok()
        .and_then(|json| json.get("error").and_then(Value::as_str).map(str::to_string))
        .or_else(|| Some(trimmed.to_string()))
}

fn parse_ocr_output(raw: &str) -> AppResult<(Vec<AttendanceRecordInput>, String)> {
    // Try to parse as JSON first
    if let Ok(json) = serde_json::from_str::<Value>(raw) {
        if json.get("success").and_then(Value::as_bool) == Some(false) {
            let error = json
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("OCR识别失败");
            return Err(AppError::Ocr(error.to_string()));
        }

        if let Some(arr) = json.as_array().or_else(|| json.get("rows").and_then(Value::as_array)) {
            let records = arr.iter().map(parse_attendance_record).collect();
            let raw_text = json
                .get("raw_text")
                .and_then(Value::as_str)
                .unwrap_or(raw)
                .to_string();
            return Ok((records, raw_text));
        }
    }

    // If not JSON, return empty with error
    Err(AppError::Ocr("OCR输出格式无法解析".to_string()))
}

fn parse_attendance_record(item: &Value) -> AttendanceRecordInput {
    AttendanceRecordInput {
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
    }
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
