use std::collections::HashMap;
use std::fs;
use std::path::Path;

use calamine::{open_workbook_auto, Data, Reader};
use rusqlite::params;
use rust_xlsxwriter::{Format, Workbook};

use crate::cashier::get_fund_daily_report;
use crate::db::get_input_tax_ledger;
use crate::errors::{AppError, AppResult};
use crate::models::*;

// ==================== Employee Excel Import ====================

pub fn read_employee_excel(path: &str) -> AppResult<Vec<Employee>> {
    let mut workbook = open_workbook_auto(path)?;
    let mut employees = Vec::new();

    let sheet_names = workbook.sheet_names().to_vec();
    if sheet_names.is_empty() {
        return Err(AppError::InvalidParam("Excel文件没有工作表".to_string()));
    }

    let range = workbook.worksheet_range(&sheet_names[0])?;

    let mut rows = range.rows();
    // Skip header row, detect column mapping
    let headers = if let Some(header_row) = rows.next() {
        let mut map: HashMap<String, usize> = HashMap::new();
        for (i, cell) in header_row.iter().enumerate() {
            let val = cell.to_string();
            let key = val.trim().to_string();
            map.insert(key, i);
        }
        map
    } else {
        return Ok(employees);
    };

    let get_col = |name: &str, headers: &HashMap<String, usize>| -> Option<usize> {
        headers.get(name).copied()
    };

    let col_no = get_col("工号", &headers).or_else(|| get_col("employee_no", &headers));
    let col_name = get_col("姓名", &headers).or_else(|| get_col("name", &headers));
    let col_dept = get_col("部门", &headers).or_else(|| get_col("department", &headers));
    let col_pos = get_col("职位", &headers).or_else(|| get_col("position", &headers));
    let col_idcard = get_col("身份证号", &headers).or_else(|| get_col("id_card", &headers));
    let col_phone = get_col("手机号", &headers).or_else(|| get_col("phone", &headers));
    // 邮箱列（stage9 spec 3.1）：模板列名「邮箱(email)」，兼容「邮箱」/「email」旧表头
    let col_email = get_col("邮箱(email)", &headers)
        .or_else(|| get_col("邮箱", &headers))
        .or_else(|| get_col("email", &headers));
    let col_bank = get_col("银行账号", &headers).or_else(|| get_col("bank_account", &headers));
    let col_bankname = get_col("开户行", &headers).or_else(|| get_col("bank_name", &headers));
    let col_hire = get_col("入职日期", &headers).or_else(|| get_col("hire_date", &headers));
    let col_base = get_col("基本工资", &headers).or_else(|| get_col("base_salary", &headers));
    let col_pos_salary =
        get_col("岗位工资", &headers).or_else(|| get_col("position_salary", &headers));
    let col_perf =
        get_col("绩效工资", &headers).or_else(|| get_col("performance_salary", &headers));
    let col_social =
        get_col("社保基数", &headers).or_else(|| get_col("social_security_base", &headers));
    let col_housing =
        get_col("公积金基数", &headers).or_else(|| get_col("housing_fund_base", &headers));
    let col_special =
        get_col("专项附加扣除", &headers).or_else(|| get_col("special_deduction", &headers));
    let col_remark = get_col("备注", &headers).or_else(|| get_col("remark", &headers));

    if col_no.is_none() || col_name.is_none() {
        return Err(AppError::InvalidParam(
            "Excel缺少必要列: 需要包含'工号'和'姓名'列".to_string(),
        ));
    }

    for row in rows {
        let get_string = |col: Option<usize>| -> Option<String> {
            col.and_then(|i| row.get(i)).map(|c| {
                let s = c.to_string();
                if s.is_empty() {
                    String::new()
                } else {
                    s
                }
            })
        };

        let get_f64 = |col: Option<usize>| -> f64 {
            col.and_then(|i| row.get(i))
                .map(|c| match c {
                    Data::Float(f) => *f,
                    Data::Int(i) => *i as f64,
                    Data::String(s) => s.parse::<f64>().unwrap_or(0.0),
                    _ => 0.0,
                })
                .unwrap_or(0.0)
        };

        let employee_no = get_string(col_no).unwrap_or_default();
        let name = get_string(col_name).unwrap_or_default();

        if employee_no.trim().is_empty() || name.trim().is_empty() {
            continue;
        }

        employees.push(Employee {
            id: 0,
            employee_no: employee_no.trim().to_string(),
            name: name.trim().to_string(),
            department: get_string(col_dept),
            position: get_string(col_pos),
            id_card: get_string(col_idcard),
            phone: get_string(col_phone),
            email: {
                // 邮箱 trim 后空归 None（带空白邮箱会导致 SMTP 收件失败）
                let raw = get_string(col_email);
                raw.map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            },
            bank_account: get_string(col_bank),
            bank_name: get_string(col_bankname),
            hire_date: get_string(col_hire),
            status: "active".to_string(),
            base_salary: get_f64(col_base),
            position_salary: get_f64(col_pos_salary),
            performance_salary: get_f64(col_perf),
            social_security_base: get_f64(col_social),
            housing_fund_base: get_f64(col_housing),
            special_deduction: get_f64(col_special),
            remark: get_string(col_remark),
            created_at: None,
            updated_at: None,
        });
    }

    Ok(employees)
}

// ==================== Attendance Excel Import ====================

pub fn read_attendance_excel(path: &str, month: &str) -> AppResult<Vec<AttendanceRecord>> {
    let mut workbook = open_workbook_auto(path)?;
    let mut records = Vec::new();

    let sheet_names = workbook.sheet_names().to_vec();
    if sheet_names.is_empty() {
        return Err(AppError::InvalidParam("Excel文件没有工作表".to_string()));
    }

    let range = workbook.worksheet_range(&sheet_names[0])?;

    let mut rows = range.rows();
    let headers = if let Some(header_row) = rows.next() {
        let mut map: HashMap<String, usize> = HashMap::new();
        for (i, cell) in header_row.iter().enumerate() {
            let key = cell.to_string().trim().to_string();
            map.insert(key, i);
        }
        map
    } else {
        return Ok(records);
    };

    let get_col = |name: &str, headers: &HashMap<String, usize>| -> Option<usize> {
        headers.get(name).copied()
    };

    let col_no = get_col("工号", &headers).or_else(|| get_col("employee_no", &headers));
    let col_name = get_col("姓名", &headers).or_else(|| get_col("name", &headers));
    let col_expected =
        get_col("应出勤天数", &headers).or_else(|| get_col("expected_days", &headers));
    let col_actual = get_col("实出勤天数", &headers).or_else(|| get_col("actual_days", &headers));
    let col_late = get_col("迟到次数", &headers).or_else(|| get_col("late_count", &headers));
    let col_early =
        get_col("早退次数", &headers).or_else(|| get_col("early_leave_count", &headers));
    let col_personal =
        get_col("事假天数", &headers).or_else(|| get_col("personal_leave_days", &headers));
    let col_sick = get_col("病假天数", &headers).or_else(|| get_col("sick_leave_days", &headers));
    let col_absent = get_col("旷工天数", &headers).or_else(|| get_col("absent_days", &headers));
    let col_overtime =
        get_col("加班小时", &headers).or_else(|| get_col("overtime_hours", &headers));
    let col_remark = get_col("备注", &headers).or_else(|| get_col("remark", &headers));

    if col_no.is_none() {
        return Err(AppError::InvalidParam(
            "Excel缺少必要列: 需要包含'工号'列".to_string(),
        ));
    }

    for row in rows {
        let get_string = |col: Option<usize>| -> Option<String> {
            col.and_then(|i| row.get(i)).map(|c| {
                let s = c.to_string();
                if s.is_empty() {
                    String::new()
                } else {
                    s
                }
            })
        };

        let get_f64 = |col: Option<usize>| -> f64 {
            col.and_then(|i| row.get(i))
                .map(|c| match c {
                    Data::Float(f) => *f,
                    Data::Int(i) => *i as f64,
                    Data::String(s) => s.parse::<f64>().unwrap_or(0.0),
                    _ => 0.0,
                })
                .unwrap_or(0.0)
        };

        let get_i32 = |col: Option<usize>| -> i32 { get_f64(col) as i32 };

        let employee_no = get_string(col_no).unwrap_or_default();
        if employee_no.trim().is_empty() {
            continue;
        }

        records.push(AttendanceRecord {
            id: 0,
            salary_month: month.to_string(),
            employee_no: employee_no.trim().to_string(),
            name: get_string(col_name),
            expected_days: get_f64(col_expected),
            actual_days: get_f64(col_actual),
            late_count: get_i32(col_late),
            early_leave_count: get_i32(col_early),
            personal_leave_days: get_f64(col_personal),
            sick_leave_days: get_f64(col_sick),
            absent_days: get_f64(col_absent),
            overtime_hours: get_f64(col_overtime),
            source_type: Some("excel_import".to_string()),
            ocr_batch_id: None,
            remark: get_string(col_remark),
            created_at: None,
            updated_at: None,
        });
    }

    Ok(records)
}

// ==================== Bank Transaction Import ====================

/// 银行流水文件解析结果：表头（字段识别展示）+ 解析出的流水行
pub struct BankTransactionFileContent {
    pub headers: Vec<String>,
    pub transactions: Vec<BankTransaction>,
}

pub fn read_bank_transactions_file(path: &str) -> AppResult<Vec<BankTransaction>> {
    Ok(read_bank_transactions_file_content(path)?.transactions)
}

/// 解析流水文件（CSV/Excel），同时返回表头供导入预览做字段识别（Task 11）
pub fn read_bank_transactions_file_content(path: &str) -> AppResult<BankTransactionFileContent> {
    let ext = Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "csv" {
        read_bank_transactions_csv(path)
    } else {
        read_bank_transactions_excel(path)
    }
}

fn read_bank_transactions_excel(path: &str) -> AppResult<BankTransactionFileContent> {
    let mut workbook = open_workbook_auto(path)?;
    let sheet_names = workbook.sheet_names().to_vec();
    if sheet_names.is_empty() {
        return Err(AppError::InvalidParam("Excel文件没有工作表".to_string()));
    }

    let range = workbook.worksheet_range(&sheet_names[0])?;
    let mut rows = range.rows();
    let (header_map, headers) = if let Some(header_row) = rows.next() {
        let raw: Vec<String> = header_row.iter().map(|cell| cell.to_string()).collect();
        (
            header_map(raw.clone()),
            raw.into_iter()
                .map(|h| h.trim().trim_start_matches('\u{feff}').to_string())
                .collect(),
        )
    } else {
        return Ok(BankTransactionFileContent {
            headers: Vec::new(),
            transactions: Vec::new(),
        });
    };

    let mut transactions = Vec::new();
    for row in rows {
        let values: Vec<String> = row.iter().map(cell_to_string).collect();
        if let Some(transaction) = bank_transaction_from_values(&header_map, &values, path)? {
            transactions.push(transaction);
        }
    }
    Ok(BankTransactionFileContent {
        headers,
        transactions,
    })
}

fn read_bank_transactions_csv(path: &str) -> AppResult<BankTransactionFileContent> {
    let content = fs::read_to_string(path)?;
    let mut lines = content.lines();
    let Some(header_line) = lines.next() else {
        return Ok(BankTransactionFileContent {
            headers: Vec::new(),
            transactions: Vec::new(),
        });
    };
    let raw_headers = split_csv_line(header_line);
    let headers = header_map(raw_headers.clone());

    let mut transactions = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let values = split_csv_line(line);
        if let Some(transaction) = bank_transaction_from_values(&headers, &values, path)? {
            transactions.push(transaction);
        }
    }
    Ok(BankTransactionFileContent {
        headers: raw_headers
            .into_iter()
            .map(|h| h.trim().trim_start_matches('\u{feff}').to_string())
            .collect(),
        transactions,
    })
}

fn header_map(headers: Vec<String>) -> HashMap<String, usize> {
    headers
        .into_iter()
        .enumerate()
        .map(|(idx, header)| {
            (
                header.trim().trim_start_matches('\u{feff}').to_string(),
                idx,
            )
        })
        .collect()
}

fn bank_transaction_from_values(
    headers: &HashMap<String, usize>,
    values: &[String],
    path: &str,
) -> AppResult<Option<BankTransaction>> {
    let transaction_date = get_bank_value(
        headers,
        values,
        &["交易日期", "记账日期", "日期", "transaction_date", "date"],
    )
    .unwrap_or_default();
    if transaction_date.trim().is_empty() {
        return Ok(None);
    }
    let transaction_date = normalize_bank_date(&transaction_date)?;
    let belong_month = transaction_date.chars().take(7).collect::<String>();
    let summary = get_bank_value(
        headers,
        values,
        &["摘要", "用途", "备注", "summary", "remark"],
    );
    let counterparty_name = get_bank_value(
        headers,
        values,
        &["对方户名", "对方名称", "收付款方", "counterparty_name"],
    );
    let counterparty_account = get_bank_value(
        headers,
        values,
        &["对方账号", "对方账户", "counterparty_account"],
    );
    let income_amount = parse_money(
        get_bank_value(headers, values, &["收入", "贷方金额", "income", "credit"])
            .as_deref()
            .unwrap_or(""),
    );
    let expense_amount = parse_money(
        get_bank_value(
            headers,
            values,
            &["支出", "借方金额", "付款金额", "expense", "debit"],
        )
        .as_deref()
        .unwrap_or(""),
    );
    let balance =
        get_bank_value(headers, values, &["余额", "balance"]).map(|value| parse_money(&value));

    if income_amount == 0.0 && expense_amount == 0.0 {
        return Ok(None);
    }

    let raw_json = serde_json::to_string(values).ok();
    Ok(Some(BankTransaction {
        id: 0,
        transaction_date,
        belong_month,
        summary,
        counterparty_name,
        counterparty_account,
        income_amount,
        expense_amount,
        balance,
        status: "unmatched".to_string(),
        ignore_reason: None,
        imported_file: Some(path.to_string()),
        raw_json,
        fund_account_id: None,
        fund_account_name: None,
        matched_batch_id: None,
        matched_batch_no: None,
        matched_batch_type: None,
        matched_amount: None,
        match_score: None,
        match_remark: None,
        allocated_amount: None,
        remaining_amount: None,
        created_at: None,
        updated_at: None,
    }))
}

fn get_bank_value(
    headers: &HashMap<String, usize>,
    values: &[String],
    aliases: &[&str],
) -> Option<String> {
    aliases
        .iter()
        .find_map(|alias| headers.get(*alias))
        .and_then(|idx| values.get(*idx))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn normalize_bank_date(value: &str) -> AppResult<String> {
    let trimmed = value.trim().replace('/', "-").replace('.', "-");
    if trimmed.len() >= 10 {
        return Ok(trimmed.chars().take(10).collect());
    }
    Err(AppError::InvalidParam(format!(
        "银行流水日期格式无效: {value}"
    )))
}

fn parse_money(value: &str) -> f64 {
    value
        .trim()
        .replace(',', "")
        .replace('，', "")
        .replace('¥', "")
        .parse::<f64>()
        .unwrap_or(0.0)
}

fn cell_to_string(cell: &Data) -> String {
    match cell {
        Data::Float(value) => value.to_string(),
        Data::Int(value) => value.to_string(),
        Data::String(value) => value.trim().to_string(),
        Data::Bool(value) => value.to_string(),
        _ => cell.to_string(),
    }
}

fn split_csv_line(line: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars().peekable();
    let mut in_quotes = false;

    while let Some(ch) = chars.next() {
        match ch {
            '"' if in_quotes && chars.peek() == Some(&'"') => {
                current.push('"');
                chars.next();
            }
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                values.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    values.push(current.trim().to_string());
    values
}

// ==================== Export Helpers ====================

fn make_header_format() -> Format {
    Format::new()
        .set_bold()
        .set_background_color("#4472C4")
        .set_font_color("#FFFFFF")
        .set_border(rust_xlsxwriter::FormatBorder::Thin)
        .set_align(rust_xlsxwriter::FormatAlign::Center)
        .set_align(rust_xlsxwriter::FormatAlign::VerticalCenter)
        .set_text_wrap()
}

fn make_cell_format() -> Format {
    Format::new()
        .set_border(rust_xlsxwriter::FormatBorder::Thin)
        .set_align(rust_xlsxwriter::FormatAlign::Center)
        .set_align(rust_xlsxwriter::FormatAlign::VerticalCenter)
}

fn make_money_format() -> Format {
    Format::new()
        .set_border(rust_xlsxwriter::FormatBorder::Thin)
        .set_align(rust_xlsxwriter::FormatAlign::Center)
        .set_align(rust_xlsxwriter::FormatAlign::VerticalCenter)
        .set_num_format("#,##0.00")
}

fn write_header_row(
    worksheet: &mut rust_xlsxwriter::Worksheet,
    headers: &[&str],
    row: u32,
) -> AppResult<()> {
    let format = make_header_format();
    for (col, header) in headers.iter().enumerate() {
        worksheet.write_string_with_format(row, col as u16, *header, &format)?;
    }
    Ok(())
}

// ==================== Import Templates ====================

fn next_employee_no(employees: &[Employee]) -> String {
    let max_no = employees
        .iter()
        .filter_map(|employee| {
            let employee_no = employee.employee_no.trim();
            let digits = employee_no
                .strip_prefix('A')
                .or_else(|| employee_no.strip_prefix('a'))?;
            digits.parse::<u32>().ok()
        })
        .max()
        .unwrap_or(0);
    format!("A{:03}", max_no + 1)
}

pub fn export_employee_template(path: &str, employees: &[Employee]) -> AppResult<()> {
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet.set_name("员工导入模板")?;

    let headers = vec![
        "工号",
        "姓名",
        "部门",
        "职位",
        "身份证号",
        "手机号",
        "邮箱(email)",
        "银行账号",
        "开户行",
        "入职日期",
        "基本工资",
        "岗位工资",
        "绩效工资",
        "社保基数",
        "公积金基数",
        "专项附加扣除",
        "备注",
    ];
    write_header_row(worksheet, &headers, 0)?;

    let cell_fmt = make_cell_format();
    let money_fmt = make_money_format();
    worksheet.write_string_with_format(1, 0, &next_employee_no(employees), &cell_fmt)?;
    worksheet.write_string_with_format(1, 1, "张三", &cell_fmt)?;
    worksheet.write_string_with_format(1, 2, "生产部", &cell_fmt)?;
    worksheet.write_string_with_format(1, 3, "操作员", &cell_fmt)?;
    worksheet.write_string_with_format(1, 4, "", &cell_fmt)?;
    worksheet.write_string_with_format(1, 5, "1XXXXXXXXXX", &cell_fmt)?;
    worksheet.write_string_with_format(1, 6, "zhangsan@example.com", &cell_fmt)?;
    worksheet.write_string_with_format(1, 7, "", &cell_fmt)?;
    worksheet.write_string_with_format(1, 8, "", &cell_fmt)?;
    worksheet.write_string_with_format(1, 9, "2026-01-01", &cell_fmt)?;
    worksheet.write_number_with_format(1, 10, 5000.0, &money_fmt)?;
    worksheet.write_number_with_format(1, 11, 1000.0, &money_fmt)?;
    worksheet.write_number_with_format(1, 12, 800.0, &money_fmt)?;
    worksheet.write_number_with_format(1, 13, 5000.0, &money_fmt)?;
    worksheet.write_number_with_format(1, 14, 5000.0, &money_fmt)?;
    worksheet.write_number_with_format(1, 15, 0.0, &money_fmt)?;
    worksheet.write_string_with_format(1, 16, "示例行，可删除；工号不可重复", &cell_fmt)?;

    let widths = [
        12, 10, 12, 12, 20, 14, 24, 22, 18, 12, 12, 12, 12, 12, 12, 14, 20,
    ];
    for (col, w) in widths.iter().enumerate() {
        worksheet.set_column_width(col as u16, *w)?;
    }

    workbook.save(path)?;
    Ok(())
}

pub fn export_attendance_template(path: &str, employees: &[Employee]) -> AppResult<()> {
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet.set_name("考勤导入模板")?;

    let headers = vec![
        "工号",
        "姓名",
        "应出勤天数",
        "实出勤天数",
        "迟到次数",
        "早退次数",
        "事假天数",
        "病假天数",
        "旷工天数",
        "加班小时",
        "备注",
    ];
    write_header_row(worksheet, &headers, 0)?;

    let cell_fmt = make_cell_format();
    let num_fmt = Format::new()
        .set_border(rust_xlsxwriter::FormatBorder::Thin)
        .set_align(rust_xlsxwriter::FormatAlign::Center)
        .set_num_format("0.0");
    let active_employees: Vec<&Employee> = employees
        .iter()
        .filter(|employee| employee.status == "active" || employee.status == "probation")
        .collect();
    let template_rows: Vec<(String, String, String)> = if active_employees.is_empty() {
        vec![(
            "A001".to_string(),
            "张三".to_string(),
            "示例行，可删除".to_string(),
        )]
    } else {
        active_employees
            .iter()
            .map(|employee| {
                (
                    employee.employee_no.clone(),
                    employee.name.clone(),
                    String::new(),
                )
            })
            .collect()
    };

    for (idx, (employee_no, name, remark)) in template_rows.iter().enumerate() {
        let row = idx as u32 + 1;
        worksheet.write_string_with_format(row, 0, employee_no, &cell_fmt)?;
        worksheet.write_string_with_format(row, 1, name, &cell_fmt)?;
        worksheet.write_number_with_format(row, 2, 22.0, &num_fmt)?;
        worksheet.write_number_with_format(row, 3, 22.0, &num_fmt)?;
        worksheet.write_number_with_format(row, 4, 0.0, &cell_fmt)?;
        worksheet.write_number_with_format(row, 5, 0.0, &cell_fmt)?;
        worksheet.write_number_with_format(row, 6, 0.0, &num_fmt)?;
        worksheet.write_number_with_format(row, 7, 0.0, &num_fmt)?;
        worksheet.write_number_with_format(row, 8, 0.0, &num_fmt)?;
        worksheet.write_number_with_format(row, 9, 0.0, &num_fmt)?;
        worksheet.write_string_with_format(row, 10, remark, &cell_fmt)?;
    }

    let widths = [12, 10, 12, 12, 10, 10, 10, 10, 10, 10, 20];
    for (col, w) in widths.iter().enumerate() {
        worksheet.set_column_width(col as u16, *w)?;
    }

    workbook.save(path)?;
    Ok(())
}

// ==================== Salary Detail Export ====================

pub fn export_salary_excel(results: &[SalaryResult], path: &str) -> AppResult<()> {
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet.set_name("工资明细")?;

    let headers = vec![
        "序号",
        "工号",
        "姓名",
        "部门",
        "基本工资",
        "岗位工资",
        "绩效工资",
        "加班工资",
        "餐补",
        "交通补助",
        "其他补助",
        "应发合计",
        "社保个人",
        "公积金个人",
        "考勤扣款",
        "个人所得税",
        "其他扣款",
        "实发工资",
        "状态",
        "备注",
    ];

    write_header_row(worksheet, &headers, 0)?;

    let cell_fmt = make_cell_format();
    let money_fmt = make_money_format();

    for (i, r) in results.iter().enumerate() {
        let row = (i + 1) as u32;
        worksheet.write_number_with_format(row, 0, (i + 1) as f64, &cell_fmt)?;
        worksheet.write_string_with_format(row, 1, &r.employee_no, &cell_fmt)?;
        worksheet.write_string_with_format(row, 2, r.name.as_deref().unwrap_or(""), &cell_fmt)?;
        worksheet.write_string_with_format(
            row,
            3,
            r.department.as_deref().unwrap_or(""),
            &cell_fmt,
        )?;
        worksheet.write_number_with_format(row, 4, r.base_salary, &money_fmt)?;
        worksheet.write_number_with_format(row, 5, r.position_salary, &money_fmt)?;
        worksheet.write_number_with_format(row, 6, r.performance_salary, &money_fmt)?;
        worksheet.write_number_with_format(row, 7, r.overtime_salary, &money_fmt)?;
        worksheet.write_number_with_format(row, 8, r.meal_allowance, &money_fmt)?;
        worksheet.write_number_with_format(row, 9, r.transport_allowance, &money_fmt)?;
        worksheet.write_number_with_format(row, 10, r.other_allowance, &money_fmt)?;
        worksheet.write_number_with_format(row, 11, r.gross_salary, &money_fmt)?;
        worksheet.write_number_with_format(row, 12, r.social_security_personal, &money_fmt)?;
        worksheet.write_number_with_format(row, 13, r.housing_fund_personal, &money_fmt)?;
        worksheet.write_number_with_format(row, 14, r.attendance_deduction, &money_fmt)?;
        worksheet.write_number_with_format(row, 15, r.tax_amount, &money_fmt)?;
        worksheet.write_number_with_format(row, 16, r.other_deduction, &money_fmt)?;
        worksheet.write_number_with_format(row, 17, r.net_salary, &money_fmt)?;
        worksheet.write_string_with_format(row, 18, &r.status, &cell_fmt)?;
        worksheet.write_string_with_format(
            row,
            19,
            r.remark.as_deref().unwrap_or(""),
            &cell_fmt,
        )?;
    }

    // Summary row
    if !results.is_empty() {
        let summary_row = (results.len() + 1) as u32;
        let bold_fmt = Format::new()
            .set_bold()
            .set_border(rust_xlsxwriter::FormatBorder::Thin);
        worksheet.write_string_with_format(summary_row, 0, "合计", &bold_fmt)?;
        let sum_col = |col: u16| -> f64 {
            results
                .iter()
                .map(|r| match col {
                    4 => r.base_salary,
                    5 => r.position_salary,
                    6 => r.performance_salary,
                    7 => r.overtime_salary,
                    8 => r.meal_allowance,
                    9 => r.transport_allowance,
                    10 => r.other_allowance,
                    11 => r.gross_salary,
                    12 => r.social_security_personal,
                    13 => r.housing_fund_personal,
                    14 => r.attendance_deduction,
                    15 => r.tax_amount,
                    16 => r.other_deduction,
                    17 => r.net_salary,
                    _ => 0.0,
                })
                .sum()
        };
        for col in [4u16, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17] {
            let sum_money_fmt = Format::new()
                .set_bold()
                .set_border(rust_xlsxwriter::FormatBorder::Thin)
                .set_num_format("#,##0.00");
            worksheet.write_number_with_format(summary_row, col, sum_col(col), &sum_money_fmt)?;
        }
    }

    // Set column widths
    let widths = [
        6, 12, 10, 12, 12, 12, 12, 12, 10, 10, 10, 12, 12, 12, 12, 12, 12, 12, 8, 20,
    ];
    for (col, w) in widths.iter().enumerate() {
        worksheet.set_column_width(col as u16, *w)?;
    }

    workbook.save(path)?;
    Ok(())
}

// ==================== Bank Payment Export ====================

pub fn export_bank_payment(results: &[SalaryResult], path: &str) -> AppResult<()> {
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet.set_name("银行代发")?;

    let headers = vec![
        "序号",
        "姓名",
        "银行账号",
        "开户行",
        "实发金额",
        "工号",
        "备注",
    ];

    write_header_row(worksheet, &headers, 0)?;

    let cell_fmt = make_cell_format();
    let money_fmt = make_money_format();

    for (i, r) in results.iter().enumerate() {
        let row = (i + 1) as u32;
        worksheet.write_number_with_format(row, 0, (i + 1) as f64, &cell_fmt)?;
        worksheet.write_string_with_format(row, 1, r.name.as_deref().unwrap_or(""), &cell_fmt)?;
        worksheet.write_string_with_format(row, 2, "", &cell_fmt)?; // bank_account from employee, but we have salary result
        worksheet.write_string_with_format(row, 3, "", &cell_fmt)?;
        worksheet.write_number_with_format(row, 4, r.net_salary, &money_fmt)?;
        worksheet.write_string_with_format(row, 5, &r.employee_no, &cell_fmt)?;
        worksheet.write_string_with_format(row, 6, r.remark.as_deref().unwrap_or(""), &cell_fmt)?;
    }

    let widths = [6, 12, 22, 20, 14, 12, 20];
    for (col, w) in widths.iter().enumerate() {
        worksheet.set_column_width(col as u16, *w)?;
    }

    workbook.save(path)?;
    Ok(())
}

// ==================== Salary Slip Export ====================

pub fn export_salary_slip(result: &SalaryResult, path: &str) -> AppResult<()> {
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet.set_name("工资条")?;

    let title_fmt = Format::new()
        .set_bold()
        .set_font_size(16)
        .set_align(rust_xlsxwriter::FormatAlign::Center);
    let label_fmt = Format::new()
        .set_bold()
        .set_border(rust_xlsxwriter::FormatBorder::Thin)
        .set_background_color("#D9E2F3");
    let value_fmt = Format::new().set_border(rust_xlsxwriter::FormatBorder::Thin);
    let money_fmt = Format::new()
        .set_border(rust_xlsxwriter::FormatBorder::Thin)
        .set_num_format("#,##0.00");

    let month = &result.salary_month;
    worksheet.merge_range(0, 0, 0, 3, &format!("{month} 工资条"), &title_fmt)?;

    let name_str = result.name.as_deref().unwrap_or("");
    let dept_str = result.department.as_deref().unwrap_or("");

    struct SlipRow {
        label: &'static str,
        value: String,
        is_money: bool,
    }

    let rows = vec![
        SlipRow {
            label: "工号",
            value: result.employee_no.clone(),
            is_money: false,
        },
        SlipRow {
            label: "姓名",
            value: name_str.to_string(),
            is_money: false,
        },
        SlipRow {
            label: "部门",
            value: dept_str.to_string(),
            is_money: false,
        },
        SlipRow {
            label: "",
            value: String::new(),
            is_money: false,
        },
        SlipRow {
            label: "基本工资",
            value: format!("{:.2}", result.base_salary),
            is_money: true,
        },
        SlipRow {
            label: "岗位工资",
            value: format!("{:.2}", result.position_salary),
            is_money: true,
        },
        SlipRow {
            label: "绩效工资",
            value: format!("{:.2}", result.performance_salary),
            is_money: true,
        },
        SlipRow {
            label: "加班工资",
            value: format!("{:.2}", result.overtime_salary),
            is_money: true,
        },
        SlipRow {
            label: "餐补",
            value: format!("{:.2}", result.meal_allowance),
            is_money: true,
        },
        SlipRow {
            label: "交通补助",
            value: format!("{:.2}", result.transport_allowance),
            is_money: true,
        },
        SlipRow {
            label: "其他补助",
            value: format!("{:.2}", result.other_allowance),
            is_money: true,
        },
        SlipRow {
            label: "",
            value: String::new(),
            is_money: false,
        },
        SlipRow {
            label: "应发合计",
            value: format!("{:.2}", result.gross_salary),
            is_money: true,
        },
        SlipRow {
            label: "",
            value: String::new(),
            is_money: false,
        },
        SlipRow {
            label: "社保个人扣款",
            value: format!("{:.2}", result.social_security_personal),
            is_money: true,
        },
        SlipRow {
            label: "公积金个人扣款",
            value: format!("{:.2}", result.housing_fund_personal),
            is_money: true,
        },
        SlipRow {
            label: "考勤扣款",
            value: format!("{:.2}", result.attendance_deduction),
            is_money: true,
        },
        SlipRow {
            label: "个人所得税",
            value: format!("{:.2}", result.tax_amount),
            is_money: true,
        },
        SlipRow {
            label: "其他扣款",
            value: format!("{:.2}", result.other_deduction),
            is_money: true,
        },
        SlipRow {
            label: "",
            value: String::new(),
            is_money: false,
        },
        SlipRow {
            label: "实发工资",
            value: format!("{:.2}", result.net_salary),
            is_money: true,
        },
    ];

    for (i, row_data) in rows.iter().enumerate() {
        let row = (i + 2) as u32;
        worksheet.write_string_with_format(row, 0, row_data.label, &label_fmt)?;
        worksheet.merge_range(row, 1, row, 3, "", &value_fmt)?;
        if row_data.is_money {
            let val: f64 = row_data.value.parse().unwrap_or(0.0);
            worksheet.write_number_with_format(row, 1, val, &money_fmt)?;
        } else {
            worksheet.write_string_with_format(row, 1, &row_data.value, &value_fmt)?;
        }
    }

    worksheet.set_column_width(0, 16)?;
    worksheet.set_column_width(1, 20)?;
    worksheet.set_column_width(2, 12)?;
    worksheet.set_column_width(3, 12)?;

    workbook.save(path)?;
    Ok(())
}

// ==================== Attendance Summary Export ====================

pub fn export_attendance_summary(records: &[AttendanceRecord], path: &str) -> AppResult<()> {
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet.set_name("考勤汇总")?;

    let headers = vec![
        "序号",
        "工号",
        "姓名",
        "月份",
        "应出勤天数",
        "实出勤天数",
        "迟到次数",
        "早退次数",
        "事假天数",
        "病假天数",
        "旷工天数",
        "加班小时",
        "数据来源",
        "备注",
    ];

    write_header_row(worksheet, &headers, 0)?;

    let cell_fmt = make_cell_format();
    let num_fmt = Format::new()
        .set_border(rust_xlsxwriter::FormatBorder::Thin)
        .set_align(rust_xlsxwriter::FormatAlign::Center)
        .set_num_format("0.0");

    for (i, r) in records.iter().enumerate() {
        let row = (i + 1) as u32;
        worksheet.write_number_with_format(row, 0, (i + 1) as f64, &cell_fmt)?;
        worksheet.write_string_with_format(row, 1, &r.employee_no, &cell_fmt)?;
        worksheet.write_string_with_format(row, 2, r.name.as_deref().unwrap_or(""), &cell_fmt)?;
        worksheet.write_string_with_format(row, 3, &r.salary_month, &cell_fmt)?;
        worksheet.write_number_with_format(row, 4, r.expected_days, &num_fmt)?;
        worksheet.write_number_with_format(row, 5, r.actual_days, &num_fmt)?;
        worksheet.write_number_with_format(row, 6, r.late_count as f64, &cell_fmt)?;
        worksheet.write_number_with_format(row, 7, r.early_leave_count as f64, &cell_fmt)?;
        worksheet.write_number_with_format(row, 8, r.personal_leave_days, &num_fmt)?;
        worksheet.write_number_with_format(row, 9, r.sick_leave_days, &num_fmt)?;
        worksheet.write_number_with_format(row, 10, r.absent_days, &num_fmt)?;
        worksheet.write_number_with_format(row, 11, r.overtime_hours, &num_fmt)?;
        worksheet.write_string_with_format(
            row,
            12,
            r.source_type.as_deref().unwrap_or(""),
            &cell_fmt,
        )?;
        worksheet.write_string_with_format(
            row,
            13,
            r.remark.as_deref().unwrap_or(""),
            &cell_fmt,
        )?;
    }

    let widths = [6, 12, 10, 10, 12, 12, 10, 10, 10, 10, 10, 10, 12, 20];
    for (col, w) in widths.iter().enumerate() {
        worksheet.set_column_width(col as u16, *w)?;
    }

    workbook.save(path)?;
    Ok(())
}

// ==================== Punch Card Template ====================

/// 2026 Chinese public holidays (month, day ranges)
pub const HOLIDAYS_2026: &[(u32, u32, u32, &str)] = &[
    (1, 1, 1, "元旦"),
    (2, 17, 19, "春节"), // Spring Festival approx
    (4, 4, 6, "清明"),
    (5, 1, 3, "劳动节"),
    (5, 31, 6, "端午"), // spans month boundary
    (9, 25, 27, "中秋"),
    (10, 1, 7, "国庆"),
];

fn is_holiday(month: u32, day: u32) -> Option<&'static str> {
    for &(h_mon, h_start, h_end, name) in HOLIDAYS_2026 {
        if h_mon == month && day >= h_start && day <= h_end {
            return Some(name);
        }
        // Handle holidays spanning month boundary (e.g., 5/31 - 6/2)
        if h_start > h_end && h_mon == month && day >= h_start {
            return Some(name);
        }
    }
    None
}

pub fn export_punch_card_template(
    path: &str,
    month: &str,
    _department: &str,
    _shift_type: &str,
    employees: &[Employee],
) -> AppResult<()> {
    let mut workbook = Workbook::new();
    let ws = workbook.add_worksheet();

    let title_fmt = Format::new()
        .set_bold()
        .set_font_size(14)
        .set_align(rust_xlsxwriter::FormatAlign::Center);
    let info_fmt = Format::new().set_font_size(10);
    let header_fmt = Format::new()
        .set_bold()
        .set_font_size(9)
        .set_background_color("D9E1F2")
        .set_align(rust_xlsxwriter::FormatAlign::Center);
    let cell_fmt = Format::new()
        .set_font_size(9)
        .set_align(rust_xlsxwriter::FormatAlign::Center);
    let holiday_fmt = Format::new()
        .set_font_size(7)
        .set_font_color("FF0000")
        .set_align(rust_xlsxwriter::FormatAlign::Center);

    let days_in_month = get_days_in_month(month);
    let month_parts: Vec<&str> = month.split('-').collect();
    let mon: u32 = month_parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
    let year: u32 = month_parts
        .first()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2026);

    // Display month name (e.g., "2026年5月")
    let month_label = format!("{}年{}月", year, mon);

    // Column layout: 序号(Col 0), 姓名(Col 1), then day*2 cols (白/夜 per day), then 合计白(Col -4), 合计夜(Col -3), 合计(Col -2), 备注(Col -1)
    let day_col_start: u16 = 2; // first day sub-column
    let summary_col_day: u16 = day_col_start + (days_in_month * 2) as u16;
    let summary_col_night: u16 = summary_col_day + 1;
    let summary_col_total: u16 = summary_col_day + 2;
    let remark_col: u16 = summary_col_day + 3;

    // Row 0: Title (merged across all columns)
    ws.merge_range(
        0,
        0,
        0,
        remark_col,
        &format!("{}员工考勤汇总表", month_label),
        &title_fmt,
    )?;

    // Row 1: Department info
    ws.write_string_with_format(1, 0, &format!("部门: {}", _department), &info_fmt)?;

    // Row 2-3: Headers
    ws.merge_range(2, 0, 3, 0, "序号", &header_fmt)?;
    ws.merge_range(2, 1, 3, 1, "姓名", &header_fmt)?;

    for day in 1..=days_in_month {
        let col = day_col_start + ((day - 1) * 2) as u16;
        ws.merge_range(2, col, 2, col + 1, &day.to_string(), &header_fmt)?;
    }

    // Summary headers (merged row 2-3)
    ws.merge_range(
        2,
        summary_col_day,
        3,
        summary_col_day,
        "白班合计",
        &header_fmt,
    )?;
    ws.merge_range(
        2,
        summary_col_night,
        3,
        summary_col_night,
        "夜班合计",
        &header_fmt,
    )?;
    ws.merge_range(
        2,
        summary_col_total,
        3,
        summary_col_total,
        "合计",
        &header_fmt,
    )?;
    ws.merge_range(2, remark_col, 3, remark_col, "备注", &header_fmt)?;

    // Row 3: Shift sub-headers (白/夜 per day)
    for day in 1..=days_in_month {
        let col = day_col_start + ((day - 1) * 2) as u16;
        ws.write_string_with_format(3, col, "白", &header_fmt)?;
        ws.write_string_with_format(3, col + 1, "夜", &header_fmt)?;
    }

    // Data rows: 2 rows per employee
    for (i, emp) in employees.iter().enumerate() {
        let base_row: u32 = (4 + i * 2) as u32;
        let day_row = base_row;
        let night_row = base_row + 1;

        // Seq number (merged 2 rows)
        ws.merge_range(day_row, 0, night_row, 0, &(i + 1).to_string(), &cell_fmt)?;

        // Name (merged 2 rows)
        ws.merge_range(day_row, 1, night_row, 1, &emp.name, &cell_fmt)?;

        // Pre-fill holidays in both day and night rows
        for day in 1..=days_in_month {
            let col = day_col_start + ((day - 1) * 2) as u16;
            if let Some(holiday_name) = is_holiday(mon, day) {
                ws.write_string_with_format(day_row, col, holiday_name, &holiday_fmt)?;
                ws.write_string_with_format(night_row, col + 1, holiday_name, &holiday_fmt)?;
            }
        }

        // Summary formulas
        // White shift count = COUNTIF of √ in day columns
        let day_col_letter_start = col_letter(day_col_start);
        let day_col_letter_end = col_letter(day_col_start + ((days_in_month - 1) * 2) as u16);
        let night_col_letter_start = col_letter(day_col_start + 1);
        let night_col_letter_end = col_letter(day_col_start + ((days_in_month - 1) * 2 + 1) as u16);

        ws.write_string_with_format(
            day_row,
            summary_col_day,
            &format!(
                "=COUNTIF({day_col_letter_start}{day_row}:{day_col_letter_end}{day_row},\"√\")"
            ),
            &cell_fmt,
        )?;
        ws.write_string_with_format(night_row, summary_col_night, &format!("=COUNTIF({night_col_letter_start}{night_row}:{night_col_letter_end}{night_row},\"√\")"), &cell_fmt)?;
        let day_total_col = col_letter(summary_col_day);
        let night_total_col = col_letter(summary_col_night);
        ws.write_string_with_format(
            day_row,
            summary_col_total,
            &format!("={day_total_col}{day_row}+{night_total_col}{night_row}"),
            &cell_fmt,
        )?;

        // Remark (merged 2 rows)
        ws.merge_range(day_row, remark_col, night_row, remark_col, "", &cell_fmt)?;
    }

    // Bottom legend row
    let legend_row = (4 + employees.len() * 2 + 1) as u32;
    ws.write_string_with_format(legend_row, 0, "标注:", &info_fmt)?;
    ws.write_string_with_format(
        legend_row,
        1,
        "√=出勤  休=公休  S(+时数)=事假  病=病假",
        &info_fmt,
    )?;

    // Signature area
    let sign_row = legend_row + 1;
    ws.write_string_with_format(sign_row, 0, "考勤人签字:", &info_fmt)?;
    ws.write_string_with_format(sign_row, 5, "行政经理签字:", &info_fmt)?;
    ws.write_string_with_format(sign_row, 10, "日期:", &info_fmt)?;

    // Column widths
    ws.set_column_width(0, 5)?; // 序号
    ws.set_column_width(1, 10)?; // 姓名
    for day in 0..days_in_month * 2 {
        ws.set_column_width(day_col_start + day as u16, 4)?;
    }
    ws.set_column_width(summary_col_day, 6)?;
    ws.set_column_width(summary_col_night, 6)?;
    ws.set_column_width(summary_col_total, 6)?;
    ws.set_column_width(remark_col, 12)?;

    // Print settings
    ws.set_print_scale(60);

    workbook.save(path)?;
    Ok(())
}

/// Convert a 0-based column index to Excel column letter(s) (A, B, ..., Z, AA, AB, ...)
fn col_letter(col: u16) -> String {
    let mut result = String::new();
    let mut c = col;
    loop {
        result.insert(0, (b'A' + (c % 26) as u8) as char);
        if c < 26 {
            break;
        }
        c = c / 26 - 1;
    }
    result
}

fn get_days_in_month(month: &str) -> u32 {
    let parts: Vec<&str> = month.split('-').collect();
    if parts.len() != 2 {
        return 31;
    }
    let year: u32 = parts[0].parse().unwrap_or(2026);
    let mon: u32 = parts[1].parse().unwrap_or(1);
    match mon {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0) {
                29
            } else {
                28
            }
        }
        _ => 31,
    }
}

// ==================== Invoice List Export ====================

pub fn export_invoice_list(invoices: &[Invoice], path: &str) -> AppResult<bool> {
    use rust_xlsxwriter::FormatBorder;

    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet.set_name("发票清单")?;

    let header_fmt = Format::new()
        .set_bold()
        .set_background_color("#D9E1F2")
        .set_border(FormatBorder::Thin);

    let headers = [
        "归属月份",
        "报销人ID",
        "发票类型",
        "发票代码",
        "发票号码",
        "开票日期",
        "金额",
        "税额",
        "价税合计",
        "销售方",
        "销售方税号",
        "购买方",
        "购买方税号",
        "费用类型",
        "状态",
        "备注",
        "录入时间",
    ];
    for (col, h) in headers.iter().enumerate() {
        worksheet.write_with_format(0, col as u16, *h, &header_fmt)?;
    }

    for (row_idx, inv) in invoices.iter().enumerate() {
        let row = (row_idx + 1) as u32;
        worksheet.write_string(row, 0, inv.belong_month.clone().unwrap_or_default())?;
        worksheet.write_number(row, 1, inv.employee_id.unwrap_or(0) as f64)?;
        worksheet.write_string(row, 2, inv.invoice_type.clone().unwrap_or_default())?;
        worksheet.write_string(row, 3, inv.invoice_code.clone().unwrap_or_default())?;
        worksheet.write_string(row, 4, inv.invoice_number.clone().unwrap_or_default())?;
        worksheet.write_string(row, 5, inv.issue_date.clone().unwrap_or_default())?;
        worksheet.write_number(row, 6, inv.amount)?;
        worksheet.write_number(row, 7, inv.tax_amount)?;
        worksheet.write_number(row, 8, inv.total_amount)?;
        worksheet.write_string(row, 9, inv.seller_name.clone().unwrap_or_default())?;
        worksheet.write_string(row, 10, inv.seller_tax_id.clone().unwrap_or_default())?;
        worksheet.write_string(row, 11, inv.buyer_name.clone().unwrap_or_default())?;
        worksheet.write_string(row, 12, inv.buyer_tax_id.clone().unwrap_or_default())?;
        worksheet.write_string(row, 13, inv.expense_type_code.clone().unwrap_or_default())?;
        worksheet.write_string(row, 14, inv.status.clone().unwrap_or_default())?;
        worksheet.write_string(row, 15, inv.remark.clone().unwrap_or_default())?;
        worksheet.write_string(row, 16, inv.created_at.clone().unwrap_or_default())?;
    }

    worksheet.set_column_width(0, 10)?;
    worksheet.set_column_width(3, 16)?;
    worksheet.set_column_width(4, 12)?;
    worksheet.set_column_width(9, 30)?;
    worksheet.set_column_width(11, 30)?;
    worksheet.set_column_width(16, 22)?;

    workbook.save(path)?;
    Ok(true)
}

pub fn export_reimbursement_claim_list(
    claims: &[ReimbursementClaim],
    path: &str,
) -> AppResult<bool> {
    use rust_xlsxwriter::FormatBorder;

    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet.set_name("报销清单")?;

    let header_fmt = Format::new()
        .set_bold()
        .set_background_color("#D9E1F2")
        .set_border(FormatBorder::Thin);
    let money_fmt = report_money_format();

    let headers = [
        "归属月份",
        "报销单号",
        "报销人",
        "部门",
        "标题",
        "发票张数",
        "报销金额",
        "审批状态",
        "付款状态",
        "付款日期",
        "备注",
        "更新时间",
    ];
    for (col, h) in headers.iter().enumerate() {
        worksheet.write_with_format(0, col as u16, *h, &header_fmt)?;
    }

    for (row_idx, claim) in claims.iter().enumerate() {
        let row = (row_idx + 1) as u32;
        worksheet.write_string(row, 0, &claim.belong_month)?;
        worksheet.write_string(row, 1, &claim.claim_no)?;
        worksheet.write_string(row, 2, claim.employee_name.clone().unwrap_or_default())?;
        worksheet.write_string(row, 3, claim.department.clone().unwrap_or_default())?;
        worksheet.write_string(row, 4, &claim.title)?;
        worksheet.write_number(row, 5, claim.invoice_count as f64)?;
        worksheet.write_number_with_format(row, 6, claim.total_amount, &money_fmt)?;
        worksheet.write_string(row, 7, reimbursement_status_text(&claim.status))?;
        worksheet.write_string(row, 8, payment_status_text(&claim.payment_status))?;
        worksheet.write_string(row, 9, claim.payment_date.clone().unwrap_or_default())?;
        worksheet.write_string(row, 10, claim.remark.clone().unwrap_or_default())?;
        worksheet.write_string(row, 11, claim.updated_at.clone().unwrap_or_default())?;
    }

    worksheet.set_column_width(0, 10)?;
    worksheet.set_column_width(1, 20)?;
    worksheet.set_column_width(2, 12)?;
    worksheet.set_column_width(3, 14)?;
    worksheet.set_column_width(4, 28)?;
    worksheet.set_column_width(6, 14)?;
    worksheet.set_column_width(10, 24)?;
    worksheet.set_column_width(11, 22)?;

    workbook.save(path)?;
    Ok(true)
}

pub fn export_payment_batch(detail: &PaymentBatchDetail, path: &str) -> AppResult<bool> {
    if detail.items.is_empty() {
        return Err(AppError::InvalidParam("付款批次没有明细，不能导出".into()));
    }

    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet.set_name("付款明细")?;

    let header_fmt = report_header_format();
    let money_fmt = report_money_format();
    let batch_type_text = match detail.batch.batch_type.as_str() {
        "salary" => "工资",
        "reimbursement" => "报销",
        "general" => "通用",
        _ => "未知",
    };

    worksheet.write_string(0, 0, format!("付款批次：{}", detail.batch.batch_no))?;
    worksheet.write_string(1, 0, format!("类型：{batch_type_text}"))?;
    worksheet.write_string(1, 2, format!("月份：{}", detail.batch.belong_month))?;
    worksheet.write_number_with_format(1, 4, detail.batch.total_amount, &money_fmt)?;
    worksheet.write_string(
        2,
        0,
        format!(
            "付款账户：{}",
            detail
                .batch
                .fund_account_name
                .as_deref()
                .unwrap_or("历史批次未指定")
        ),
    )?;

    let headers = [
        "序号",
        "收款人",
        "银行账号",
        "开户行",
        "付款金额",
        "来源类型",
        "来源ID",
        "工号",
        "备注",
    ];
    for (col, header) in headers.iter().enumerate() {
        worksheet.write_with_format(3, col as u16, *header, &header_fmt)?;
    }

    for (idx, item) in detail.items.iter().enumerate() {
        let row = (idx + 4) as u32;
        worksheet.write_number(row, 0, (idx + 1) as f64)?;
        worksheet.write_string(row, 1, item.employee_name.clone().unwrap_or_default())?;
        worksheet.write_string(row, 2, item.bank_account.clone().unwrap_or_default())?;
        worksheet.write_string(row, 3, item.bank_name.clone().unwrap_or_default())?;
        worksheet.write_number_with_format(row, 4, item.amount, &money_fmt)?;
        worksheet.write_string(row, 5, payment_source_text(&item.source_type))?;
        worksheet.write_number(row, 6, item.source_id as f64)?;
        worksheet.write_string(row, 7, item.employee_no.clone().unwrap_or_default())?;
        worksheet.write_string(row, 8, item.remark.clone().unwrap_or_default())?;
    }

    let widths = [6, 12, 24, 22, 14, 12, 10, 12, 24];
    for (col, width) in widths.iter().enumerate() {
        worksheet.set_column_width(col as u16, *width)?;
    }

    workbook.save(path)?;
    Ok(true)
}

// ==================== Financial Analysis Export ====================

pub fn export_department_cost_analysis(
    rows: &[DepartmentCostAnalysis],
    month: &str,
    path: &str,
) -> AppResult<bool> {
    use rust_xlsxwriter::FormatBorder;

    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet.set_name("部门成本表")?;
    let header_fmt = report_header_format();
    let money_fmt = report_money_format();

    worksheet.write_string(0, 0, format!("{month} 部门成本分析"))?;
    let headers = [
        "部门",
        "人数",
        "应发工资",
        "社保",
        "公积金",
        "工资成本",
        "发票费用",
        "报销金额",
        "总成本",
    ];
    for (col, header) in headers.iter().enumerate() {
        worksheet.write_with_format(2, col as u16, *header, &header_fmt)?;
    }
    for (idx, row_data) in rows.iter().enumerate() {
        let row = (idx + 3) as u32;
        worksheet.write_string(row, 0, &row_data.department)?;
        worksheet.write_number(row, 1, row_data.employee_count as f64)?;
        worksheet.write_number_with_format(row, 2, row_data.gross_salary, &money_fmt)?;
        worksheet.write_number_with_format(row, 3, row_data.social_security, &money_fmt)?;
        worksheet.write_number_with_format(row, 4, row_data.housing_fund, &money_fmt)?;
        worksheet.write_number_with_format(row, 5, row_data.salary_cost, &money_fmt)?;
        worksheet.write_number_with_format(row, 6, row_data.invoice_amount, &money_fmt)?;
        worksheet.write_number_with_format(row, 7, row_data.reimbursement_amount, &money_fmt)?;
        worksheet.write_number_with_format(row, 8, row_data.total_cost, &money_fmt)?;
    }
    set_report_columns(worksheet, headers.len() as u16)?;

    workbook.save(path)?;
    let _ = FormatBorder::Thin;
    Ok(true)
}

pub fn export_expense_analysis_report(
    report: &FinancialAnalysisReport,
    path: &str,
) -> AppResult<bool> {
    let mut workbook = Workbook::new();
    let header_fmt = report_header_format();
    let money_fmt = report_money_format();

    {
        let worksheet = workbook.add_worksheet();
        worksheet.set_name("费用类型趋势")?;
        worksheet.write_string(
            0,
            0,
            format!("{} 最近{}个月费用类型趋势", report.month, report.months),
        )?;
        let headers = ["月份", "费用类型", "发票张数", "发票金额", "报销金额"];
        for (col, header) in headers.iter().enumerate() {
            worksheet.write_with_format(2, col as u16, *header, &header_fmt)?;
        }
        for (idx, row_data) in report.expense_trends.iter().enumerate() {
            let row = (idx + 3) as u32;
            worksheet.write_string(row, 0, &row_data.month)?;
            worksheet.write_string(row, 1, &row_data.expense_type_name)?;
            worksheet.write_number(row, 2, row_data.invoice_count as f64)?;
            worksheet.write_number_with_format(row, 3, row_data.invoice_amount, &money_fmt)?;
            worksheet.write_number_with_format(
                row,
                4,
                row_data.reimbursement_amount,
                &money_fmt,
            )?;
        }
        set_report_columns(worksheet, headers.len() as u16)?;
    }

    {
        let worksheet = workbook.add_worksheet();
        worksheet.set_name("员工成本")?;
        let headers = [
            "部门",
            "工号",
            "姓名",
            "应发工资",
            "实发工资",
            "社保",
            "公积金",
            "考勤扣款",
            "发票费用",
            "报销金额",
            "异常考勤",
            "总成本",
        ];
        for (col, header) in headers.iter().enumerate() {
            worksheet.write_with_format(0, col as u16, *header, &header_fmt)?;
        }
        for (idx, row_data) in report.employee_costs.iter().enumerate() {
            let row = (idx + 1) as u32;
            worksheet.write_string(row, 0, &row_data.department)?;
            worksheet.write_string(row, 1, &row_data.employee_no)?;
            worksheet.write_string(row, 2, &row_data.name)?;
            worksheet.write_number_with_format(row, 3, row_data.gross_salary, &money_fmt)?;
            worksheet.write_number_with_format(row, 4, row_data.net_salary, &money_fmt)?;
            worksheet.write_number_with_format(row, 5, row_data.social_security, &money_fmt)?;
            worksheet.write_number_with_format(row, 6, row_data.housing_fund, &money_fmt)?;
            worksheet.write_number_with_format(
                row,
                7,
                row_data.attendance_deduction,
                &money_fmt,
            )?;
            worksheet.write_number_with_format(row, 8, row_data.invoice_amount, &money_fmt)?;
            worksheet.write_number_with_format(
                row,
                9,
                row_data.reimbursement_amount,
                &money_fmt,
            )?;
            worksheet.write_number(row, 10, row_data.abnormal_attendance_count as f64)?;
            worksheet.write_number_with_format(row, 11, row_data.total_cost, &money_fmt)?;
        }
        set_report_columns(worksheet, headers.len() as u16)?;
    }

    workbook.save(path)?;
    Ok(true)
}

pub fn export_month_close_report(
    report: &FinancialAnalysisReport,
    workbench: &MonthCloseWorkbench,
    path: &str,
) -> AppResult<bool> {
    let mut workbook = Workbook::new();
    let header_fmt = report_header_format();
    let money_fmt = report_money_format();

    {
        let worksheet = workbook.add_worksheet();
        worksheet.set_name("月结概览")?;
        worksheet.write_string(0, 0, format!("{} 月结报告", report.month))?;
        let summary = &workbench.summary;
        let rows = [
            ("在职员工", summary.active_employee_count as f64),
            ("工资结果", summary.salary_count as f64),
            ("异常考勤", summary.abnormal_attendance_count as f64),
            ("发票张数", summary.invoice_count as f64),
            ("报销单数", summary.reimbursement_count as f64),
            ("工资应发合计", summary.total_salary_cost),
            ("发票价税合计", summary.total_invoice_amount),
            ("已批报销金额", summary.approved_reimbursement_amount),
            ("已付款报销金额", summary.paid_reimbursement_amount),
        ];
        worksheet.write_with_format(2, 0, "指标", &header_fmt)?;
        worksheet.write_with_format(2, 1, "数值", &header_fmt)?;
        for (idx, (label, value)) in rows.iter().enumerate() {
            let row = (idx + 3) as u32;
            worksheet.write_string(row, 0, *label)?;
            worksheet.write_number_with_format(row, 1, *value, &money_fmt)?;
        }
        worksheet.set_column_width(0, 22)?;
        worksheet.set_column_width(1, 18)?;
    }

    {
        let worksheet = workbook.add_worksheet();
        worksheet.set_name("月结检查")?;
        let headers = ["检查项", "状态", "数量", "说明"];
        for (col, header) in headers.iter().enumerate() {
            worksheet.write_with_format(0, col as u16, *header, &header_fmt)?;
        }
        for (idx, item) in workbench.checks.iter().enumerate() {
            let row = (idx + 1) as u32;
            worksheet.write_string(row, 0, &item.title)?;
            worksheet.write_string(row, 1, status_text(&item.status))?;
            worksheet.write_number(row, 2, item.count as f64)?;
            worksheet.write_string(row, 3, &item.description)?;
        }
        set_report_columns(worksheet, headers.len() as u16)?;
    }

    {
        let worksheet = workbook.add_worksheet();
        worksheet.set_name("月度对比")?;
        let headers = [
            "月份",
            "应发",
            "实发",
            "扣款",
            "社保",
            "公积金",
            "发票",
            "报销",
            "总成本",
        ];
        for (col, header) in headers.iter().enumerate() {
            worksheet.write_with_format(0, col as u16, *header, &header_fmt)?;
        }
        for (idx, row_data) in report.monthly_comparison.iter().enumerate() {
            let row = (idx + 1) as u32;
            worksheet.write_string(row, 0, &row_data.month)?;
            worksheet.write_number_with_format(row, 1, row_data.gross_salary, &money_fmt)?;
            worksheet.write_number_with_format(row, 2, row_data.net_salary, &money_fmt)?;
            worksheet.write_number_with_format(row, 3, row_data.deduction, &money_fmt)?;
            worksheet.write_number_with_format(row, 4, row_data.social_security, &money_fmt)?;
            worksheet.write_number_with_format(row, 5, row_data.housing_fund, &money_fmt)?;
            worksheet.write_number_with_format(row, 6, row_data.invoice_amount, &money_fmt)?;
            worksheet.write_number_with_format(
                row,
                7,
                row_data.reimbursement_amount,
                &money_fmt,
            )?;
            worksheet.write_number_with_format(row, 8, row_data.total_cost, &money_fmt)?;
        }
        set_report_columns(worksheet, headers.len() as u16)?;
    }

    {
        let worksheet = workbook.add_worksheet();
        worksheet.set_name("部门成本")?;
        let headers = ["部门", "人数", "工资成本", "发票费用", "报销金额", "总成本"];
        for (col, header) in headers.iter().enumerate() {
            worksheet.write_with_format(0, col as u16, *header, &header_fmt)?;
        }
        for (idx, row_data) in report.department_costs.iter().enumerate() {
            let row = (idx + 1) as u32;
            worksheet.write_string(row, 0, &row_data.department)?;
            worksheet.write_number(row, 1, row_data.employee_count as f64)?;
            worksheet.write_number_with_format(row, 2, row_data.salary_cost, &money_fmt)?;
            worksheet.write_number_with_format(row, 3, row_data.invoice_amount, &money_fmt)?;
            worksheet.write_number_with_format(
                row,
                4,
                row_data.reimbursement_amount,
                &money_fmt,
            )?;
            worksheet.write_number_with_format(row, 5, row_data.total_cost, &money_fmt)?;
        }
        set_report_columns(worksheet, headers.len() as u16)?;
    }

    workbook.save(path)?;
    Ok(true)
}

// ==================== Financial Statements Export（第五阶段 Task 11） ====================

/// 三大报表共用区段写入器：区段标题行、表头（项目/本期金额/对比金额/上年同期）、数据行、合计行。
/// 返回写入完成后下一个可用行号。
#[allow(clippy::too_many_arguments)]
fn write_statement_section(
    worksheet: &mut rust_xlsxwriter::Worksheet,
    start_row: u32,
    section: &str,
    headers: [&str; 4],
    rows: &[ReportRow],
    total_label: &str,
    total_current: f64,
    total_comparative: f64,
    total_prior: f64,
) -> AppResult<u32> {
    let header_fmt = report_header_format();
    let money_fmt = report_money_format();
    worksheet.write_string(start_row, 0, section)?;
    for (col, header) in headers.iter().enumerate() {
        worksheet.write_with_format(start_row + 1, col as u16, *header, &header_fmt)?;
    }
    let mut row = start_row + 2;
    for r in rows {
        worksheet.write_string(row, 0, &r.label)?;
        worksheet.write_number_with_format(row, 1, r.current, &money_fmt)?;
        worksheet.write_number_with_format(row, 2, r.comparative, &money_fmt)?;
        worksheet.write_number_with_format(row, 3, r.prior_year, &money_fmt)?;
        row += 1;
    }
    worksheet.write_string(row, 0, total_label)?;
    worksheet.write_number_with_format(row, 1, total_current, &money_fmt)?;
    worksheet.write_number_with_format(row, 2, total_comparative, &money_fmt)?;
    worksheet.write_number_with_format(row, 3, total_prior, &money_fmt)?;
    Ok(row + 1)
}

/// 资产负债表：资产 / 负债和所有者权益两个区段，各含期末余额与年初余额两列；
/// 不平衡时尾部红字提示。
pub fn export_balance_sheet(report: &BalanceSheet, path: &str) -> AppResult<bool> {
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet.set_name("资产负债表")?;
    worksheet.write_string(0, 0, format!("资产负债表 {}", report.month))?;
    let headers = ["项目", "期末余额", "年初余额", "上年年末"];
    let next = write_statement_section(
        worksheet,
        1,
        "资产",
        headers,
        &report.asset_rows,
        "资产总计",
        report.asset_total,
        report.asset_rows.iter().map(|r| r.comparative).sum(),
        report.asset_rows.iter().map(|r| r.prior_year).sum(),
    )?;
    write_statement_section(
        worksheet,
        next + 1,
        "负债和所有者权益",
        headers,
        &report.liability_equity_rows,
        "负债和所有者权益总计",
        report.liability_equity_total,
        report
            .liability_equity_rows
            .iter()
            .map(|r| r.comparative)
            .sum(),
        report
            .liability_equity_rows
            .iter()
            .map(|r| r.prior_year)
            .sum(),
    )?;
    if !report.balanced {
        worksheet.write_with_format(
            next + 1 + report.liability_equity_rows.len() as u32 + 4,
            0,
            "提示：资产合计与负债及所有者权益合计不平，请检查期初余额与凭证",
            &Format::new().set_font_color("#CC0000"),
        )?;
    }
    set_report_columns(worksheet, 3)?;
    workbook.save(path)?;
    Ok(true)
}

/// 利润表：固定标准行（含营业利润/利润总额/净利润计算行），本月金额与本年累计两列。
pub fn export_income_statement(report: &IncomeStatement, path: &str) -> AppResult<bool> {
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet.set_name("利润表")?;
    worksheet.write_string(0, 0, format!("利润表 {}", report.month))?;
    let header_fmt = report_header_format();
    let money_fmt = report_money_format();
    for (col, header) in ["项目", "本月金额", "本年累计", "上年同期"]
        .iter()
        .enumerate()
    {
        worksheet.write_with_format(1, col as u16, *header, &header_fmt)?;
    }
    let mut row = 2u32;
    for r in &report.rows {
        worksheet.write_string(row, 0, &r.label)?;
        worksheet.write_number_with_format(row, 1, r.current, &money_fmt)?;
        worksheet.write_number_with_format(row, 2, r.comparative, &money_fmt)?;
        worksheet.write_number_with_format(row, 3, r.prior_year, &money_fmt)?;
        row += 1;
    }
    set_report_columns(worksheet, 4)?;
    workbook.save(path)?;
    Ok(true)
}

/// 现金流量表：六行汇总 + 其他行 + 现金净增加额合计行；
/// 存在未分类现金收支时附加"未分类现金收支"明细 sheet。
pub fn export_cash_flow_statement(report: &CashFlowStatement, path: &str) -> AppResult<bool> {
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet.set_name("现金流量表")?;
    worksheet.write_string(0, 0, format!("现金流量表 {}", report.month))?;
    write_statement_section(
        worksheet,
        1,
        "现金流量",
        ["项目", "本期金额", "本年累计", "上年同期"],
        &report.rows,
        "现金净增加额",
        report.net_increase,
        report.rows.iter().map(|r| r.comparative).sum(),
        report.rows.iter().map(|r| r.prior_year).sum(),
    )?;
    set_report_columns(worksheet, 4)?;

    if !report.unclassified.is_empty() {
        let detail = workbook.add_worksheet();
        detail.set_name("未分类现金收支")?;
        let header_fmt = report_header_format();
        let money_fmt = report_money_format();
        detail.write_string(0, 0, format!("未分类现金收支明细 {}", report.month))?;
        for (col, header) in ["凭证号", "摘要", "金额"].iter().enumerate() {
            detail.write_with_format(1, col as u16, *header, &header_fmt)?;
        }
        for (idx, item) in report.unclassified.iter().enumerate() {
            let row = (idx + 2) as u32;
            detail.write_string(row, 0, &item.voucher_no)?;
            detail.write_string(row, 1, item.summary.as_deref().unwrap_or(""))?;
            detail.write_number_with_format(row, 2, item.amount, &money_fmt)?;
        }
        set_report_columns(detail, 3)?;
    }

    workbook.save(path)?;
    Ok(true)
}

/// 个税年度汇总表（第六阶段 Task 10）：按员工聚合年度累计收入/扣除/预扣与差额。
pub fn export_annual_tax_summary_excel(
    rows: &[AnnualTaxSummaryRow],
    year: i64,
    path: &str,
) -> AppResult<()> {
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.set_name("个税年度汇总")?;
    let title = rust_xlsxwriter::Format::new().set_bold().set_font_size(14);
    let header = rust_xlsxwriter::Format::new()
        .set_bold()
        .set_border(rust_xlsxwriter::FormatBorder::Thin);
    let cell = rust_xlsxwriter::Format::new().set_border(rust_xlsxwriter::FormatBorder::Thin);
    let money = rust_xlsxwriter::Format::new()
        .set_border(rust_xlsxwriter::FormatBorder::Thin)
        .set_num_format("0.00");
    sheet.merge_range(0, 0, 0, 9, &format!("个税年度汇总表（{year}年度）"), &title)?;
    let headers = [
        "工号",
        "姓名",
        "月数",
        "累计收入",
        "累计社保个人",
        "累计公积金个人",
        "累计专项附加",
        "累计已预扣",
        "年度应预扣",
        "差额",
    ];
    for (i, h) in headers.iter().enumerate() {
        sheet.write_with_format(1, i as u16, *h, &header)?;
    }
    let mut r: u32 = 2;
    for row in rows {
        sheet.write_with_format(r, 0, &row.employee_no, &cell)?;
        sheet.write_with_format(r, 1, row.name.as_deref().unwrap_or(""), &cell)?;
        sheet.write_number_with_format(r, 2, row.month_count, &cell)?;
        sheet.write_number_with_format(r, 3, row.total_gross, &money)?;
        sheet.write_number_with_format(r, 4, row.total_ss_personal, &money)?;
        sheet.write_number_with_format(r, 5, row.total_hf_personal, &money)?;
        sheet.write_number_with_format(r, 6, row.total_special_deduction, &money)?;
        sheet.write_number_with_format(r, 7, row.total_tax_withheld, &money)?;
        sheet.write_number_with_format(r, 8, row.annual_tax_due, &money)?;
        sheet.write_number_with_format(r, 9, row.difference, &money)?;
        r += 1;
    }
    for col in 0..10u16 {
        sheet.set_column_width(col, if col < 2 { 12 } else { 14 })?;
    }
    workbook.save(path)?;
    Ok(())
}

/// 个税扣缴申报表（spec 8）：仅锁定月份可导出；每员工一行，列序对齐电子税务局申报录入页。
/// 三险拆列：有社保台账按台账三险个人分摊比例拆 `social_security_personal`，无台账按全局
/// 规则三险个人比例拆；所用比例之和 ≠ 100%（容差 0.005）时该行退合并展示（养老列=总额、
/// 医疗/失业列空）并整表尾注。
pub fn export_tax_withholding_declaration(
    conn: &rusqlite::Connection,
    month: &str,
    path: &str,
) -> AppResult<()> {
    const SPLIT_TOLERANCE: f64 = 0.005;

    // ---- 门禁：仅 status='已锁定'（locked=1）月份可导出；无数据拒绝 ----
    let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM salary_monthly_results WHERE salary_month = ?1",
        params![month],
        |r| r.get(0),
    )?;
    if total == 0 {
        return Err(AppError::InvalidParam(format!(
            "{month} 无工资数据，无法导出个税扣缴申报表"
        )));
    }
    let unlocked: i64 = conn.query_row(
        "SELECT COUNT(*) FROM salary_monthly_results WHERE salary_month = ?1 AND locked = 0",
        params![month],
        |r| r.get(0),
    )?;
    if unlocked > 0 {
        return Err(AppError::InvalidParam(format!(
            "{month} 存在 {unlocked} 条未锁定的工资数据，请先复核并锁定后再导出个税扣缴申报表"
        )));
    }

    // ---- 全局配置：起征点（当月规则）与全局三险个人比例（工资规则页，键缺省视为未配置） ----
    let threshold = crate::db::get_salary_rules(conn)?
        .into_iter()
        .find(|r| r.enabled == 1 && r.rule_key == "tax_threshold")
        .map(|r| r.rule_value)
        .unwrap_or(5000.0);
    let global_rates: Option<(f64, f64, f64)> = {
        let rules = crate::db::get_salary_rules(conn)?;
        let get = |key: &str| {
            rules
                .iter()
                .find(|r| r.enabled == 1 && r.rule_key == key)
                .map(|r| r.rule_value)
        };
        match (
            get("pension_personal_rate"),
            get("medical_personal_rate"),
            get("unemployment_personal_rate"),
        ) {
            (Some(p), Some(m), Some(u)) => Some((p, m, u)),
            _ => None,
        }
    };

    let valid_rates = |rates: (f64, f64, f64)| -> bool {
        let (p, m, u) = rates;
        p >= 0.0 && m >= 0.0 && u >= 0.0 && (p + m + u - 1.0).abs() <= SPLIT_TOLERANCE
    };

    #[derive(Debug)]
    struct WithholdingRow {
        employee_no: String,
        name: String,
        id_card: String,
        special_deduction: f64,
        gross: f64,
        ss_personal: f64,
        hf_personal: f64,
    }

    // ---- 数据行：工资结果联员工表取身份证号/专项附加 ----
    let mut stmt = conn.prepare(
        "SELECT r.employee_no, COALESCE(e.name, r.name), COALESCE(e.id_card, ''),
                COALESCE(e.special_deduction, 0),
                r.gross_salary, r.social_security_personal, r.housing_fund_personal
         FROM salary_monthly_results r
         LEFT JOIN employees e ON e.employee_no = r.employee_no
         WHERE r.salary_month = ?1
         ORDER BY r.employee_no",
    )?;
    let rows = stmt
        .query_map(params![month], |row| {
            Ok(WithholdingRow {
                employee_no: row.get(0)?,
                name: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                id_card: row.get(2)?,
                special_deduction: row.get(3)?,
                gross: row.get(4)?,
                ss_personal: row.get(5)?,
                hf_personal: row.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::from)?;

    // 台账年度取工资月前四位（历史月汇总 SQL 与档位匹配在 salary::cumulative_tax_context 内完成）
    let year: i64 = month.get(0..4).and_then(|y| y.parse().ok()).unwrap_or(0);

    struct WithholdingSplit {
        pension: f64,
        medical: Option<f64>,
        unemployment: Option<f64>,
    }

    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.set_name("扣缴申报表")?;
    let title = Format::new().set_bold().set_font_size(14);
    let header = Format::new()
        .set_bold()
        .set_border(rust_xlsxwriter::FormatBorder::Thin);
    let cell = Format::new().set_border(rust_xlsxwriter::FormatBorder::Thin);
    let money = Format::new()
        .set_border(rust_xlsxwriter::FormatBorder::Thin)
        .set_num_format("#,##0.00");
    let percent = Format::new()
        .set_border(rust_xlsxwriter::FormatBorder::Thin)
        .set_num_format("0.00%");
    let note = Format::new().set_font_color("#8C8C8C").set_font_size(10);

    sheet.merge_range(0, 0, 0, 12, &format!("个税扣缴申报表（{month}）"), &title)?;
    let headers = [
        "姓名",
        "身份证号",
        "收入额",
        "基本养老保险",
        "基本医疗保险",
        "失业保险",
        "住房公积金（个人）",
        "减除费用",
        "专项附加扣除",
        "累计应纳税所得额",
        "当期预扣率",
        "速算扣除数",
        "累计已预扣税额",
    ];
    for (i, h) in headers.iter().enumerate() {
        sheet.write_with_format(1, i as u16, *h, &header)?;
    }

    let mut r: u32 = 2;
    let (mut sum_gross, mut sum_pension, mut sum_medical, mut sum_unemployment) =
        (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let (mut sum_hf, mut sum_prev_tax) = (0.0f64, 0.0f64);
    let mut fallback_count = 0usize;
    for row in &rows {
        // 累计预扣中间量与 salary::calculate_cumulative_tax 共用单一事实源
        // （salary::cumulative_tax_context：历史月汇总 SQL / 应纳税所得额公式 / 档位匹配）
        let ctx = crate::salary::cumulative_tax_context(
            conn,
            &row.employee_no,
            month,
            row.gross,
            row.ss_personal,
            row.hf_personal,
            row.special_deduction,
            threshold,
        )?;

        // 三险拆列：有台账行只认台账比例，无台账行只认全局比例（不交叉回退）
        let ledger_rates: Option<(f64, f64, f64)> = conn
            .query_row(
                "SELECT pension_personal_rate, medical_personal_rate, unemployment_personal_rate
                 FROM social_insurance_profiles WHERE employee_no = ?1 AND profile_year = ?2",
                params![row.employee_no, year],
                |x| Ok((x.get(0)?, x.get(1)?, x.get(2)?)),
            )
            .ok();
        let rates = match ledger_rates {
            Some(lr) => {
                if valid_rates(lr) {
                    Some(lr)
                } else {
                    if row.ss_personal > 0.0 {
                        fallback_count += 1;
                    }
                    None
                }
            }
            None => match global_rates {
                Some(gr) if valid_rates(gr) => Some(gr),
                _ => {
                    if row.ss_personal > 0.0 {
                        fallback_count += 1;
                    }
                    None
                }
            },
        };
        let split = match rates {
            Some((p, m, _u)) => {
                // 逐项四舍五入、失业险吃尾差，保证三列之和精确等于社保个人总额
                let pension = (row.ss_personal * p * 100.0).round() / 100.0;
                let medical = (row.ss_personal * m * 100.0).round() / 100.0;
                let unemployment = ((row.ss_personal - pension - medical) * 100.0).round() / 100.0;
                WithholdingSplit {
                    pension,
                    medical: Some(medical),
                    unemployment: Some(unemployment),
                }
            }
            None => WithholdingSplit {
                pension: row.ss_personal,
                medical: None,
                unemployment: None,
            },
        };

        sheet.write_with_format(r, 0, &row.name, &cell)?;
        sheet.write_with_format(r, 1, &row.id_card, &cell)?;
        sheet.write_number_with_format(r, 2, row.gross, &money)?;
        sheet.write_number_with_format(r, 3, split.pension, &money)?;
        match split.medical {
            Some(v) => {
                sheet.write_number_with_format(r, 4, v, &money)?;
            }
            None => {
                sheet.write_with_format(r, 4, "", &cell)?;
            }
        }
        match split.unemployment {
            Some(v) => {
                sheet.write_number_with_format(r, 5, v, &money)?;
            }
            None => {
                sheet.write_with_format(r, 5, "", &cell)?;
            }
        }
        sheet.write_number_with_format(r, 6, row.hf_personal, &money)?;
        sheet.write_number_with_format(r, 7, threshold, &money)?;
        sheet.write_number_with_format(r, 8, row.special_deduction, &money)?;
        sheet.write_number_with_format(r, 9, ctx.cumulative_taxable, &money)?;
        sheet.write_number_with_format(r, 10, ctx.tax_rate, &percent)?;
        sheet.write_number_with_format(r, 11, ctx.quick_deduction, &money)?;
        sheet.write_number_with_format(r, 12, ctx.prev_tax, &money)?;

        sum_gross += row.gross;
        sum_pension += split.pension;
        sum_medical += split.medical.unwrap_or(0.0);
        sum_unemployment += split.unemployment.unwrap_or(0.0);
        sum_hf += row.hf_personal;
        sum_prev_tax += ctx.prev_tax;
        r += 1;
    }

    // ---- 表尾合计行：收入额 / 三险 / 公积金 / 累计已预扣 ----
    sheet.write_with_format(r, 0, "合计", &header)?;
    sheet.write_with_format(r, 1, "", &cell)?;
    sheet.write_number_with_format(r, 2, sum_gross, &money)?;
    sheet.write_number_with_format(r, 3, sum_pension, &money)?;
    sheet.write_number_with_format(r, 4, sum_medical, &money)?;
    sheet.write_number_with_format(r, 5, sum_unemployment, &money)?;
    sheet.write_number_with_format(r, 6, sum_hf, &money)?;
    for col in [7u16, 8, 9, 10, 11] {
        sheet.write_with_format(r, col, "", &cell)?;
    }
    sheet.write_number_with_format(r, 12, sum_prev_tax, &money)?;

    // ---- 整表尾注：退合并说明 ----
    if fallback_count > 0 {
        sheet.merge_range(
            r + 1,
            0,
            r + 1,
            12,
            &format!(
                "注：{fallback_count} 名员工的三险个人比例未配置或比例之和不等于100%（容差0.005），\
                 其社保个人部分已合并列示于“基本养老保险”列。"
            ),
            &note,
        )?;
    }

    let widths = [10u16, 22, 12, 14, 14, 12, 16, 10, 12, 16, 10, 12, 14];
    for (col, w) in widths.iter().enumerate() {
        sheet.set_column_width(col as u16, *w)?;
    }

    workbook.save(path)?;
    Ok(())
}

/// 科目余额表（试算平衡）：借/贷四栏余额 + 合计行。
pub fn export_trial_balance_excel(report: &TrialBalanceReport, path: &str) -> AppResult<()> {
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.set_name("科目余额表")?;
    let title = rust_xlsxwriter::Format::new().set_bold().set_font_size(14);
    let header = rust_xlsxwriter::Format::new()
        .set_bold()
        .set_border(rust_xlsxwriter::FormatBorder::Thin);
    let cell = rust_xlsxwriter::Format::new().set_border(rust_xlsxwriter::FormatBorder::Thin);
    let money = rust_xlsxwriter::Format::new()
        .set_border(rust_xlsxwriter::FormatBorder::Thin)
        .set_num_format("0.00");
    sheet.merge_range(
        0,
        0,
        0,
        8,
        &format!("科目余额表（{} ~ {}）", report.from_month, report.to_month),
        &title,
    )?;
    let headers = [
        "科目编码",
        "科目名称",
        "期初余额(借)",
        "期初余额(贷)",
        "本期发生(借)",
        "本期发生(贷)",
        "期末余额(借)",
        "期末余额(贷)",
        "类别",
    ];
    for (i, h) in headers.iter().enumerate() {
        sheet.write_with_format(1, i as u16, *h, &header)?;
    }
    let mut r: u32 = 2;
    for row in &report.rows {
        sheet.write_with_format(r, 0, &row.code, &cell)?;
        sheet.write_with_format(r, 1, &row.name, &cell)?;
        sheet.write_number_with_format(r, 2, row.opening_debit, &money)?;
        sheet.write_number_with_format(r, 3, row.opening_credit, &money)?;
        sheet.write_number_with_format(r, 4, row.period_debit, &money)?;
        sheet.write_number_with_format(r, 5, row.period_credit, &money)?;
        sheet.write_number_with_format(r, 6, row.ending_debit, &money)?;
        sheet.write_number_with_format(r, 7, row.ending_credit, &money)?;
        sheet.write_with_format(r, 8, &row.category, &cell)?;
        r += 1;
    }
    sheet.write_with_format(r, 1, "合计", &header)?;
    let total_debit: f64 = report.rows.iter().map(|x| x.ending_debit).sum();
    let total_credit: f64 = report.rows.iter().map(|x| x.ending_credit).sum();
    sheet.write_number_with_format(r, 6, total_debit, &money)?;
    sheet.write_number_with_format(r, 7, total_credit, &money)?;
    for col in 0..9u16 {
        sheet.set_column_width(col, 14)?;
    }
    workbook.save(path)?;
    Ok(())
}

/// 资金日记账（现金/银行存款/第三方支付日记账，spec 6.1/8）：期初 + 明细 + 合计。
/// 列：日期/凭证号/来源/摘要/对方单位/收入/支出/余额/对账状态。
pub fn export_fund_journal_excel(journal: &FundJournal, path: &str) -> AppResult<()> {
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    let title = Format::new().set_bold().set_font_size(14);
    let header = Format::new()
        .set_bold()
        .set_border(rust_xlsxwriter::FormatBorder::Thin);
    let cell = Format::new().set_border(rust_xlsxwriter::FormatBorder::Thin);
    let money = Format::new()
        .set_border(rust_xlsxwriter::FormatBorder::Thin)
        .set_num_format("#,##0.00");
    let account_title = match journal.account_type.as_str() {
        "cash" => "现金日记账",
        "bank" => "银行存款日记账",
        _ => "资金日记账",
    };
    let range_text = match (&journal.from_month, &journal.to_month) {
        (Some(f), Some(t)) if f == t => f.clone(),
        (Some(f), Some(t)) => format!("{f} ~ {t}"),
        (Some(f), None) => format!("{f} 起"),
        (None, Some(t)) => format!("至 {t}"),
        _ => "全部".to_string(),
    };
    sheet.merge_range(
        0,
        0,
        0,
        8,
        &format!(
            "{account_title}（{} {}）",
            journal.fund_account_name, range_text
        ),
        &title,
    )?;
    let headers = [
        "日期",
        "凭证号",
        "来源",
        "摘要",
        "对方单位",
        "收入",
        "支出",
        "余额",
        "对账状态",
    ];
    for (i, h) in headers.iter().enumerate() {
        sheet.write_with_format(1, i as u16, *h, &header)?;
    }
    // 期初行
    sheet.merge_range(2, 0, 2, 4, "期初余额", &header)?;
    sheet.write_number_with_format(2, 7, journal.opening_balance, &money)?;
    for col in [0u16, 1, 2, 3, 4, 5, 6, 8] {
        sheet.write_with_format(2, col, "", &cell)?;
    }
    let mut r: u32 = 3;
    for row in &journal.rows {
        sheet.write_with_format(r, 0, &row.voucher_date, &cell)?;
        sheet.write_with_format(r, 1, &row.voucher_no, &cell)?;
        sheet.write_with_format(r, 2, &row.source_type, &cell)?;
        sheet.write_with_format(r, 3, row.summary.as_deref().unwrap_or(""), &cell)?;
        sheet.write_with_format(r, 4, row.partner_name.as_deref().unwrap_or(""), &cell)?;
        if row.income_amount > 0.0 {
            sheet.write_number_with_format(r, 5, row.income_amount, &money)?;
        } else {
            sheet.write_with_format(r, 5, "", &cell)?;
        }
        if row.expense_amount > 0.0 {
            sheet.write_number_with_format(r, 6, row.expense_amount, &money)?;
        } else {
            sheet.write_with_format(r, 6, "", &cell)?;
        }
        sheet.write_number_with_format(r, 7, row.balance, &money)?;
        sheet.write_with_format(r, 8, journal_reconcile_text(&row.reconcile_status), &cell)?;
        r += 1;
    }
    // 合计行
    sheet.write_with_format(r, 3, "本期合计", &header)?;
    sheet.write_number_with_format(r, 5, journal.total_income, &money)?;
    sheet.write_number_with_format(r, 6, journal.total_expense, &money)?;
    sheet.write_number_with_format(r, 7, journal.closing_balance, &money)?;
    for col in [0u16, 1, 2, 4, 8] {
        sheet.write_with_format(r, col, "", &cell)?;
    }
    let widths = [12u16, 14, 16, 24, 14, 14, 14, 14, 10];
    for (col, w) in widths.iter().enumerate() {
        sheet.set_column_width(col as u16, *w)?;
    }
    workbook.save(path)?;
    Ok(())
}

fn journal_reconcile_text(status: &str) -> &str {
    match status {
        "allocated" => "已核销",
        "partial" => "部分核销",
        "unallocated" => "未核销",
        _ => status,
    }
}

/// 资金日报（第八阶段 Task 7，spec 7）：两 sheet（账户汇总 + 当日明细）。
/// 数据经 `cashier::get_fund_daily_report` 现算（voucher_lines 资金分录，与资金日记账
/// 同源同口径）；敏感导出门禁与日记账导出一致，由前端解锁后调用（get/export 命令层）。
pub fn export_fund_daily_report(
    conn: &rusqlite::Connection,
    date: &str,
    path: &str,
) -> AppResult<()> {
    let report = get_fund_daily_report(conn, date)?;

    let mut workbook = Workbook::new();
    let title = Format::new().set_bold().set_font_size(14);
    let header = Format::new()
        .set_bold()
        .set_border(rust_xlsxwriter::FormatBorder::Thin);
    let cell = Format::new().set_border(rust_xlsxwriter::FormatBorder::Thin);
    let money = Format::new()
        .set_border(rust_xlsxwriter::FormatBorder::Thin)
        .set_num_format("#,##0.00");
    let account_type_text = |t: &str| match t {
        "bank" => "银行",
        "cash" => "现金",
        _ => "第三方支付",
    };

    // ---- Sheet 1：账户汇总（期初 + 收入 − 支出 = 期末，含合计行） ----
    let summary = workbook.add_worksheet();
    summary.set_name("账户汇总")?;
    summary.merge_range(
        0,
        0,
        0,
        5,
        &format!("资金日报（{}）账户汇总", report.date),
        &title,
    )?;
    let summary_headers = [
        "账户",
        "类型",
        "期初余额",
        "当日收入",
        "当日支出",
        "期末余额",
    ];
    for (i, h) in summary_headers.iter().enumerate() {
        summary.write_with_format(1, i as u16, *h, &header)?;
    }
    let mut r: u32 = 2;
    let (mut total_opening, mut total_income, mut total_expense, mut total_closing) =
        (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for row in &report.accounts {
        summary.write_with_format(r, 0, &row.account_name, &cell)?;
        summary.write_with_format(r, 1, account_type_text(&row.account_type), &cell)?;
        summary.write_number_with_format(r, 2, row.opening, &money)?;
        summary.write_number_with_format(r, 3, row.income, &money)?;
        summary.write_number_with_format(r, 4, row.expense, &money)?;
        summary.write_number_with_format(r, 5, row.closing, &money)?;
        total_opening += row.opening;
        total_income += row.income;
        total_expense += row.expense;
        total_closing += row.closing;
        r += 1;
    }
    summary.write_with_format(r, 0, "合计", &header)?;
    summary.write_with_format(r, 1, "", &cell)?;
    summary.write_number_with_format(r, 2, total_opening, &money)?;
    summary.write_number_with_format(r, 3, total_income, &money)?;
    summary.write_number_with_format(r, 4, total_expense, &money)?;
    summary.write_number_with_format(r, 5, total_closing, &money)?;
    let summary_widths = [18u16, 10, 14, 14, 14, 14];
    for (col, w) in summary_widths.iter().enumerate() {
        summary.set_column_width(col as u16, *w)?;
    }

    // ---- Sheet 2：当日明细（按账户分组，含凭证号与摘要，含合计行） ----
    let detail = workbook.add_worksheet();
    detail.set_name("当日明细")?;
    detail.merge_range(
        0,
        0,
        0,
        5,
        &format!("资金日报（{}）当日收支明细", report.date),
        &title,
    )?;
    let detail_headers = ["账户", "凭证号", "摘要", "收入", "支出", "余额"];
    for (i, h) in detail_headers.iter().enumerate() {
        detail.write_with_format(1, i as u16, *h, &header)?;
    }
    let mut r: u32 = 2;
    let (mut entry_income, mut entry_expense) = (0.0f64, 0.0f64);
    for row in &report.entries {
        detail.write_with_format(r, 0, &row.account_name, &cell)?;
        detail.write_with_format(r, 1, &row.voucher_no, &cell)?;
        detail.write_with_format(r, 2, row.summary.as_deref().unwrap_or(""), &cell)?;
        if row.income_amount > 0.0 {
            detail.write_number_with_format(r, 3, row.income_amount, &money)?;
        } else {
            detail.write_with_format(r, 3, "", &cell)?;
        }
        if row.expense_amount > 0.0 {
            detail.write_number_with_format(r, 4, row.expense_amount, &money)?;
        } else {
            detail.write_with_format(r, 4, "", &cell)?;
        }
        detail.write_number_with_format(r, 5, row.balance, &money)?;
        entry_income += row.income_amount;
        entry_expense += row.expense_amount;
        r += 1;
    }
    detail.write_with_format(r, 0, "合计", &header)?;
    detail.write_with_format(r, 1, "", &cell)?;
    detail.write_with_format(r, 2, "", &cell)?;
    detail.write_number_with_format(r, 3, entry_income, &money)?;
    detail.write_number_with_format(r, 4, entry_expense, &money)?;
    detail.write_with_format(r, 5, "", &cell)?;
    let detail_widths = [18u16, 14, 28, 14, 14, 14];
    for (col, w) in detail_widths.iter().enumerate() {
        detail.set_column_width(col as u16, *w)?;
    }

    workbook.save(path)?;
    Ok(())
}

/// 增值税进项台账导出（第八阶段 Task 10，spec 9）：单 sheet 明细 + 月度小计行 + 区间合计行，
/// 文件名 `进项台账_YYYYMM-YYYYMM.xlsx` 由前端 save 对话框给定；表尾明示
/// 「发票登记 ≠ 进项认证」。数据经 `db::get_input_tax_ledger` 现算（排除 void）；
/// 敏感导出门禁与既有导出一致，由前端解锁后调用（get/export 命令层）。
pub fn export_input_tax_ledger(
    conn: &rusqlite::Connection,
    from_month: &str,
    to_month: &str,
    path: &str,
) -> AppResult<()> {
    let report = get_input_tax_ledger(conn, from_month, to_month)?;

    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.set_name("进项台账")?;
    let title = Format::new().set_bold().set_font_size(14);
    let header = Format::new()
        .set_bold()
        .set_border(rust_xlsxwriter::FormatBorder::Thin);
    let cell = Format::new().set_border(rust_xlsxwriter::FormatBorder::Thin);
    let money = Format::new()
        .set_border(rust_xlsxwriter::FormatBorder::Thin)
        .set_num_format("#,##0.00");
    let note = Format::new().set_font_color("#808080");

    // 列：0所属月份 1发票代码 2发票号码 3开票日期 4销方名称 5销方税号
    //     6不含税金额 7税额 8价税合计 9费用类型 10关联报销单号
    sheet.merge_range(
        0,
        0,
        0,
        10,
        &format!(
            "增值税进项台账（{} 至 {}，共 {} 张）",
            report.from_month, report.to_month, report.invoice_count
        ),
        &title,
    )?;
    let headers = [
        "所属月份",
        "发票代码",
        "发票号码",
        "开票日期",
        "销方名称",
        "销方税号",
        "不含税金额",
        "税额",
        "价税合计",
        "费用类型",
        "关联报销单号",
    ];
    for (i, h) in headers.iter().enumerate() {
        sheet.write_with_format(1, i as u16, *h, &header)?;
    }

    let expense_type_text = |row: &InputTaxLedgerRow| -> String {
        row.expense_type_name
            .clone()
            .or_else(|| row.expense_type_code.clone())
            .unwrap_or_else(|| "-".to_string())
    };
    let claim_text = |row: &InputTaxLedgerRow| -> String {
        match &row.claim_no {
            Some(no) if row.refs_count > 1 => format!("{no}（共{}张）", row.refs_count),
            Some(no) => no.clone(),
            None => "-".to_string(),
        }
    };

    let mut r: u32 = 2;
    // 明细行按月分段输出，每月末插小计行（行与小计同源且已按 belong_month 排序，双指针单趟扫完）
    let mut idx = 0usize;
    for subtotal in &report.monthly_subtotals {
        while idx < report.rows.len() && report.rows[idx].belong_month == subtotal.belong_month {
            let row = &report.rows[idx];
            sheet.write_with_format(r, 0, &row.belong_month, &cell)?;
            sheet.write_with_format(r, 1, row.invoice_code.as_deref().unwrap_or(""), &cell)?;
            sheet.write_with_format(r, 2, row.invoice_number.as_deref().unwrap_or(""), &cell)?;
            sheet.write_with_format(r, 3, row.issue_date.as_deref().unwrap_or(""), &cell)?;
            sheet.write_with_format(r, 4, row.seller_name.as_deref().unwrap_or(""), &cell)?;
            sheet.write_with_format(r, 5, row.seller_tax_id.as_deref().unwrap_or(""), &cell)?;
            sheet.write_number_with_format(r, 6, row.amount, &money)?;
            sheet.write_number_with_format(r, 7, row.tax_amount, &money)?;
            sheet.write_number_with_format(r, 8, row.total_amount, &money)?;
            sheet.write_with_format(r, 9, &expense_type_text(row), &cell)?;
            sheet.write_with_format(r, 10, &claim_text(row), &cell)?;
            idx += 1;
            r += 1;
        }
        // 月度小计行
        sheet.write_with_format(r, 0, &format!("{} 小计", subtotal.belong_month), &header)?;
        for col in [1u16, 2, 3, 4, 5] {
            sheet.write_with_format(r, col, "", &cell)?;
        }
        sheet.write_number_with_format(r, 6, subtotal.amount, &money)?;
        sheet.write_number_with_format(r, 7, subtotal.tax_amount, &money)?;
        sheet.write_number_with_format(r, 8, subtotal.total_amount, &money)?;
        sheet.write_with_format(r, 9, &format!("共{}张", subtotal.invoice_count), &cell)?;
        sheet.write_with_format(r, 10, "", &cell)?;
        r += 1;
    }

    // 区间合计行
    sheet.write_with_format(r, 0, "区间合计", &header)?;
    for col in [1u16, 2, 3, 4, 5] {
        sheet.write_with_format(r, col, "", &cell)?;
    }
    sheet.write_number_with_format(r, 6, report.sum_amount, &money)?;
    sheet.write_number_with_format(r, 7, report.sum_tax_amount, &money)?;
    sheet.write_number_with_format(r, 8, report.sum_total_amount, &money)?;
    sheet.write_with_format(r, 9, &format!("共{}张", report.invoice_count), &cell)?;
    sheet.write_with_format(r, 10, "", &cell)?;
    r += 1;
    // 表尾口径提示
    sheet.merge_range(
        r,
        0,
        r,
        10,
        "发票登记 ≠ 进项认证，认证状态以税务系统为准",
        &note,
    )?;

    let widths = [11u16, 20, 14, 12, 26, 20, 14, 12, 14, 12, 22];
    for (col, w) in widths.iter().enumerate() {
        sheet.set_column_width(col as u16, *w)?;
    }

    workbook.save(path)?;
    Ok(())
}

/// 借款备用金台账（Task 14，spec 4.11）：按借款单输出核销进度、未清余额、逾期与账龄。
pub fn export_advance_ledger_excel(ledger: &AdvanceLedger, path: &str) -> AppResult<()> {
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.set_name("借款备用金台账")?;
    let title = Format::new().set_bold().set_font_size(14);
    let header = Format::new()
        .set_bold()
        .set_border(rust_xlsxwriter::FormatBorder::Thin);
    let cell = Format::new().set_border(rust_xlsxwriter::FormatBorder::Thin);
    let money = Format::new()
        .set_border(rust_xlsxwriter::FormatBorder::Thin)
        .set_num_format("#,##0.00");
    sheet.merge_range(
        0,
        0,
        0,
        11,
        &format!("员工借款备用金台账（共 {} 笔）", ledger.rows.len()),
        &title,
    )?;
    let headers = [
        "借款单号",
        "员工",
        "部门",
        "借款日期",
        "预计归还日",
        "借款金额",
        "已核销",
        "未核销余额",
        "未清天数",
        "逾期天数",
        "账龄",
        "状态",
    ];
    for (i, h) in headers.iter().enumerate() {
        sheet.write_with_format(1, i as u16, *h, &header)?;
    }
    fn status_text(s: &str) -> &str {
        match s {
            "approved" => "待发放",
            "batched" => "发放中",
            "settled" => "已发放",
            other => other,
        }
    }
    let mut r: u32 = 2;
    for row in &ledger.rows {
        sheet.write_with_format(r, 0, &row.document_no, &cell)?;
        sheet.write_with_format(r, 1, row.employee_name.as_deref().unwrap_or(""), &cell)?;
        sheet.write_with_format(r, 2, row.department.as_deref().unwrap_or(""), &cell)?;
        sheet.write_with_format(r, 3, &row.document_date, &cell)?;
        sheet.write_with_format(r, 4, row.due_date.as_deref().unwrap_or(""), &cell)?;
        sheet.write_number_with_format(r, 5, row.amount, &money)?;
        sheet.write_number_with_format(r, 6, row.settled_amount, &money)?;
        sheet.write_number_with_format(r, 7, row.outstanding_amount, &money)?;
        sheet.write_number(r, 8, row.days_outstanding as f64)?;
        sheet.write_number(r, 9, row.overdue_days as f64)?;
        sheet.write_with_format(r, 10, &row.aging_bucket, &cell)?;
        sheet.write_with_format(r, 11, status_text(&row.advance_status), &cell)?;
        r += 1;
    }
    // 合计行
    sheet.write_with_format(r, 0, "合计", &header)?;
    for col in [1u16, 2, 3, 4, 8, 9, 10, 11] {
        sheet.write_with_format(r, col, "", &cell)?;
    }
    sheet.write_number_with_format(r, 5, ledger.total_amount, &money)?;
    sheet.write_number_with_format(r, 6, ledger.total_settled, &money)?;
    sheet.write_number_with_format(r, 7, ledger.total_outstanding, &money)?;
    let widths = [16u16, 10, 12, 12, 12, 12, 12, 12, 10, 10, 10, 10];
    for (col, w) in widths.iter().enumerate() {
        sheet.set_column_width(col as u16, *w)?;
    }
    workbook.save(path)?;
    Ok(())
}

/// 银行余额调节表（spec 4.10）：两侧调节勾稽 + 未达项清单。
pub fn export_bank_reconciliation_excel(
    period: &BankReconciliationPeriod,
    path: &str,
) -> AppResult<()> {
    let detail: BankReconciliationDetail = period
        .detail_json
        .as_deref()
        .and_then(|json| serde_json::from_str(json).ok())
        .unwrap_or_default();

    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.set_name("余额调节表")?;
    let title = Format::new().set_bold().set_font_size(14);
    let section = Format::new().set_bold();
    let header = Format::new()
        .set_bold()
        .set_border(rust_xlsxwriter::FormatBorder::Thin);
    let cell = Format::new().set_border(rust_xlsxwriter::FormatBorder::Thin);
    let money = Format::new()
        .set_border(rust_xlsxwriter::FormatBorder::Thin)
        .set_num_format("#,##0.00");

    sheet.merge_range(
        0,
        0,
        0,
        3,
        &format!(
            "银行余额调节表（{} {}月）",
            period.fund_account_name.as_deref().unwrap_or(""),
            period.belong_month
        ),
        &title,
    )?;

    let mut r: u32 = 2;
    let write_item = |sheet: &mut rust_xlsxwriter::Worksheet,
                      row: &mut u32,
                      label: &str,
                      amount: f64,
                      fmt: &Format|
     -> AppResult<()> {
        sheet.write_with_format(*row, 0, label, &cell)?;
        sheet.merge_range(*row, 1, *row, 2, "", &cell)?;
        sheet.write_number_with_format(*row, 3, amount, fmt)?;
        *row += 1;
        Ok(())
    };

    sheet.merge_range(r, 0, r, 3, "一、账面侧（企业日记账）", &section)?;
    r += 1;
    write_item(
        sheet,
        &mut r,
        "账面期末余额",
        period.book_closing_balance,
        &money,
    )?;
    let tx_income: f64 = detail
        .unallocated_transactions
        .iter()
        .filter(|t| t.direction == "income")
        .map(|t| t.remaining_amount)
        .sum();
    let tx_expense: f64 = detail
        .unallocated_transactions
        .iter()
        .filter(|t| t.direction == "expense")
        .map(|t| t.remaining_amount)
        .sum();
    write_item(sheet, &mut r, "加：银行已收、企业未入账", tx_income, &money)?;
    write_item(
        sheet,
        &mut r,
        "减：银行已付、企业未入账",
        tx_expense,
        &money,
    )?;
    write_item(
        sheet,
        &mut r,
        "账面侧调节后余额",
        period.adjusted_book_balance,
        &header,
    )?;

    sheet.merge_range(r, 0, r, 3, "二、银行侧（对账单）", &section)?;
    r += 1;
    write_item(
        sheet,
        &mut r,
        "对账单期末余额",
        period.statement_closing_balance,
        &money,
    )?;
    let line_debit: f64 = detail
        .unallocated_lines
        .iter()
        .filter(|l| l.direction == "debit")
        .map(|l| l.remaining_amount)
        .sum();
    let line_credit: f64 = detail
        .unallocated_lines
        .iter()
        .filter(|l| l.direction == "credit")
        .map(|l| l.remaining_amount)
        .sum();
    write_item(
        sheet,
        &mut r,
        "加：企业已收、银行未收付",
        line_debit,
        &money,
    )?;
    write_item(
        sheet,
        &mut r,
        "减：企业已付、银行未收付",
        line_credit,
        &money,
    )?;
    write_item(
        sheet,
        &mut r,
        "银行侧调节后余额",
        period.adjusted_bank_balance,
        &header,
    )?;

    sheet.merge_range(r, 0, r, 3, "三、勾稽", &section)?;
    r += 1;
    write_item(
        sheet,
        &mut r,
        "对账单期初余额",
        period.statement_opening_balance,
        &money,
    )?;
    write_item(
        sheet,
        &mut r,
        "调节差额（应为 0）",
        period.difference,
        &money,
    )?;
    sheet.write_with_format(r, 0, "确认状态", &cell)?;
    sheet.merge_range(
        r,
        1,
        r,
        3,
        &format!(
            "{}{}",
            if period.status == "confirmed" {
                "已确认"
            } else {
                "草稿"
            },
            period
                .confirmed_by
                .as_deref()
                .map(|by| format!("（{by}）"))
                .unwrap_or_default()
        ),
        &cell,
    )?;
    r += 2;

    // 未达项清单
    sheet.merge_range(
        r,
        0,
        r,
        3,
        "四、未达项——银行已收付、账面未对应（未核销流水）",
        &section,
    )?;
    r += 1;
    sheet.write_with_format(r, 0, "日期", &header)?;
    sheet.write_with_format(r, 1, "流水号", &header)?;
    sheet.write_with_format(r, 2, "摘要/对方", &header)?;
    sheet.write_with_format(r, 3, "未核销金额", &header)?;
    r += 1;
    for tx in &detail.unallocated_transactions {
        sheet.write_with_format(r, 0, &tx.transaction_date, &cell)?;
        sheet.write_with_format(r, 1, &format!("ID={}", tx.transaction_id), &cell)?;
        sheet.write_with_format(
            r,
            2,
            &format!(
                "{}{}",
                tx.summary.as_deref().unwrap_or(""),
                tx.counterparty_name
                    .as_deref()
                    .map(|s| format!(" {s}"))
                    .unwrap_or_default()
            ),
            &cell,
        )?;
        sheet.write_number_with_format(r, 3, tx.remaining_amount, &money)?;
        r += 1;
    }
    sheet.merge_range(
        r,
        0,
        r,
        3,
        "五、未达项——账面已记账、银行未对应（未核销分录）",
        &section,
    )?;
    r += 1;
    sheet.write_with_format(r, 0, "凭证号", &header)?;
    sheet.write_with_format(r, 1, "日期", &header)?;
    sheet.write_with_format(r, 2, "摘要", &header)?;
    sheet.write_with_format(r, 3, "未核销金额", &header)?;
    r += 1;
    for line in &detail.unallocated_lines {
        sheet.write_with_format(r, 0, &line.voucher_no, &cell)?;
        sheet.write_with_format(r, 1, &line.voucher_date, &cell)?;
        sheet.write_with_format(r, 2, line.summary.as_deref().unwrap_or(""), &cell)?;
        sheet.write_number_with_format(r, 3, line.remaining_amount, &money)?;
        r += 1;
    }
    for col in 0..4u16 {
        sheet.set_column_width(col, if col == 0 { 26 } else { 18 })?;
    }
    workbook.save(path)?;
    Ok(())
}

fn report_header_format() -> Format {
    use rust_xlsxwriter::FormatBorder;
    Format::new()
        .set_bold()
        .set_background_color("#D9EAD3")
        .set_border(FormatBorder::Thin)
}

fn report_money_format() -> Format {
    Format::new().set_num_format("#,##0.00")
}

fn set_report_columns(
    worksheet: &mut rust_xlsxwriter::Worksheet,
    column_count: u16,
) -> AppResult<()> {
    for col in 0..column_count {
        worksheet.set_column_width(col, if col == 0 { 16 } else { 14 })?;
    }
    Ok(())
}

fn status_text(status: &str) -> &str {
    match status {
        "ok" => "正常",
        "warning" => "提醒",
        "blocking" => "阻塞",
        _ => status,
    }
}

fn reimbursement_status_text(status: &str) -> &str {
    match status {
        "draft" => "草稿",
        "submitted" => "已提交",
        "approved" => "已审批",
        "rejected" => "已驳回",
        "void" => "已作废",
        _ => status,
    }
}

fn payment_status_text(status: &str) -> &str {
    match status {
        "unpaid" => "未付款",
        "paid" => "已付款",
        _ => status,
    }
}

fn payment_source_text(source_type: &str) -> &str {
    match source_type {
        "salary_result" => "工资",
        "reimbursement_claim" => "报销",
        "fund_document" => "资金单",
        _ => source_type,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// stage9 spec 3.1：导入模板含「邮箱(email)」列，导入解析可回读邮箱（含空串归 None）
    #[test]
    fn test_employee_template_and_import_support_email_column() {
        let path = std::env::temp_dir().join(format!(
            "salary-employee-email-template-{}.xlsx",
            uuid::Uuid::new_v4()
        ));
        export_employee_template(&path.to_string_lossy(), &[]).unwrap();
        assert!(path.exists());

        // 用 calamine 直接改写模板示例行太绕：改为构造含邮箱列的导入文件再解析
        let import_path = std::env::temp_dir().join(format!(
            "salary-employee-email-import-{}.xlsx",
            uuid::Uuid::new_v4()
        ));
        let mut workbook = rust_xlsxwriter::Workbook::new();
        let sheet = workbook.add_worksheet();
        let headers = [
            "工号",
            "姓名",
            "手机号",
            "邮箱(email)",
            "基本工资",
        ];
        for (col, h) in headers.iter().enumerate() {
            sheet.write_string(0, col as u16, *h).unwrap();
        }
        sheet.write_string(1, 0, "E100").unwrap();
        sheet.write_string(1, 1, "张三").unwrap();
        sheet.write_string(1, 2, "1XXXXXXXXXX").unwrap();
        sheet.write_string(1, 3, " zhang@example.com ").unwrap();
        sheet.write_number(1, 4, 5000.0).unwrap();
        sheet.write_string(2, 0, "E101").unwrap();
        sheet.write_string(2, 1, "李四").unwrap();
        // E101 不填邮箱 → None
        workbook.save(&import_path).unwrap();

        let employees = read_employee_excel(&import_path.to_string_lossy()).unwrap();
        assert_eq!(employees.len(), 2);
        let zhang = employees.iter().find(|e| e.employee_no == "E100").unwrap();
        assert_eq!(zhang.email.as_deref(), Some("zhang@example.com"));
        let li = employees.iter().find(|e| e.employee_no == "E101").unwrap();
        assert_eq!(li.email, None);

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(import_path);
    }

    #[test]
    fn test_export_reimbursement_claim_list_creates_file() {
        let path = std::env::temp_dir().join(format!(
            "salary-reimbursement-export-{}.xlsx",
            uuid::Uuid::new_v4()
        ));
        let claims = vec![ReimbursementClaim {
            id: 1,
            claim_no: "BX2026080001".into(),
            employee_id: Some(1),
            employee_name: Some("张三".into()),
            department: Some("销售部".into()),
            belong_month: "2026-08".into(),
            title: "差旅报销".into(),
            total_amount: 128.5,
            invoice_count: 2,
            status: "approved".into(),
            payment_status: "paid".into(),
            payment_date: Some("2026-08-31".into()),
            remark: Some("测试".into()),
            created_at: Some("2026-08-01T00:00:00Z".into()),
            updated_at: Some("2026-08-31T00:00:00Z".into()),
        }];

        export_reimbursement_claim_list(&claims, &path.to_string_lossy()).unwrap();
        assert!(path.exists());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn test_export_balance_sheet_creates_file() {
        let path = std::env::temp_dir().join(format!(
            "salary-balance-sheet-{}.xlsx",
            uuid::Uuid::new_v4()
        ));
        let report = BalanceSheet {
            month: "2026-02".into(),
            enabled: true,
            asset_rows: vec![
                ReportRow {
                    key: "monetary".into(),
                    label: "货币资金".into(),
                    current: 67000.0,
                    comparative: 100000.0,
                    prior_year: 0.0,
                },
                ReportRow {
                    key: "1601".into(),
                    label: "固定资产".into(),
                    current: 30000.0,
                    comparative: 0.0,
                    prior_year: 0.0,
                },
            ],
            liability_equity_rows: vec![ReportRow {
                key: "3001".into(),
                label: "实收资本".into(),
                current: 97000.0,
                comparative: 100000.0,
                prior_year: 0.0,
            }],
            asset_total: 97000.0,
            liability_equity_total: 97000.0,
            balanced: true,
            has_prior_year: false,
        };

        export_balance_sheet(&report, &path.to_string_lossy()).unwrap();
        assert_file_nonempty(&path);
    }

    #[test]
    fn test_export_income_statement_creates_file() {
        let path = std::env::temp_dir().join(format!(
            "salary-income-statement-{}.xlsx",
            uuid::Uuid::new_v4()
        ));
        let report = IncomeStatement {
            month: "2026-02".into(),
            year_cumulative: true,
            rows: vec![
                ReportRow {
                    key: "6001".into(),
                    label: "主营业务收入".into(),
                    current: 20000.0,
                    comparative: 50000.0,
                    prior_year: 0.0,
                },
                ReportRow {
                    key: "net_profit".into(),
                    label: "净利润".into(),
                    current: 8000.0,
                    comparative: 21000.0,
                    prior_year: 0.0,
                },
            ],
            net_profit_month: 8000.0,
            net_profit_year: 21000.0,
            has_prior_year: false,
        };

        export_income_statement(&report, &path.to_string_lossy()).unwrap();
        assert_file_nonempty(&path);
    }

    #[test]
    fn test_export_cash_flow_statement_creates_file() {
        let path =
            std::env::temp_dir().join(format!("salary-cash-flow-{}.xlsx", uuid::Uuid::new_v4()));
        let report = CashFlowStatement {
            month: "2026-02".into(),
            rows: vec![
                ReportRow {
                    key: "operating_inflow".into(),
                    label: "经营活动现金流入".into(),
                    current: 2000.0,
                    comparative: 0.0,
                    prior_year: 0.0,
                },
                ReportRow {
                    key: "operating_outflow".into(),
                    label: "经营活动现金流出".into(),
                    current: 5000.0,
                    comparative: 0.0,
                    prior_year: 0.0,
                },
            ],
            net_increase: -3000.0,
            unclassified: vec![UnclassifiedCashItem {
                voucher_no: "JZ-202602-001".into(),
                summary: Some("未分类支出".into()),
                amount: -100.0,
            }],
            has_prior_year: false,
        };

        export_cash_flow_statement(&report, &path.to_string_lossy()).unwrap();
        assert_file_nonempty(&path);
    }

    #[test]
    fn test_export_trial_balance_smoke() {
        let report = TrialBalanceReport {
            from_month: "2026-01".into(),
            to_month: "2026-06".into(),
            enabled: true,
            balanced: true,
            rows: vec![TrialBalanceRow {
                code: "1001".into(),
                name: "库存现金".into(),
                category: "asset".into(),
                direction: "debit".into(),
                opening_debit: 1000.0,
                opening_credit: 0.0,
                period_debit: 0.0,
                period_credit: 100.0,
                ending_debit: 900.0,
                ending_credit: 0.0,
            }],
        };
        let path = std::env::temp_dir().join("trial_balance_smoke.xlsx");
        export_trial_balance_excel(&report, path.to_str().unwrap()).unwrap();
        assert!(path.exists());
        let _ = std::fs::remove_file(path);
    }

    fn assert_file_nonempty(path: &Path) {
        assert!(path.exists(), "导出文件未生成: {}", path.display());
        let size = std::fs::metadata(path).unwrap().len();
        assert!(size > 0, "导出文件为空: {}", path.display());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn test_read_bank_transactions_csv_with_quoted_summary() {
        let path = std::env::temp_dir().join(format!(
            "salary-bank-transactions-{}.csv",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(
            &path,
            "交易日期,摘要,对方户名,对方账号,收入,支出,余额\n2026-08-31,\"工资,代发\",张三,62220001,0,7800,10000\n",
        )
        .unwrap();

        let records = read_bank_transactions_file(&path.to_string_lossy()).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].belong_month, "2026-08");
        assert_eq!(records[0].summary.as_deref(), Some("工资,代发"));
        assert_eq!(records[0].expense_amount, 7800.0);
        let _ = std::fs::remove_file(path);
    }

    // ==================== 个税扣缴申报表导出（Task 9，spec 8） ====================

    fn withholding_env() -> rusqlite::Connection {
        use crate::db::{create_tables, insert_default_data, seed_gl_accounts};
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        seed_gl_accounts(&conn).unwrap();
        insert_default_data(&conn).unwrap();
        conn
    }

    fn withholding_employee(
        conn: &rusqlite::Connection,
        no: &str,
        name: &str,
        id_card: &str,
        special: f64,
    ) {
        conn.execute(
            "INSERT INTO employees (employee_no, name, id_card, special_deduction, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 'active', '2026-01-01', '2026-01-01')",
            params![no, name, id_card, special],
        )
        .unwrap();
    }

    fn withholding_result(
        conn: &rusqlite::Connection,
        month: &str,
        no: &str,
        name: &str,
        gross: f64,
        ss: f64,
        hf: f64,
        tax: f64,
        locked: bool,
    ) {
        conn.execute(
            "INSERT INTO salary_monthly_results
                (salary_month, employee_no, name, gross_salary, social_security_personal,
                 housing_fund_personal, tax_amount, status, locked, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, '2026-01-01', '2026-01-01')",
            params![
                month,
                no,
                name,
                gross,
                ss,
                hf,
                tax,
                if locked { "locked" } else { "draft" },
                locked as i64,
            ],
        )
        .unwrap();
    }

    fn withholding_profile(conn: &rusqlite::Connection, no: &str, rates: (f64, f64, f64)) {
        conn.execute(
            "INSERT INTO social_insurance_profiles
                (employee_no, profile_year, ss_base, hf_base,
                 pension_personal_rate, medical_personal_rate, unemployment_personal_rate,
                 created_at, updated_at)
             VALUES (?1, 2026, 5000, 5000, ?2, ?3, ?4, '2026-01-01', '2026-01-01')",
            params![no, rates.0, rates.1, rates.2],
        )
        .unwrap();
    }

    fn withholding_global_rates(conn: &rusqlite::Connection, rates: (f64, f64, f64)) {
        for (key, value) in [
            ("pension_personal_rate", rates.0),
            ("medical_personal_rate", rates.1),
            ("unemployment_personal_rate", rates.2),
        ] {
            conn.execute(
                "INSERT INTO salary_rules (rule_key, rule_name, rule_value, rule_type, enabled)
                 VALUES (?1, '三险个人比例（导出拆列）', ?2, 'insurance', 1)",
                params![key, value],
            )
            .unwrap();
        }
    }

    fn withholding_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "tax-withholding-{}-{name}.xlsx",
            std::process::id()
        ))
    }

    /// 门禁：月份无工资数据拒绝导出
    #[test]
    fn test_tax_withholding_gate_empty_month_rejected() {
        let conn = withholding_env();
        let path = withholding_path("empty");
        let err = export_tax_withholding_declaration(&conn, "2026-08", path.to_str().unwrap())
            .unwrap_err();
        assert!(err.to_string().contains("无工资数据"), "实际错误：{err}");
    }

    /// 门禁：存在未锁定（草稿）工资数据的月份拒绝导出
    #[test]
    fn test_tax_withholding_gate_draft_month_rejected() {
        let conn = withholding_env();
        withholding_employee(&conn, "E001", "张三", "110101199001011234", 1500.0);
        withholding_result(
            &conn, "2026-08", "E001", "张三", 10000.0, 1050.0, 1200.0, 80.0, false,
        );
        let path = withholding_path("draft");
        let err = export_tax_withholding_declaration(&conn, "2026-08", path.to_str().unwrap())
            .unwrap_err();
        assert!(err.to_string().contains("未锁定"), "实际错误：{err}");
    }

    /// 台账拆列：三险按台账分摊比例拆 social_security_personal（失业险吃尾差）；
    /// 税额各列与累计预扣口径勾稽；身份证号取 employees；合计行勾稽；无尾注。
    #[test]
    fn test_tax_withholding_ledger_split() {
        use calamine::{open_workbook_auto, Data, Reader};
        let conn = withholding_env();
        withholding_employee(&conn, "E001", "张三", "110101199001011234", 1500.0);
        // 台账三险个人分摊比例：0.762 + 0.190 + 0.048 = 1.000
        withholding_profile(&conn, "E001", (0.762, 0.190, 0.048));
        // 上月已预扣 600（同年度历史月自动作为累计已预扣基数）
        withholding_result(
            &conn, "2026-07", "E001", "张三", 9000.0, 900.0, 1000.0, 600.0, true,
        );
        withholding_result(
            &conn, "2026-08", "E001", "张三", 10000.0, 1050.0, 1200.0, 80.0, true,
        );

        let path = withholding_path("ledger");
        export_tax_withholding_declaration(&conn, "2026-08", path.to_str().unwrap()).unwrap();

        let mut workbook = open_workbook_auto(path.to_str().unwrap()).unwrap();
        let sheet = workbook.worksheet_range("扣缴申报表").unwrap();
        let text = |r: u32, c: u32| -> String {
            match sheet.get_value((r, c)) {
                Some(Data::String(s)) => s.to_string(),
                other => panic!("应为文本单元格：({r},{c}) = {other:?}"),
            }
        };
        let num = |r: u32, c: u32| -> f64 {
            match sheet.get_value((r, c)) {
                Some(Data::Float(v)) => *v,
                Some(Data::Int(v)) => *v as f64,
                other => panic!("应为数字单元格：({r},{c}) = {other:?}"),
            }
        };

        // 标题 + 表头 + 1 数据行 + 合计 = 4 行（无退合并尾注）
        assert_eq!(sheet.height(), 4, "无退合并时不应有尾注行");
        assert_eq!(text(2, 0), "张三");
        assert_eq!(
            text(2, 1),
            "110101199001011234",
            "身份证号应取 employees.id_card"
        );
        assert!((num(2, 2) - 10000.0).abs() < 0.005, "收入额=应发合计");
        assert!((num(2, 3) - 800.1).abs() < 0.005, "养老=1050×0.762");
        assert!((num(2, 4) - 199.5).abs() < 0.005, "医疗=1050×0.190");
        assert!(
            (num(2, 5) - 50.4).abs() < 0.005,
            "失业=总额-养老-医疗（尾差）"
        );
        assert!((num(2, 6) - 1200.0).abs() < 0.005, "公积金个人");
        assert!(
            (num(2, 7) - 5000.0).abs() < 0.005,
            "减除费用=当月规则起征点"
        );
        assert!(
            (num(2, 8) - 1500.0).abs() < 0.005,
            "专项附加=employees.special_deduction"
        );
        // 累计应纳税所得额 = 19000 − 4150 − 5000×2 − 1500×2 = 1850 → 3% 档
        assert!((num(2, 9) - 1850.0).abs() < 0.005, "累计应纳税所得额");
        assert!((num(2, 10) - 0.03).abs() < 0.000001, "当期预扣率");
        assert!((num(2, 11) - 0.0).abs() < 0.005, "速算扣除数");
        assert!((num(2, 12) - 600.0).abs() < 0.005, "累计已预扣=上月税额");

        // 合计行：收入额 / 三险 / 公积金 / 累计已预扣
        assert_eq!(text(3, 0), "合计");
        assert!((num(3, 2) - 10000.0).abs() < 0.005);
        assert!(
            (num(3, 3) + num(3, 4) + num(3, 5) - 1050.0).abs() < 0.005,
            "三险合计=社保个人总额"
        );
        assert!((num(3, 6) - 1200.0).abs() < 0.005);
        assert!((num(3, 12) - 600.0).abs() < 0.005);
        let _ = std::fs::remove_file(path);
    }

    /// 全局比例拆列：无台账员工按 salary_rules 三险个人比例拆
    #[test]
    fn test_tax_withholding_global_rates_split() {
        use calamine::{open_workbook_auto, Data, Reader};
        let conn = withholding_env();
        withholding_employee(&conn, "E002", "李四", "", 1000.0);
        withholding_global_rates(&conn, (0.8, 0.15, 0.05));
        withholding_result(
            &conn, "2026-08", "E002", "李四", 8000.0, 200.0, 900.0, 30.0, true,
        );

        let path = withholding_path("global");
        export_tax_withholding_declaration(&conn, "2026-08", path.to_str().unwrap()).unwrap();

        let mut workbook = open_workbook_auto(path.to_str().unwrap()).unwrap();
        let sheet = workbook.worksheet_range("扣缴申报表").unwrap();
        let num = |r: u32, c: u32| -> f64 {
            match sheet.get_value((r, c)) {
                Some(Data::Float(v)) => *v,
                Some(Data::Int(v)) => *v as f64,
                other => panic!("应为数字单元格：({r},{c}) = {other:?}"),
            }
        };
        assert_eq!(sheet.height(), 4);
        assert!((num(2, 3) - 160.0).abs() < 0.005, "养老=200×0.8");
        assert!((num(2, 4) - 30.0).abs() < 0.005, "医疗=200×0.15");
        assert!((num(2, 5) - 10.0).abs() < 0.005, "失业=200×0.05（尾差）");
        // 累计应纳税所得额 = 8000 − 1100 − 5000 − 1000 = 900 → 3% 档
        assert!((num(2, 9) - 900.0).abs() < 0.005);
        let _ = std::fs::remove_file(path);
    }

    /// 比例之和 ≠ 100%（容差 0.005）→ 退合并展示（养老列=总额、医疗/失业列空）+ 整表尾注
    #[test]
    fn test_tax_withholding_ratio_fallback_merged_with_note() {
        use calamine::{open_workbook_auto, Data, Reader};
        let conn = withholding_env();
        // E003 无 employees 行：姓名回落工资结果、身份证号空（LEFT JOIN + COALESCE）
        withholding_profile(&conn, "E003", (0.5, 0.2, 0.1)); // 和 = 0.8 ≠ 100%
        withholding_result(
            &conn, "2026-08", "E003", "王五", 10000.0, 1050.0, 1200.0, 80.0, true,
        );

        let path = withholding_path("fallback");
        export_tax_withholding_declaration(&conn, "2026-08", path.to_str().unwrap()).unwrap();

        let mut workbook = open_workbook_auto(path.to_str().unwrap()).unwrap();
        let sheet = workbook.worksheet_range("扣缴申报表").unwrap();
        let text = |r: u32, c: u32| -> String {
            match sheet.get_value((r, c)) {
                Some(Data::String(s)) => s.to_string(),
                Some(Data::Empty) | None => String::new(),
                other => panic!("应为文本单元格：({r},{c}) = {other:?}"),
            }
        };
        let num = |r: u32, c: u32| -> f64 {
            match sheet.get_value((r, c)) {
                Some(Data::Float(v)) => *v,
                Some(Data::Int(v)) => *v as f64,
                other => panic!("应为数字单元格：({r},{c}) = {other:?}"),
            }
        };
        let blank = |r: u32, c: u32| match sheet.get_value((r, c)) {
            None | Some(Data::Empty) => {}
            Some(Data::String(s)) if s.is_empty() => {}
            other => panic!("应为空单元格：({r},{c}) = {other:?}"),
        };

        // 标题 + 表头 + 1 数据行 + 合计 + 尾注 = 5 行
        assert_eq!(sheet.height(), 5, "退合并时应有整表尾注行");
        assert_eq!(text(2, 0), "王五", "无员工档案时姓名回落工资结果");
        assert_eq!(text(2, 1), "", "无员工档案时身份证号为空");
        assert!(
            (num(2, 3) - 1050.0).abs() < 0.005,
            "退合并：养老列=社保个人总额"
        );
        blank(2, 4);
        blank(2, 5);
        let note = text(4, 0);
        assert!(note.contains("合并"), "尾注应说明退合并：{note}");
        assert!(note.contains("1"), "尾注应含退合并人数：{note}");
        let _ = std::fs::remove_file(path);
    }

    /// 多员工回读：行数=锁定员工数，合计行等于各行之和
    #[test]
    fn test_tax_withholding_rows_count_and_totals() {
        use calamine::{open_workbook_auto, Data, Reader};
        let conn = withholding_env();
        withholding_global_rates(&conn, (0.8, 0.15, 0.05));
        withholding_employee(&conn, "E001", "张三", "110101199001011234", 1500.0);
        withholding_employee(&conn, "E002", "李四", "110101199002022345", 1000.0);
        // 上月已预扣（计入累计已预扣列，当月税额不计）
        withholding_result(
            &conn, "2026-07", "E001", "张三", 9000.0, 900.0, 1000.0, 40.0, true,
        );
        withholding_result(
            &conn, "2026-07", "E002", "李四", 7500.0, 800.0, 850.0, 15.0, true,
        );
        withholding_result(
            &conn, "2026-08", "E001", "张三", 10000.0, 1050.0, 1200.0, 80.0, true,
        );
        withholding_result(
            &conn, "2026-08", "E002", "李四", 8000.0, 200.0, 900.0, 30.0, true,
        );
        // 其他月草稿数据不参与 2026-08 导出
        withholding_result(&conn, "2026-09", "E001", "张三", 1.0, 0.0, 0.0, 0.0, false);

        let path = withholding_path("totals");
        export_tax_withholding_declaration(&conn, "2026-08", path.to_str().unwrap()).unwrap();

        let mut workbook = open_workbook_auto(path.to_str().unwrap()).unwrap();
        let sheet = workbook.worksheet_range("扣缴申报表").unwrap();
        let num = |r: u32, c: u32| -> f64 {
            match sheet.get_value((r, c)) {
                Some(Data::Float(v)) => *v,
                Some(Data::Int(v)) => *v as f64,
                other => panic!("应为数字单元格：({r},{c}) = {other:?}"),
            }
        };
        // 标题 + 表头 + 2 数据行 + 合计 = 5 行
        assert_eq!(sheet.height(), 5, "行数应=锁定员工数（其他月草稿不计）");
        assert!((num(4, 2) - 18000.0).abs() < 0.005, "收入额合计");
        assert!(
            (num(4, 3) + num(4, 4) + num(4, 5) - 1250.0).abs() < 0.005,
            "三险合计"
        );
        assert!((num(4, 6) - 2100.0).abs() < 0.005, "公积金合计");
        assert!(
            (num(4, 12) - 55.0).abs() < 0.005,
            "累计已预扣合计=上月已预扣之和"
        );
        let _ = std::fs::remove_file(path);
    }
}
