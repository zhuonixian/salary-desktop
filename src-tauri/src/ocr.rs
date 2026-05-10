use std::path::PathBuf;
use std::process::{Command, Output};

use rusqlite::Connection;
use serde::Deserialize;
use serde_json::Value;

use crate::db::*;
use crate::errors::{AppError, AppResult};
use crate::models::*;

// ==================== Baidu OCR API Types ====================

#[derive(Deserialize)]
struct BaiduTokenResponse {
    access_token: Option<String>,
    expires_in: Option<u64>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct BaiduOcrResponse {
    words_result: Option<Vec<BaiduWord>>,
    words_result_num: Option<u32>,
    error_code: Option<i32>,
    error_msg: Option<String>,
}

#[derive(Deserialize)]
struct BaiduWord {
    words: String,
}

// ==================== Main Entry ====================

/// OCR recognition dispatcher: routes to online (Baidu API) or local (Python) mode.
pub fn ocr_recognize(
    image_path: &str,
    month: &str,
    mode: &str,
    conn: &Connection,
    resource_dir: Option<&std::path::Path>,
) -> AppResult<OcrResult> {
    match mode {
        "online" => ocr_recognize_online(image_path, month, conn),
        _ => ocr_recognize_local(image_path, month, conn, resource_dir),
    }
}

// ==================== Online OCR (Baidu API) ====================

fn ocr_recognize_online(image_path: &str, month: &str, conn: &Connection) -> AppResult<OcrResult> {
    // Read and encode image
    let image_data = std::fs::read(image_path)
        .map_err(|e| AppError::Ocr(format!("读取图片失败: {e}")))?;
    let image_b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &image_data);

    // Get access token
    let access_token = get_baidu_access_token(conn)?;

    // Call Baidu OCR API
    let url = format!(
        "https://aip.baidubce.com/rest/2.0/ocr/v1/general_basic?access_token={access_token}"
    );

    let client = reqwest::blocking::Client::new();
    let response = client
        .post(&url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&[("image", image_b64.as_str()), ("language_type", "CHN_ENG")])
        .send()
        .map_err(|e| AppError::Network(format!("百度OCR请求失败: {e}")))?;

    let body: BaiduOcrResponse = response
        .json()
        .map_err(|e| AppError::Network(format!("百度OCR响应解析失败: {e}")))?;

    if let Some(code) = body.error_code {
        let msg = body.error_msg.unwrap_or_default();
        return Err(AppError::Ocr(format!(
            "百度OCR错误({code}): {msg}"
        )));
    }

    let words = body.words_result.unwrap_or_default();
    let raw_text = words.iter().map(|w| w.words.as_str()).collect::<Vec<_>>().join("\n");

    // Parse into attendance records
    let records = parse_online_text_to_records(&raw_text);

    // Save batch
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
        r.source_type = Some("ocr-online".to_string());
        r.ocr_batch_id = Some(batch_id);
        ocr_records.push(r);
    }

    Ok(OcrResult {
        batch_id,
        records: ocr_records,
        raw_text: Some(raw_text),
    })
}

fn get_baidu_access_token(conn: &Connection) -> AppResult<String> {
    // Check cached token
    if let Some(token) = get_setting(conn, "baidu_access_token")? {
        if let Some(expires_str) = get_setting(conn, "baidu_token_expires_at")? {
            if let Ok(expires) = expires_str.parse::<i64>() {
                // Refresh 1 day before expiry
                let now = chrono::Utc::now().timestamp();
                if now < expires - 86400 {
                    return Ok(token);
                }
            }
        }
    }

    // Fetch new token
    let api_key = get_setting(conn, "baidu_api_key")?.ok_or_else(|| {
        AppError::Ocr("请先在设置中配置百度 OCR API Key".to_string())
    })?;
    let secret_key = get_setting(conn, "baidu_secret_key")?.ok_or_else(|| {
        AppError::Ocr("请先在设置中配置百度 OCR Secret Key".to_string())
    })?;

    let url = format!(
        "https://aip.baidubce.com/oauth/2.0/token?grant_type=client_credentials&client_id={api_key}&client_secret={secret_key}"
    );

    let response = reqwest::blocking::Client::new()
        .post(&url)
        .send()
        .map_err(|e| AppError::Network(format!("获取百度Token失败: {e}")))?;

    let token_resp: BaiduTokenResponse = response
        .json()
        .map_err(|e| AppError::Network(format!("解析Token响应失败: {e}")))?;

    if let Some(err) = token_resp.error {
        return Err(AppError::Ocr(format!("百度Token获取失败: {err}")));
    }

    let token = token_resp.access_token.ok_or_else(|| {
        AppError::Ocr("百度Token响应中无access_token".to_string())
    })?;

    // Cache token (expires_in is in seconds, typically 2592000 = 30 days)
    let expires_in = token_resp.expires_in.unwrap_or(2592000);
    let expires_at = chrono::Utc::now().timestamp() + expires_in as i64;

    set_setting(conn, "baidu_access_token", &token)?;
    set_setting(conn, "baidu_token_expires_at", &expires_at.to_string())?;

    Ok(token)
}

/// Parse raw OCR text (line-separated words) into attendance records.
/// Tries to detect a table structure with column headers.
fn parse_online_text_to_records(raw_text: &str) -> Vec<AttendanceRecordInput> {
    let lines: Vec<&str> = raw_text.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    if lines.is_empty() {
        return Vec::new();
    }

    // Column keyword mapping
    let header_keywords: &[(&[&str], &str)] = &[
        (&["工号", "编号", "员工编号", "employee_no"], "employee_no"),
        (&["姓名", "名字", "name"], "name"),
        (&["应出勤", "应出勤天数", "应到", "expected"], "expected_days"),
        (&["实出勤", "实出勤天数", "实到", "actual"], "actual_days"),
        (&["迟到", "迟到次数", "late"], "late_count"),
        (&["早退", "早退次数", "early"], "early_leave_count"),
        (&["事假", "事假天数", "personal"], "personal_leave_days"),
        (&["病假", "病假天数", "sick"], "sick_leave_days"),
        (&["旷工", "旷工天数", "absent"], "absent_days"),
        (&["加班", "加班时长", "加班小时", "overtime"], "overtime_hours"),
    ];

    // Find header line
    let mut header_idx = None;
    let mut col_map: Vec<(usize, &str)> = Vec::new(); // (column_index, field_name)

    for (i, line) in lines.iter().enumerate() {
        let cells: Vec<&str> = line.split_whitespace().collect();
        let mut found_cols = Vec::new();

        for (col_idx, cell) in cells.iter().enumerate() {
            for (keywords, field) in header_keywords {
                if keywords.iter().any(|k| cell.contains(k)) {
                    found_cols.push((col_idx, *field));
                    break;
                }
            }
        }

        if found_cols.len() >= 2 {
            header_idx = Some(i);
            col_map = found_cols;
            break;
        }
    }

    let data_start = header_idx.map(|i| i + 1).unwrap_or(0);

    // Parse data lines
    let mut records = Vec::new();
    for line in lines.iter().skip(data_start) {
        let cells: Vec<&str> = line.split_whitespace().collect();
        if cells.is_empty() {
            continue;
        }

        let mut record = AttendanceRecordInput {
            id: None,
            salary_month: String::new(),
            employee_no: String::new(),
            name: None,
            expected_days: None,
            actual_days: None,
            late_count: None,
            early_leave_count: None,
            personal_leave_days: None,
            sick_leave_days: None,
            absent_days: None,
            overtime_hours: None,
            source_type: None,
            ocr_batch_id: None,
            remark: None,
        };

        if col_map.is_empty() {
            // No header detected - try positional: employee_no name numbers...
            if cells.len() >= 2 {
                record.employee_no = cells[0].to_string();
                record.name = Some(cells[1].to_string());
                if let Some(v) = parse_f64(cells.get(2)) { record.expected_days = Some(v); }
                if let Some(v) = parse_f64(cells.get(3)) { record.actual_days = Some(v); }
                if let Some(v) = parse_i32(cells.get(4)) { record.late_count = Some(v); }
                if let Some(v) = parse_i32(cells.get(5)) { record.early_leave_count = Some(v); }
                if let Some(v) = parse_f64(cells.get(6)) { record.personal_leave_days = Some(v); }
                if let Some(v) = parse_f64(cells.get(7)) { record.sick_leave_days = Some(v); }
                if let Some(v) = parse_f64(cells.get(8)) { record.absent_days = Some(v); }
                if let Some(v) = parse_f64(cells.get(9)) { record.overtime_hours = Some(v); }
            }
        } else {
            // Use header mapping
            for (col_idx, field) in &col_map {
                let cell = cells.get(*col_idx).unwrap_or(&"");
                match *field {
                    "employee_no" => record.employee_no = cell.to_string(),
                    "name" => record.name = Some(cell.to_string()),
                    "expected_days" => record.expected_days = parse_f64(Some(*cell)),
                    "actual_days" => record.actual_days = parse_f64(Some(*cell)),
                    "late_count" => record.late_count = parse_i32(Some(*cell)),
                    "early_leave_count" => record.early_leave_count = parse_i32(Some(*cell)),
                    "personal_leave_days" => record.personal_leave_days = parse_f64(Some(*cell)),
                    "sick_leave_days" => record.sick_leave_days = parse_f64(Some(*cell)),
                    "absent_days" => record.absent_days = parse_f64(Some(*cell)),
                    "overtime_hours" => record.overtime_hours = parse_f64(Some(*cell)),
                    _ => {}
                }
            }
        }

        if !record.employee_no.is_empty() {
            records.push(record);
        }
    }

    records
}

fn parse_f64(s: Option<impl AsRef<str>>) -> Option<f64> {
    s.and_then(|v| v.as_ref().replace('．', ".").parse::<f64>().ok())
}

fn parse_i32(s: Option<impl AsRef<str>>) -> Option<i32> {
    s.and_then(|v| v.as_ref().replace('．', ".").parse::<f64>().ok()).map(|v| v as i32)
}

// ==================== Local OCR (Python PaddleOCR) ====================

/// Decode bytes from Python output: try UTF-8 first, fallback to GBK (Windows Chinese)
fn decode_bytes(data: &[u8]) -> String {
    if let Ok(s) = String::from_utf8(data.to_vec()) {
        return s;
    }
    let (decoded, _, _) = encoding_rs::GBK.decode(data);
    let decoded = decoded.to_string();
    if !decoded.contains('\u{fffd}') {
        return decoded;
    }
    String::from_utf8_lossy(data).to_string()
}

fn ocr_recognize_local(
    image_path: &str,
    month: &str,
    conn: &Connection,
    resource_dir: Option<&std::path::Path>,
) -> AppResult<OcrResult> {
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
    let (records, raw_text) = parse_ocr_output(&output_text)?;

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

    if let Some(rdir) = resource_dir {
        candidates.push(rdir.join("python-ocr/main.py"));
        candidates.push(rdir.join("main.py"));
    }

    candidates.push(PathBuf::from("python-ocr/main.py"));
    candidates.push(PathBuf::from("../python-ocr/main.py"));
    candidates.push(PathBuf::from("../../python-ocr/main.py"));

    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            candidates.push(exe_dir.join("python-ocr/main.py"));
            candidates.push(exe_dir.join("resources/python-ocr/main.py"));
            candidates.push(exe_dir.join("../Resources/python-ocr/main.py"));
        }
    }

    for candidate in &candidates {
        if candidate.exists() {
            return Ok(candidate.to_string_lossy().to_string());
        }
    }

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

// ==================== Confirm OCR Results ====================

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

// ==================== OCR Settings ====================

pub fn get_ocr_settings(conn: &Connection) -> AppResult<OcrSettings> {
    Ok(OcrSettings {
        ocr_mode: get_setting(conn, "ocr_mode")?.unwrap_or_else(|| "local".to_string()),
        baidu_api_key: get_setting(conn, "baidu_api_key")?.unwrap_or_default(),
        baidu_secret_key: get_setting(conn, "baidu_secret_key")?.unwrap_or_default(),
    })
}

pub fn save_ocr_settings(conn: &Connection, data: &OcrSettingsInput) -> AppResult<bool> {
    if let Some(ref v) = data.ocr_mode {
        set_setting(conn, "ocr_mode", v)?;
    }
    if let Some(ref v) = data.baidu_api_key {
        set_setting(conn, "baidu_api_key", v)?;
    }
    if let Some(ref v) = data.baidu_secret_key {
        set_setting(conn, "baidu_secret_key", v)?;
    }
    // Clear cached token when credentials change
    if data.baidu_api_key.is_some() || data.baidu_secret_key.is_some() {
        set_setting(conn, "baidu_access_token", "")?;
    }
    Ok(true)
}
