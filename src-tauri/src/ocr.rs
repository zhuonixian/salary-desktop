use std::path::PathBuf;
use std::process::{Command, Output};

use rusqlite::Connection;
use serde::Deserialize;
use serde_json::Value;

use crate::db::*;
use crate::errors::{AppError, AppResult};
use crate::models::*;
use chrono::Datelike;

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

#[derive(Deserialize, Clone)]
struct BaiduWord {
    words: String,
    location: Option<BaiduLocation>,
}

#[derive(Deserialize, Clone)]
struct BaiduLocation {
    left: u32,
    top: u32,
    width: u32,
    height: u32,
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

pub(crate) fn get_baidu_access_token(conn: &Connection) -> AppResult<String> {
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

// ==================== Punch Card OCR ====================

/// Recognize punch card image: call Baidu accurate API (returns location data), then parse.
/// The punch card has day+night sub-columns per date, and 2 rows per employee.
pub fn ocr_recognize_punch_card(
    image_path: &str,
    month: &str,
    _shift_type: &str,
    mode: &str,
    conn: &Connection,
) -> AppResult<OcrResult> {
    if mode != "online" {
        return Err(AppError::Ocr("打卡表识别目前仅支持在线模式".to_string()));
    }

    // Read and encode image
    let image_data = std::fs::read(image_path)
        .map_err(|e| AppError::Ocr(format!("读取图片失败: {e}")))?;
    let image_b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &image_data);

    // Get access token
    let access_token = get_baidu_access_token(conn)?;

    // Call Baidu accurate OCR API (returns location data per word)
    let url = format!(
        "https://aip.baidubce.com/rest/2.0/ocr/v1/accurate?access_token={access_token}"
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
        return Err(AppError::Ocr(format!("百度OCR错误({code}): {msg}")));
    }

    let words = body.words_result.unwrap_or_default();
    let raw_text = words.iter().map(|w| w.words.as_str()).collect::<Vec<_>>().join("\n");

    // Parse punch card using position data
    let records = parse_punch_card_ocr(&words, month)?;

    // Save batch
    let parsed_json = serde_json::to_string(&records).unwrap_or_default();
    let batch = OcrBatch {
        id: 0,
        batch_name: Some(format!("打卡表-{}", chrono::Utc::now().format("%Y%m%d%H%M%S"))),
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
        r.ocr_batch_id = Some(batch_id);
        ocr_records.push(r);
    }

    Ok(OcrResult {
        batch_id,
        records: ocr_records,
        raw_text: Some(raw_text),
    })
}

/// Parse punch card OCR results using position data from Baidu API.
///
/// Strategy: Don't rely on detecting 白/夜 sub-headers (they're often too small for OCR).
/// Instead:
/// 1. Group ALL words into rows by Y-coordinate
/// 2. Find employee rows by pattern: sequence_number + name at left side
/// 3. For each employee row, count check marks (√/✓ etc.) that appear to the RIGHT of the name
/// 4. If the next row has no sequence number, it's the night shift row — also count its marks
/// 5. Merge day + night into one attendance record
fn parse_punch_card_ocr(
    words: &[BaiduWord],
    month: &str,
) -> AppResult<Vec<AttendanceRecordInput>> {
    if words.is_empty() {
        return Err(AppError::Ocr("打卡表OCR结果为空".to_string()));
    }

    let days_in_month = get_days_in_month(month);
    let weekdays = compute_workdays(month, days_in_month);

    // Step 1: Group ALL words into rows by Y-coordinate (including header)
    let row_groups = group_words_by_row_all(words);

    // Step 2: Identify the "name column" X range by finding Chinese names near sequence numbers
    let name_col_max_x = find_name_column_boundary(&row_groups);

    // Step 3: Find employee rows and count their check marks
    let mut records = Vec::new();
    let mut i = 0;

    while i < row_groups.len() {
        let row = &row_groups[i];

        // Sort row by X position
        let mut sorted_row = row.clone();
        sorted_row.sort_by_key(|w| w.location.as_ref().map(|l| l.left).unwrap_or(0));

        // Check if this row starts with a sequence number (1-100)
        let first_text = sorted_row.first().map(|w| w.words.trim().to_string()).unwrap_or_default();
        let seq: Option<u32> = first_text.parse().ok();
        if seq.is_none() || seq.unwrap() == 0 || seq.unwrap() > 100 {
            i += 1;
            continue;
        }

        // Find name and employee_no from the left side of the row
        let mut name = String::new();
        let mut employee_no: Option<String> = None;
        for w in sorted_row.iter().skip(1) {
            let text = w.words.trim();
            if text.is_empty() { continue; }
            // Stop once we're past the name column area
            let wx = w.location.as_ref().map(|l| l.left).unwrap_or(0);
            if wx > name_col_max_x { break; }
            if text.chars().all(|c| c.is_ascii_digit()) && employee_no.is_none() {
                employee_no = Some(text.to_string());
            } else if !text.chars().all(|c| c.is_ascii_digit() || c == '.') {
                if name.is_empty() {
                    name = text.to_string();
                }
            }
        }

        if name.is_empty() {
            i += 1;
            continue;
        }

        // Count check marks in this row (to the RIGHT of the name column)
        let day_marks = count_row_marks(&sorted_row, name_col_max_x);

        // Check if next row is the night shift row (no sequence number, or same employee)
        let mut night_marks: f64 = 0.0;
        if i + 1 < row_groups.len() {
            let next_row = &row_groups[i + 1];
            let mut next_sorted = next_row.clone();
            next_sorted.sort_by_key(|w| w.location.as_ref().map(|l| l.left).unwrap_or(0));
            let next_first = next_sorted.first().map(|w| w.words.trim().to_string()).unwrap_or_default();
            let next_seq: Option<u32> = next_first.parse().ok();
            // Next row is night shift if it doesn't start with the next sequence number
            if next_seq.is_none() || next_seq.unwrap() != seq.unwrap() + 1 {
                night_marks = count_row_marks(&next_sorted, name_col_max_x);
                i += 1; // consume the night row
            }
        }

        let total_present = day_marks + night_marks;
        let expected = weekdays.len() as f64;

        let mut remark_parts = Vec::new();
        if day_marks > 0.0 { remark_parts.push(format!("白班{}天", day_marks as i32)); }
        if night_marks > 0.0 { remark_parts.push(format!("夜班{}天", night_marks as i32)); }

        records.push(AttendanceRecordInput {
            id: None,
            salary_month: month.to_string(),
            employee_no: employee_no.unwrap_or_default(),
            name: Some(name),
            expected_days: Some(expected),
            actual_days: Some(total_present),
            late_count: None,
            early_leave_count: None,
            personal_leave_days: None,
            sick_leave_days: None,
            absent_days: Some((expected - total_present).max(0.0)),
            overtime_hours: None,
            source_type: Some("punch_card".to_string()),
            ocr_batch_id: None,
            remark: if remark_parts.is_empty() { None } else { Some(remark_parts.join(" ")) },
        });

        i += 1;
    }

    if records.is_empty() {
        let sample: Vec<String> = words.iter().take(20).map(|w| {
            let loc = w.location.as_ref().map(|l| format!("[{},{}]", l.left, l.top)).unwrap_or_default();
            format!("{}{loc}", w.words)
        }).collect();
        return Err(AppError::Ocr(format!(
            "未能从打卡表中识别出员工记录。OCR返回{}个文字，前20个: {}",
            words.len(), sample.join("; ")
        )));
    }

    Ok(records)
}

/// Group ALL words into rows by Y-coordinate proximity (no header filtering).
fn group_words_by_row_all(words: &[BaiduWord]) -> Vec<Vec<&BaiduWord>> {
    let mut sorted: Vec<&BaiduWord> = words.iter()
        .filter(|w| w.location.is_some())
        .collect();
    sorted.sort_by_key(|w| w.location.as_ref().unwrap().top);

    let mut groups: Vec<Vec<&BaiduWord>> = Vec::new();
    let mut current_row: Vec<&BaiduWord> = Vec::new();
    let mut current_y: i32 = -100;

    for w in &sorted {
        let loc = w.location.as_ref().unwrap();
        let y = loc.top as i32;
        if (y - current_y).abs() > 15 && !current_row.is_empty() {
            groups.push(current_row.clone());
            current_row.clear();
        }
        current_y = y;
        current_row.push(w);
    }
    if !current_row.is_empty() {
        groups.push(current_row);
    }
    groups
}

/// Find the right boundary of the "name column" by looking at X positions
/// of non-numeric text that appears near sequence numbers in employee rows.
fn find_name_column_boundary(row_groups: &[Vec<&BaiduWord>]) -> u32 {
    let mut max_name_x: u32 = 200; // default: assume name column ends at ~200px

    for row in row_groups {
        let mut sorted_row = row.clone();
        sorted_row.sort_by_key(|w| w.location.as_ref().map(|l| l.left).unwrap_or(0));

        let first_text = sorted_row.first().map(|w| w.words.trim().to_string()).unwrap_or_default();
        let seq: Option<u32> = first_text.parse().ok();
        if seq.is_none() || seq.unwrap() == 0 || seq.unwrap() > 100 {
            continue;
        }

        // Find the rightmost non-checkmark, non-pure-number word in the first 3 positions
        for w in sorted_row.iter().take(4) {
            let text = w.words.trim();
            if text.is_empty() { continue; }
            // Skip pure numbers (sequence number, employee_no)
            if text.chars().all(|c| c.is_ascii_digit()) { continue; }
            // Skip check marks
            if is_check_mark(text) { continue; }
            // This is likely a name - record its right edge
            if let Some(loc) = &w.location {
                let right_edge = loc.left + loc.width + 20; // add margin
                if right_edge > max_name_x {
                    max_name_x = right_edge;
                }
            }
        }
    }

    max_name_x
}

/// Count check marks in a sorted row that appear to the RIGHT of the name column.
fn count_row_marks(sorted_row: &[&BaiduWord], name_col_max_x: u32) -> f64 {
    let mut count = 0.0;
    for w in sorted_row {
        let text = w.words.trim();
        let wx = w.location.as_ref().map(|l| l.left).unwrap_or(0);
        // Only count marks to the right of the name column
        if wx > name_col_max_x && is_check_mark(text) {
            count += 1.0;
        }
    }
    count
}

fn is_check_mark(text: &str) -> bool {
    let t = text.trim();
    t == "√" || t == "✓" || t == "✔" || t == "✗" || t == "签" || t == "V" || t == "v"
        || t.contains("签") || t.contains("√") || t.contains("✓")
        || (t.len() == 1 && t.chars().next().map(|c| c as u32).map(|c| c == 0x2713 || c == 0x2714 || c == 0x221A).unwrap_or(false))
}

fn get_days_in_month(month: &str) -> u32 {
    let parts: Vec<&str> = month.split('-').collect();
    if parts.len() != 2 { return 31; }
    let year: u32 = parts[0].parse().unwrap_or(2026);
    let mon: u32 = parts[1].parse().unwrap_or(1);
    match mon {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => if (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0) { 29 } else { 28 },
        _ => 31,
    }
}

/// Compute workdays (Mon-Fri) for the given month
fn compute_workdays(month: &str, days_in_month: u32) -> Vec<u32> {
    let parts: Vec<&str> = month.split('-').collect();
    let year: i32 = parts.first().and_then(|s| s.parse().ok()).unwrap_or(2026);
    let mon: u32 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);

    let mut workdays = Vec::new();
    for day in 1..=days_in_month {
        if let Some(date) = chrono::NaiveDate::from_ymd_opt(year, mon, day) {
            let weekday = date.weekday().num_days_from_monday(); // 0=Mon, 6=Sun
            if weekday < 5 { // Mon-Fri
                workdays.push(day);
            }
        }
    }
    workdays
}
