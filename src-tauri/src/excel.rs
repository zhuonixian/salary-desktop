use std::collections::HashMap;

use calamine::{open_workbook_auto, Data, Reader};
use rust_xlsxwriter::{Format, Workbook};

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
    let col_bank = get_col("银行账号", &headers).or_else(|| get_col("bank_account", &headers));
    let col_bankname = get_col("开户行", &headers).or_else(|| get_col("bank_name", &headers));
    let col_hire = get_col("入职日期", &headers).or_else(|| get_col("hire_date", &headers));
    let col_base = get_col("基本工资", &headers).or_else(|| get_col("base_salary", &headers));
    let col_pos_salary = get_col("岗位工资", &headers).or_else(|| get_col("position_salary", &headers));
    let col_perf = get_col("绩效工资", &headers).or_else(|| get_col("performance_salary", &headers));
    let col_social = get_col("社保基数", &headers).or_else(|| get_col("social_security_base", &headers));
    let col_housing = get_col("公积金基数", &headers).or_else(|| get_col("housing_fund_base", &headers));
    let col_special = get_col("专项附加扣除", &headers).or_else(|| get_col("special_deduction", &headers));
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
                if s.is_empty() { String::new() } else { s }
            })
        };

        let get_f64 = |col: Option<usize>| -> f64 {
            col.and_then(|i| row.get(i))
                .map(|c| {
                    match c {
                        Data::Float(f) => *f,
                        Data::Int(i) => *i as f64,
                        Data::String(s) => s.parse::<f64>().unwrap_or(0.0),
                        _ => 0.0,
                    }
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
    let col_expected = get_col("应出勤天数", &headers).or_else(|| get_col("expected_days", &headers));
    let col_actual = get_col("实出勤天数", &headers).or_else(|| get_col("actual_days", &headers));
    let col_late = get_col("迟到次数", &headers).or_else(|| get_col("late_count", &headers));
    let col_early = get_col("早退次数", &headers).or_else(|| get_col("early_leave_count", &headers));
    let col_personal = get_col("事假天数", &headers).or_else(|| get_col("personal_leave_days", &headers));
    let col_sick = get_col("病假天数", &headers).or_else(|| get_col("sick_leave_days", &headers));
    let col_absent = get_col("旷工天数", &headers).or_else(|| get_col("absent_days", &headers));
    let col_overtime = get_col("加班小时", &headers).or_else(|| get_col("overtime_hours", &headers));
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
                if s.is_empty() { String::new() } else { s }
            })
        };

        let get_f64 = |col: Option<usize>| -> f64 {
            col.and_then(|i| row.get(i))
                .map(|c| {
                    match c {
                        Data::Float(f) => *f,
                        Data::Int(i) => *i as f64,
                        Data::String(s) => s.parse::<f64>().unwrap_or(0.0),
                        _ => 0.0,
                    }
                })
                .unwrap_or(0.0)
        };

        let get_i32 = |col: Option<usize>| -> i32 {
            get_f64(col) as i32
        };

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

pub fn export_employee_template(path: &str) -> AppResult<()> {
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet.set_name("员工导入模板")?;

    let headers = vec![
        "工号", "姓名", "部门", "职位", "身份证号", "手机号", "银行账号", "开户行",
        "入职日期", "基本工资", "岗位工资", "绩效工资", "社保基数", "公积金基数",
        "专项附加扣除", "备注",
    ];
    write_header_row(worksheet, &headers, 0)?;

    let cell_fmt = make_cell_format();
    let money_fmt = make_money_format();
    worksheet.write_string_with_format(1, 0, "E001", &cell_fmt)?;
    worksheet.write_string_with_format(1, 1, "张三", &cell_fmt)?;
    worksheet.write_string_with_format(1, 2, "生产部", &cell_fmt)?;
    worksheet.write_string_with_format(1, 3, "操作员", &cell_fmt)?;
    worksheet.write_string_with_format(1, 4, "", &cell_fmt)?;
    worksheet.write_string_with_format(1, 5, "1XXXXXXXXXX", &cell_fmt)?;
    worksheet.write_string_with_format(1, 6, "", &cell_fmt)?;
    worksheet.write_string_with_format(1, 7, "", &cell_fmt)?;
    worksheet.write_string_with_format(1, 8, "2026-01-01", &cell_fmt)?;
    worksheet.write_number_with_format(1, 9, 5000.0, &money_fmt)?;
    worksheet.write_number_with_format(1, 10, 1000.0, &money_fmt)?;
    worksheet.write_number_with_format(1, 11, 800.0, &money_fmt)?;
    worksheet.write_number_with_format(1, 12, 5000.0, &money_fmt)?;
    worksheet.write_number_with_format(1, 13, 5000.0, &money_fmt)?;
    worksheet.write_number_with_format(1, 14, 0.0, &money_fmt)?;
    worksheet.write_string_with_format(1, 15, "示例行，可删除", &cell_fmt)?;

    let widths = [12, 10, 12, 12, 20, 14, 22, 18, 12, 12, 12, 12, 12, 12, 14, 20];
    for (col, w) in widths.iter().enumerate() {
        worksheet.set_column_width(col as u16, *w)?;
    }

    workbook.save(path)?;
    Ok(())
}

pub fn export_attendance_template(path: &str) -> AppResult<()> {
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet.set_name("考勤导入模板")?;

    let headers = vec![
        "工号", "姓名", "应出勤天数", "实出勤天数", "迟到次数", "早退次数",
        "事假天数", "病假天数", "旷工天数", "加班小时", "备注",
    ];
    write_header_row(worksheet, &headers, 0)?;

    let cell_fmt = make_cell_format();
    let num_fmt = Format::new()
        .set_border(rust_xlsxwriter::FormatBorder::Thin)
        .set_align(rust_xlsxwriter::FormatAlign::Center)
        .set_num_format("0.0");
    worksheet.write_string_with_format(1, 0, "E001", &cell_fmt)?;
    worksheet.write_string_with_format(1, 1, "张三", &cell_fmt)?;
    worksheet.write_number_with_format(1, 2, 22.0, &num_fmt)?;
    worksheet.write_number_with_format(1, 3, 22.0, &num_fmt)?;
    worksheet.write_number_with_format(1, 4, 0.0, &cell_fmt)?;
    worksheet.write_number_with_format(1, 5, 0.0, &cell_fmt)?;
    worksheet.write_number_with_format(1, 6, 0.0, &num_fmt)?;
    worksheet.write_number_with_format(1, 7, 0.0, &num_fmt)?;
    worksheet.write_number_with_format(1, 8, 0.0, &num_fmt)?;
    worksheet.write_number_with_format(1, 9, 0.0, &num_fmt)?;
    worksheet.write_string_with_format(1, 10, "示例行，可删除", &cell_fmt)?;

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
        "序号", "工号", "姓名", "部门", "基本工资", "岗位工资", "绩效工资",
        "加班工资", "餐补", "交通补助", "其他补助", "应发合计",
        "社保个人", "公积金个人", "考勤扣款", "个人所得税", "其他扣款", "实发工资",
        "状态", "备注",
    ];

    write_header_row(worksheet, &headers, 0)?;

    let cell_fmt = make_cell_format();
    let money_fmt = make_money_format();

    for (i, r) in results.iter().enumerate() {
        let row = (i + 1) as u32;
        worksheet.write_number_with_format(row, 0, (i + 1) as f64, &cell_fmt)?;
        worksheet.write_string_with_format(row, 1, &r.employee_no, &cell_fmt)?;
        worksheet.write_string_with_format(row, 2, r.name.as_deref().unwrap_or(""), &cell_fmt)?;
        worksheet.write_string_with_format(row, 3, r.department.as_deref().unwrap_or(""), &cell_fmt)?;
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
        worksheet.write_string_with_format(row, 19, r.remark.as_deref().unwrap_or(""), &cell_fmt)?;
    }

    // Summary row
    if !results.is_empty() {
        let summary_row = (results.len() + 1) as u32;
        let bold_fmt = Format::new().set_bold().set_border(rust_xlsxwriter::FormatBorder::Thin);
        worksheet.write_string_with_format(summary_row, 0, "合计", &bold_fmt)?;
        let sum_col = |col: u16| -> f64 {
            results.iter().map(|r| match col {
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
            }).sum()
        };
        for col in [4u16, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17] {
            let sum_money_fmt = Format::new().set_bold().set_border(rust_xlsxwriter::FormatBorder::Thin).set_num_format("#,##0.00");
            worksheet.write_number_with_format(summary_row, col, sum_col(col), &sum_money_fmt)?;
        }
    }

    // Set column widths
    let widths = [6, 12, 10, 12, 12, 12, 12, 12, 10, 10, 10, 12, 12, 12, 12, 12, 12, 12, 8, 20];
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
        "序号", "姓名", "银行账号", "开户行", "实发金额", "工号", "备注",
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
    let value_fmt = Format::new()
        .set_border(rust_xlsxwriter::FormatBorder::Thin);
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
        SlipRow { label: "工号", value: result.employee_no.clone(), is_money: false },
        SlipRow { label: "姓名", value: name_str.to_string(), is_money: false },
        SlipRow { label: "部门", value: dept_str.to_string(), is_money: false },
        SlipRow { label: "", value: String::new(), is_money: false },
        SlipRow { label: "基本工资", value: format!("{:.2}", result.base_salary), is_money: true },
        SlipRow { label: "岗位工资", value: format!("{:.2}", result.position_salary), is_money: true },
        SlipRow { label: "绩效工资", value: format!("{:.2}", result.performance_salary), is_money: true },
        SlipRow { label: "加班工资", value: format!("{:.2}", result.overtime_salary), is_money: true },
        SlipRow { label: "餐补", value: format!("{:.2}", result.meal_allowance), is_money: true },
        SlipRow { label: "交通补助", value: format!("{:.2}", result.transport_allowance), is_money: true },
        SlipRow { label: "其他补助", value: format!("{:.2}", result.other_allowance), is_money: true },
        SlipRow { label: "", value: String::new(), is_money: false },
        SlipRow { label: "应发合计", value: format!("{:.2}", result.gross_salary), is_money: true },
        SlipRow { label: "", value: String::new(), is_money: false },
        SlipRow { label: "社保个人扣款", value: format!("{:.2}", result.social_security_personal), is_money: true },
        SlipRow { label: "公积金个人扣款", value: format!("{:.2}", result.housing_fund_personal), is_money: true },
        SlipRow { label: "考勤扣款", value: format!("{:.2}", result.attendance_deduction), is_money: true },
        SlipRow { label: "个人所得税", value: format!("{:.2}", result.tax_amount), is_money: true },
        SlipRow { label: "其他扣款", value: format!("{:.2}", result.other_deduction), is_money: true },
        SlipRow { label: "", value: String::new(), is_money: false },
        SlipRow { label: "实发工资", value: format!("{:.2}", result.net_salary), is_money: true },
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
        "序号", "工号", "姓名", "月份", "应出勤天数", "实出勤天数",
        "迟到次数", "早退次数", "事假天数", "病假天数", "旷工天数",
        "加班小时", "数据来源", "备注",
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
        worksheet.write_string_with_format(row, 12, r.source_type.as_deref().unwrap_or(""), &cell_fmt)?;
        worksheet.write_string_with_format(row, 13, r.remark.as_deref().unwrap_or(""), &cell_fmt)?;
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
    (2, 17, 19, "春节"),  // Spring Festival approx
    (4, 4, 6, "清明"),
    (5, 1, 3, "劳动节"),
    (5, 31, 6, "端午"),   // spans month boundary
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

    let title_fmt = Format::new().set_bold().set_font_size(14).set_align(rust_xlsxwriter::FormatAlign::Center);
    let info_fmt = Format::new().set_font_size(10);
    let header_fmt = Format::new().set_bold().set_font_size(9).set_background_color("D9E1F2").set_align(rust_xlsxwriter::FormatAlign::Center);
    let cell_fmt = Format::new().set_font_size(9).set_align(rust_xlsxwriter::FormatAlign::Center);
    let holiday_fmt = Format::new().set_font_size(7).set_font_color("FF0000").set_align(rust_xlsxwriter::FormatAlign::Center);

    let days_in_month = get_days_in_month(month);
    let month_parts: Vec<&str> = month.split('-').collect();
    let mon: u32 = month_parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
    let year: u32 = month_parts.first().and_then(|s| s.parse().ok()).unwrap_or(2026);

    // Display month name (e.g., "2026年5月")
    let month_label = format!("{}年{}月", year, mon);

    // Column layout: 序号(Col 0), 姓名(Col 1), then day*2 cols (白/夜 per day), then 合计白(Col -4), 合计夜(Col -3), 合计(Col -2), 备注(Col -1)
    let day_col_start: u16 = 2; // first day sub-column
    let summary_col_day: u16 = day_col_start + (days_in_month * 2) as u16;
    let summary_col_night: u16 = summary_col_day + 1;
    let summary_col_total: u16 = summary_col_day + 2;
    let remark_col: u16 = summary_col_day + 3;

    // Row 0: Title (merged across all columns)
    ws.merge_range(0, 0, 0, remark_col, &format!("{}员工考勤汇总表", month_label), &title_fmt)?;

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
    ws.merge_range(2, summary_col_day, 3, summary_col_day, "白班合计", &header_fmt)?;
    ws.merge_range(2, summary_col_night, 3, summary_col_night, "夜班合计", &header_fmt)?;
    ws.merge_range(2, summary_col_total, 3, summary_col_total, "合计", &header_fmt)?;
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

        ws.write_string_with_format(day_row, summary_col_day, &format!("=COUNTIF({day_col_letter_start}{day_row}:{day_col_letter_end}{day_row},\"√\")"), &cell_fmt)?;
        ws.write_string_with_format(night_row, summary_col_night, &format!("=COUNTIF({night_col_letter_start}{night_row}:{night_col_letter_end}{night_row},\"√\")"), &cell_fmt)?;
        let day_total_col = col_letter(summary_col_day);
        let night_total_col = col_letter(summary_col_night);
        ws.write_string_with_format(day_row, summary_col_total, &format!("={day_total_col}{day_row}+{night_total_col}{night_row}"), &cell_fmt)?;

        // Remark (merged 2 rows)
        ws.merge_range(day_row, remark_col, night_row, remark_col, "", &cell_fmt)?;
    }

    // Bottom legend row
    let legend_row = (4 + employees.len() * 2 + 1) as u32;
    ws.write_string_with_format(legend_row, 0, "标注:", &info_fmt)?;
    ws.write_string_with_format(legend_row, 1, "√=出勤  休=公休  S(+时数)=事假  病=病假", &info_fmt)?;

    // Signature area
    let sign_row = legend_row + 1;
    ws.write_string_with_format(sign_row, 0, "考勤人签字:", &info_fmt)?;
    ws.write_string_with_format(sign_row, 5, "行政经理签字:", &info_fmt)?;
    ws.write_string_with_format(sign_row, 10, "日期:", &info_fmt)?;

    // Column widths
    ws.set_column_width(0, 5)?;    // 序号
    ws.set_column_width(1, 10)?;   // 姓名
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
        if c < 26 { break; }
        c = c / 26 - 1;
    }
    result
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
