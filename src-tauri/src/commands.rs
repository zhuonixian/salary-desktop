use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use rusqlite::Connection;
use tauri::Manager;

use crate::accounting;
use crate::cashier;
use crate::data_safety;
use crate::db;
use crate::errors::{AppError, AppResult};
use crate::excel;
use crate::models::*;
use crate::ocr;
use crate::salary;

fn app_data_dir(app: &tauri::AppHandle) -> Result<PathBuf, AppError> {
    app.path()
        .app_data_dir()
        .map_err(|e| AppError::General(format!("获取应用数据目录失败: {e}")))
}

// ==================== Employee Commands ====================

#[tauri::command]
pub fn get_employees(
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<Employee>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::get_employees(&conn)
}

#[tauri::command]
pub fn get_employee(
    id: i64,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Employee, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::get_employee(&conn, id)
}

#[tauri::command]
pub fn create_employee(
    data: EmployeeInput,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Employee, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let emp = db::create_employee(&conn, &data)?;
    db::log_operation(
        &conn,
        "create_employee",
        &format!("新增员工: {} ({})", emp.name, emp.employee_no),
        "system",
        None,
    )?;
    Ok(emp)
}

#[tauri::command]
pub fn update_employee(
    id: i64,
    data: EmployeeInput,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let result = db::update_employee(&conn, id, &data)?;
    if result {
        db::log_operation(
            &conn,
            "update_employee",
            &format!("更新员工ID={id}"),
            "system",
            None,
        )?;
    }
    Ok(result)
}

#[tauri::command]
pub fn delete_employee(
    id: i64,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let result = db::delete_employee(&conn, id)?;
    if result {
        db::log_operation(
            &conn,
            "delete_employee",
            &format!("删除员工ID={id}"),
            "system",
            None,
        )?;
    }
    Ok(result)
}

#[tauri::command]
pub fn search_employees(
    keyword: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<Employee>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::search_employees(&conn, &keyword)
}

#[tauri::command]
pub fn import_employees_excel(
    path: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<ImportResult, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let employees = excel::read_employee_excel(&path)?;

    let total = employees.len() as i32;
    let mut imported = 0;
    let mut skipped = 0;
    let mut errors = Vec::new();
    let mut seen_employee_no = HashSet::new();

    for emp in &employees {
        let employee_no_key = emp.employee_no.trim().to_lowercase();
        if !seen_employee_no.insert(employee_no_key.clone()) {
            skipped += 1;
            errors.push(format!("工号{}: Excel内工号重复", emp.employee_no));
            continue;
        }
        if db::employee_no_exists(&conn, &emp.employee_no, None)? {
            skipped += 1;
            errors.push(format!("工号{}: 工号已存在", emp.employee_no));
            continue;
        }

        let input = EmployeeInput {
            employee_no: emp.employee_no.clone(),
            name: emp.name.clone(),
            department: emp.department.clone(),
            position: emp.position.clone(),
            id_card: emp.id_card.clone(),
            phone: emp.phone.clone(),
            bank_account: emp.bank_account.clone(),
            bank_name: emp.bank_name.clone(),
            hire_date: emp.hire_date.clone(),
            status: Some(emp.status.clone()),
            base_salary: Some(emp.base_salary),
            position_salary: Some(emp.position_salary),
            performance_salary: Some(emp.performance_salary),
            social_security_base: Some(emp.social_security_base),
            housing_fund_base: Some(emp.housing_fund_base),
            special_deduction: Some(emp.special_deduction),
            remark: emp.remark.clone(),
        };

        match db::create_employee(&conn, &input) {
            Ok(_) => imported += 1,
            Err(e) => {
                skipped += 1;
                errors.push(format!("工号{}: {e}", emp.employee_no));
            }
        }
    }

    db::log_operation(
        &conn,
        "import_employees",
        &format!("导入员工Excel: 总{total}, 成功{imported}, 跳过{skipped}"),
        "system",
        None,
    )?;

    Ok(ImportResult {
        success: true,
        total,
        imported,
        skipped,
        errors,
    })
}

#[tauri::command]
pub fn export_employee_import_template(
    path: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let employees = db::get_employees(&conn)?;
    excel::export_employee_template(&path, &employees)?;
    Ok(true)
}

// ==================== Attendance Commands ====================

#[tauri::command]
pub fn get_attendance_records(
    month: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<AttendanceRecord>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::get_attendance_records(&conn, &month)
}

#[tauri::command]
pub fn import_attendance_excel(
    path: String,
    month: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<ImportResult, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::ensure_month_open(&conn, &month)?;
    let records = excel::read_attendance_excel(&path, &month)?;

    let total = records.len() as i32;
    let mut imported = 0;
    let mut skipped = 0;
    let mut errors = Vec::new();

    for rec in &records {
        let input = AttendanceRecordInput {
            id: None,
            salary_month: rec.salary_month.clone(),
            employee_no: rec.employee_no.clone(),
            name: rec.name.clone(),
            expected_days: Some(rec.expected_days),
            actual_days: Some(rec.actual_days),
            late_count: Some(rec.late_count),
            early_leave_count: Some(rec.early_leave_count),
            personal_leave_days: Some(rec.personal_leave_days),
            sick_leave_days: Some(rec.sick_leave_days),
            absent_days: Some(rec.absent_days),
            overtime_hours: Some(rec.overtime_hours),
            source_type: rec.source_type.clone(),
            ocr_batch_id: rec.ocr_batch_id,
            remark: rec.remark.clone(),
        };

        match db::upsert_attendance_record(&conn, &input) {
            Ok(_) => imported += 1,
            Err(e) => {
                skipped += 1;
                errors.push(format!("工号{}: {e}", rec.employee_no));
            }
        }
    }

    db::log_operation(
        &conn,
        "import_attendance",
        &format!("导入{month}考勤: 总{total}, 成功{imported}, 跳过{skipped}"),
        "system",
        None,
    )?;

    Ok(ImportResult {
        success: true,
        total,
        imported,
        skipped,
        errors,
    })
}

#[tauri::command]
pub fn export_attendance_import_template(
    path: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let employees = db::get_employees(&conn)?;
    excel::export_attendance_template(&path, &employees)?;
    Ok(true)
}

#[tauri::command]
pub fn save_attendance_records(
    records: Vec<AttendanceRecordInput>,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    for rec in &records {
        db::ensure_month_open(&conn, &rec.salary_month)?;
        db::upsert_attendance_record(&conn, rec)?;
    }
    Ok(true)
}

#[tauri::command]
pub fn create_attendance_record(
    data: AttendanceRecordInput,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<AttendanceRecord, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::ensure_month_open(&conn, &data.salary_month)?;
    db::create_attendance_record(&conn, &data)
}

#[tauri::command]
pub fn update_attendance_record(
    id: i64,
    data: AttendanceRecordInput,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let month = if data.salary_month.trim().is_empty() {
        db::get_attendance_record_month(&conn, id)?
    } else {
        data.salary_month.clone()
    };
    db::ensure_month_open(&conn, &month)?;
    db::update_attendance_record(&conn, id, &data)
}

#[tauri::command]
pub fn delete_attendance_record(
    id: i64,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let month = db::get_attendance_record_month(&conn, id)?;
    db::ensure_month_open(&conn, &month)?;
    db::delete_attendance_record(&conn, id)
}

// ==================== Salary Rules Commands ====================

#[tauri::command]
pub fn get_salary_rules(
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<SalaryRule>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::get_salary_rules(&conn)
}

#[tauri::command]
pub fn update_salary_rule(
    id: i64,
    rule_value: f64,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let result = db::update_salary_rule(&conn, id, rule_value)?;
    if result {
        db::log_operation(
            &conn,
            "update_salary_rule",
            &format!("更新工资规则ID={id}, 值={rule_value}"),
            "system",
            None,
        )?;
    }
    Ok(result)
}

#[tauri::command]
pub fn get_tax_rules(state: tauri::State<'_, Mutex<Connection>>) -> Result<Vec<TaxRule>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::get_tax_rules(&conn)
}

#[tauri::command]
pub fn update_tax_rule(
    id: i64,
    data: TaxRuleInput,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let result = db::update_tax_rule(&conn, id, &data)?;
    if result {
        db::log_operation(
            &conn,
            "update_tax_rule",
            &format!("更新税率规则ID={id}"),
            "system",
            None,
        )?;
    }
    Ok(result)
}

// ==================== Salary Calculation Commands ====================

#[tauri::command]
pub fn calculate_salary(
    month: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<SalaryResult>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::ensure_month_open(&conn, &month)?;
    salary::calculate_monthly_salary(&month, &conn)
}

#[tauri::command]
pub fn get_salary_results(
    month: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<SalaryResult>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::get_salary_results(&conn, &month)
}

#[tauri::command]
pub fn update_salary_result(
    id: i64,
    data: SalaryResultUpdate,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let month = db::get_salary_result_month(&conn, id)?;
    db::ensure_month_open(&conn, &month)?;
    db::update_salary_result(&conn, id, &data)
}

#[tauri::command]
pub fn lock_salary_results(
    month: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::ensure_month_open(&conn, &month)?;
    let result = db::lock_salary_results(&conn, &month)?;
    if result {
        db::log_operation(
            &conn,
            "lock_salary",
            &format!("锁定{month}工资"),
            "system",
            None,
        )?;
    }
    Ok(result)
}

#[tauri::command]
pub fn review_salary_results(
    month: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::ensure_month_open(&conn, &month)?;
    let result = db::review_salary_results(&conn, &month)?;
    if result {
        db::log_operation(
            &conn,
            "review_salary",
            &format!("复核{month}工资"),
            "system",
            None,
        )?;
    }
    Ok(result)
}

#[tauri::command]
pub fn recalculate_employee(
    month: String,
    employee_no: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<SalaryResult, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::ensure_month_open(&conn, &month)?;
    salary::recalculate_single(&month, &employee_no, &conn)
}

// ==================== OCR Commands ====================

#[tauri::command]
pub fn ocr_recognize(
    image_path: String,
    month: String,
    mode: Option<String>,
    app: tauri::AppHandle,
    state: tauri::State<'_, Mutex<Connection>>,
    sec: tauri::State<'_, crate::security::SecurityState>,
) -> Result<OcrResult, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::ensure_month_open(&conn, &month)?;
    let resource_dir = app.path().resource_dir().ok();
    let mode = mode.as_deref().unwrap_or("local");
    ocr::ocr_recognize(
        &image_path,
        &month,
        mode,
        &conn,
        resource_dir.as_deref(),
        Some(sec.inner()),
    )
}

#[tauri::command]
pub fn get_ocr_batches(
    month: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<OcrBatch>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::get_ocr_batches(&conn, &month)
}

#[tauri::command]
pub fn confirm_ocr_results(
    batch_id: i64,
    records: Vec<AttendanceRecordInput>,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let months = records
        .iter()
        .map(|record| record.salary_month.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    for month in months {
        db::ensure_month_open(&conn, month)?;
    }
    ocr::confirm_ocr_results(batch_id, &records, &conn)
}

// ==================== Export Commands ====================

#[tauri::command]
pub fn export_salary_detail(
    month: String,
    path: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let results = db::get_salary_results(&conn, &month)?;
    excel::export_salary_excel(&results, &path)?;
    db::log_operation(
        &conn,
        "export",
        &format!("导出{month}工资明细到{path}"),
        "system",
        None,
    )?;
    Ok(true)
}

#[tauri::command]
pub fn export_bank_payment_file(
    month: String,
    path: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let results = db::get_salary_results(&conn, &month)?;
    excel::export_bank_payment(&results, &path)?;
    db::log_operation(
        &conn,
        "export",
        &format!("导出{month}银行代发到{path}"),
        "system",
        None,
    )?;
    Ok(true)
}

#[tauri::command]
pub fn export_salary_slips(
    month: String,
    dir: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let results = db::get_salary_results(&conn, &month)?;

    for result in &results {
        let name = result.name.as_deref().unwrap_or("unknown");
        let filename = format!("{dir}/{month}_{name}_工资条.xlsx");
        excel::export_salary_slip(result, &filename)?;
    }

    db::log_operation(
        &conn,
        "export",
        &format!("导出{month}个人工资条{}份到{dir}", results.len()),
        "system",
        None,
    )?;

    Ok(true)
}

#[tauri::command]
pub fn export_attendance_summary_file(
    month: String,
    path: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let records = db::get_attendance_records(&conn, &month)?;
    excel::export_attendance_summary(&records, &path)?;
    db::log_operation(
        &conn,
        "export",
        &format!("导出{month}考勤汇总到{path}"),
        "system",
        None,
    )?;
    Ok(true)
}

// ==================== Dashboard Command ====================

#[tauri::command]
pub fn get_dashboard_summary(
    month: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<DashboardSummary, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::get_dashboard_summary(&conn, &month)
}

#[tauri::command]
pub fn get_month_close_workbench(
    month: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<MonthCloseWorkbench, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::get_month_close_workbench(&conn, &month)
}

#[tauri::command]
pub fn get_month_close_status(
    month: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Option<MonthCloseRecord>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::get_month_close_record(&conn, &month)
}

#[tauri::command]
pub fn close_month(
    data: MonthCloseInput,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<MonthCloseRecord, AppError> {
    let mut conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let tx = conn.transaction()?;
    let result = db::close_month(&tx, &data.month, "system", data.remark.as_deref())?;
    // 12 月正式月结后自动生成年末损益结转凭证（幂等）
    if data.month.ends_with("-12") {
        let n = accounting::generate_period_close_vouchers(&tx, &data.month)?;
        if n > 0 {
            db::log_operation(
                &tx,
                "period_close_vouchers",
                &format!("{} 年末结转凭证 {} 张", data.month, n),
                "system",
                None,
            )?;
        }
    }
    db::log_operation(
        &tx,
        "close_month",
        &format!("正式月结{}", result.month),
        "system",
        data.remark.as_deref(),
    )?;
    tx.commit()?;
    Ok(result)
}

#[tauri::command]
pub fn reopen_month(
    data: MonthReopenInput,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<MonthCloseRecord, AppError> {
    let mut conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let tx = conn.transaction()?;
    // 反月结前先批量作废该月年末结转凭证，保持数据一致
    if data.month.ends_with("-12") {
        accounting::void_period_close_vouchers(&tx, &data.month)?;
    }
    let result = db::reopen_month(&tx, &data.month, &data.reason)?;
    db::log_operation(
        &tx,
        "reopen_month",
        &format!("反月结{}", result.month),
        "system",
        Some(&data.reason),
    )?;
    tx.commit()?;
    Ok(result)
}

#[tauri::command]
pub fn get_financial_analysis(
    query: FinancialAnalysisQuery,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<FinancialAnalysisReport, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::get_financial_analysis(&conn, &query)
}

#[tauri::command]
pub fn export_department_cost_report(
    query: FinancialAnalysisQuery,
    path: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let report = db::get_financial_analysis(&conn, &query)?;
    excel::export_department_cost_analysis(&report.department_costs, &report.month, &path)?;
    db::log_operation(
        &conn,
        "export_department_cost_report",
        &format!("导出{}部门成本表到{}", report.month, path),
        "system",
        None,
    )?;
    Ok(true)
}

#[tauri::command]
pub fn export_expense_analysis_report(
    query: FinancialAnalysisQuery,
    path: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let report = db::get_financial_analysis(&conn, &query)?;
    excel::export_expense_analysis_report(&report, &path)?;
    db::log_operation(
        &conn,
        "export_expense_analysis_report",
        &format!("导出{}费用分析表到{}", report.month, path),
        "system",
        None,
    )?;
    Ok(true)
}

#[tauri::command]
pub fn export_month_close_report(
    query: FinancialAnalysisQuery,
    path: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let report = db::get_financial_analysis(&conn, &query)?;
    let workbench = db::get_month_close_workbench(&conn, &query.month)?;
    excel::export_month_close_report(&report, &workbench, &path)?;
    db::log_operation(
        &conn,
        "export_month_close_report",
        &format!("导出{}月结报告到{}", report.month, path),
        "system",
        None,
    )?;
    Ok(true)
}

#[tauri::command]
pub fn export_month_close_package(
    month: String,
    dir: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<MonthClosePackageResult, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    export_month_close_package_to_dir(&conn, &month, &dir)
}

pub fn export_month_close_package_to_dir(
    conn: &Connection,
    month: &str,
    dir: &str,
) -> Result<MonthClosePackageResult, AppError> {
    let month_close = db::get_month_close_record(conn, month)?
        .ok_or_else(|| AppError::InvalidParam(format!("{month} 尚未正式月结")))?;
    if month_close.status != "closed" {
        return Err(AppError::InvalidParam(format!(
            "{month} 当前不是已月结状态，不能导出月结包"
        )));
    }

    let output_dir = PathBuf::from(&dir).join(format!("{month}-month-close-package"));
    fs::create_dir_all(&output_dir)?;

    let report = db::get_financial_analysis(
        conn,
        &FinancialAnalysisQuery {
            month: month.to_string(),
            months: Some(6),
        },
    )?;
    let workbench = db::get_month_close_workbench(conn, month)?;
    let salary_results = db::get_salary_results(conn, month)?;
    let invoices = db::query_invoices(
        conn,
        &InvoiceQuery {
            belong_month: Some(month.to_string()),
            ..Default::default()
        },
    )?;
    let reimbursements = db::query_reimbursement_claims(
        conn,
        &ReimbursementQuery {
            belong_month: Some(month.to_string()),
            ..Default::default()
        },
    )?;
    let paid_payment_batches = db::query_payment_batches(
        conn,
        &PaymentBatchQuery {
            belong_month: Some(month.to_string()),
            status: Some("paid".to_string()),
            ..Default::default()
        },
    )?;

    let mut files = Vec::new();
    let month_close_report = output_dir.join(format!("{month}_月结报告.xlsx"));
    excel::export_month_close_report(&report, &workbench, &month_close_report.to_string_lossy())?;
    files.push(month_close_report.to_string_lossy().to_string());

    let salary_detail = output_dir.join(format!("{month}_工资明细.xlsx"));
    excel::export_salary_excel(&salary_results, &salary_detail.to_string_lossy())?;
    files.push(salary_detail.to_string_lossy().to_string());

    let bank_payment = output_dir.join(format!("{month}_银行代发.xlsx"));
    excel::export_bank_payment(&salary_results, &bank_payment.to_string_lossy())?;
    files.push(bank_payment.to_string_lossy().to_string());

    let invoice_list = output_dir.join(format!("{month}_发票清单.xlsx"));
    excel::export_invoice_list(&invoices, &invoice_list.to_string_lossy())?;
    files.push(invoice_list.to_string_lossy().to_string());

    let reimbursement_list = output_dir.join(format!("{month}_报销清单.xlsx"));
    excel::export_reimbursement_claim_list(&reimbursements, &reimbursement_list.to_string_lossy())?;
    files.push(reimbursement_list.to_string_lossy().to_string());

    for batch in paid_payment_batches {
        let detail = db::get_payment_batch_detail(conn, batch.id)?;
        let batch_file = output_dir.join(format!("{month}_{}_付款明细.xlsx", batch.batch_no));
        excel::export_payment_batch(&detail, &batch_file.to_string_lossy())?;
        files.push(batch_file.to_string_lossy().to_string());
    }

    let manifest_path = output_dir.join("manifest.json");
    let result_files = files.clone();
    let manifest = serde_json::json!({
        "month": month,
        "status": month_close.status,
        "closed_at": month_close.closed_at,
        "closed_by": month_close.closed_by,
        "files": result_files,
    });
    fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;
    let manifest_path_str = manifest_path.to_string_lossy().to_string();
    let output_dir_str = output_dir.to_string_lossy().to_string();
    let mut result_files = files;
    result_files.push(manifest_path_str);

    db::log_operation(
        conn,
        "export_month_close_package",
        &format!("导出{}月结包到{}", month_close.month, output_dir_str),
        "system",
        None,
    )?;

    Ok(MonthClosePackageResult {
        success: true,
        output_dir: output_dir_str,
        files: result_files,
    })
}

#[tauri::command]
pub fn query_payment_batches(
    query: PaymentBatchQuery,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<PaymentBatch>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::query_payment_batches(&conn, &query)
}

#[tauri::command]
pub fn get_payment_batch_detail(
    id: i64,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<PaymentBatchDetail, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::get_payment_batch_detail(&conn, id)
}

#[tauri::command]
pub fn create_payment_batch(
    data: PaymentBatchInput,
    state: tauri::State<'_, Mutex<Connection>>,
    current: tauri::State<'_, crate::cashier::CurrentOperatorState>,
) -> Result<PaymentBatchDetail, AppError> {
    let mut conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let detail = db::create_payment_batch(&mut conn, &data, &current)?;
    db::log_operation(
        &conn,
        "create_payment_batch",
        &format!(
            "生成{}付款批次{}，{}笔，金额{:.2}",
            match detail.batch.batch_type.as_str() {
                "salary" => "工资",
                "reimbursement" => "报销",
                _ => "通用",
            },
            detail.batch.batch_no,
            detail.batch.item_count,
            detail.batch.total_amount
        ),
        &crate::cashier::current_operator_name(&conn, &current),
        data.remark.as_deref(),
    )?;
    Ok(detail)
}

#[tauri::command]
pub fn export_payment_batch_file(
    id: i64,
    path: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<PaymentBatch, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let detail = db::get_payment_batch_detail(&conn, id)?;
    // 先跑导出门禁（月份开放/未作废/已指定资金账户），避免门禁拒绝时已写出孤儿文件
    db::ensure_payment_batch_exportable(&conn, id)?;
    excel::export_payment_batch(&detail, &path)?;
    let batch = db::mark_payment_batch_exported(&conn, id)?;
    db::log_operation(
        &conn,
        "export_payment_batch",
        &format!("导出付款批次{}到{}", batch.batch_no, path),
        "system",
        None,
    )?;
    Ok(batch)
}

#[tauri::command]
pub fn mark_payment_batch_paid(
    data: PaymentBatchPaidInput,
    state: tauri::State<'_, Mutex<Connection>>,
    current: tauri::State<'_, crate::cashier::CurrentOperatorState>,
) -> Result<PaymentBatch, AppError> {
    let mut conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let batch = db::mark_payment_batch_paid(&mut conn, &data, &current)?;
    db::log_operation(
        &conn,
        "mark_payment_batch_paid",
        &format!("标记付款批次{}已付款", batch.batch_no),
        &crate::cashier::current_operator_name(&conn, &current),
        Some(&data.payment_date),
    )?;
    Ok(batch)
}

#[tauri::command]
pub fn void_payment_batch(
    data: PaymentBatchVoidInput,
    state: tauri::State<'_, Mutex<Connection>>,
    current: tauri::State<'_, crate::cashier::CurrentOperatorState>,
) -> Result<PaymentBatch, AppError> {
    let mut conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let batch = db::void_payment_batch(&mut conn, &data, &current)?;
    db::log_operation(
        &conn,
        "void_payment_batch",
        &format!("作废付款批次{}", batch.batch_no),
        &crate::cashier::current_operator_name(&conn, &current),
        Some(&data.reason),
    )?;
    Ok(batch)
}

#[tauri::command]
pub fn update_payment_batch_remark(
    data: PaymentBatchRemarkInput,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<PaymentBatch, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let batch = db::update_payment_batch_remark(&conn, &data)?;
    db::log_operation(
        &conn,
        "update_payment_batch_remark",
        &format!("更新付款批次{}备注", batch.batch_no),
        "system",
        data.remark.as_deref(),
    )?;
    Ok(batch)
}

/// 银行流水导入预览（Task 11，spec 4.8）：解析结果先行展示，确认后才入库；只读不落库
#[tauri::command]
pub fn preview_bank_transaction_import(
    path: String,
    fund_account_id: i64,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<BankImportPreview, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::preview_bank_transaction_import(&conn, &path, fund_account_id)
}

/// 银行流水导入：必须指定 bank/third_party 资金账户（spec 4.8）。
/// 前端应先 preview 确认，本命令按行落库（重复行跳过、月结行拦截进 errors）。
#[tauri::command]
pub fn import_bank_transactions_file(
    path: String,
    fund_account_id: i64,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<ImportResult, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    // 账户前置校验（现金/停用直接失败，不进入逐行循环）
    let account = db::ensure_bank_import_account(&conn, fund_account_id)?;
    let mut transactions = excel::read_bank_transactions_file(&path)?;
    for tx in &mut transactions {
        tx.fund_account_id = Some(fund_account_id);
    }
    let total = transactions.len() as i32;
    let mut imported = 0;
    let mut skipped = 0;
    let mut errors = Vec::new();

    for tx in &transactions {
        match db::insert_bank_transaction(&conn, tx) {
            Ok(true) => imported += 1,
            Ok(false) => {
                skipped += 1;
                errors.push(format!(
                    "{} {}: 重复流水已跳过",
                    tx.transaction_date,
                    tx.summary.as_deref().unwrap_or("")
                ));
            }
            Err(e) => {
                skipped += 1;
                errors.push(format!(
                    "{} {}: {e}",
                    tx.transaction_date,
                    tx.summary.as_deref().unwrap_or("")
                ));
            }
        }
    }

    db::log_operation(
        &conn,
        "import_bank_transactions",
        &format!(
            "导入银行流水[账户{}]: 总{total}, 成功{imported}, 跳过{skipped}",
            account.name
        ),
        "system",
        Some(&path),
    )?;

    Ok(ImportResult {
        success: true,
        total,
        imported,
        skipped,
        errors,
    })
}

#[tauri::command]
pub fn query_bank_transactions(
    query: BankTransactionQuery,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<BankTransaction>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::query_bank_transactions(&conn, &query)
}

#[tauri::command]
pub fn auto_match_bank_transactions(
    month: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<BankAutoMatchResult, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let result = db::auto_match_bank_transactions(&conn, &month)?;
    db::log_operation(
        &conn,
        "auto_match_bank_transactions",
        &format!(
            "自动匹配{}银行流水: 成功{}, 跳过{}",
            month, result.matched, result.skipped
        ),
        "system",
        None,
    )?;
    Ok(result)
}

#[tauri::command]
pub fn confirm_bank_transaction_match(
    data: BankTransactionMatchInput,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<BankTransactionMatch, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let result = db::confirm_bank_transaction_match(&conn, &data, 100)?;
    db::log_operation(
        &conn,
        "confirm_bank_transaction_match",
        &format!(
            "确认银行流水ID={}匹配付款批次ID={}",
            data.transaction_id, data.payment_batch_id
        ),
        "system",
        data.remark.as_deref(),
    )?;
    Ok(result)
}

#[tauri::command]
pub fn cancel_bank_transaction_match(
    transaction_id: i64,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let result = db::cancel_bank_transaction_match(&conn, transaction_id)?;
    if result {
        db::log_operation(
            &conn,
            "cancel_bank_transaction_match",
            &format!("取消银行流水ID={transaction_id}匹配"),
            "system",
            None,
        )?;
    }
    Ok(result)
}

#[tauri::command]
pub fn ignore_bank_transaction(
    data: BankTransactionIgnoreInput,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let result = db::ignore_bank_transaction(&conn, &data)?;
    if result {
        db::log_operation(
            &conn,
            "ignore_bank_transaction",
            &format!("忽略银行流水ID={}", data.transaction_id),
            "system",
            Some(&data.reason),
        )?;
    }
    Ok(result)
}

// ==================== Task 12：银行流水多对多核销（spec 4.9/6.2/6.3） ====================

#[tauri::command]
pub fn preview_bank_allocation_candidates(
    transaction_id: i64,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<BankAutoMatchPreviewItem, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    cashier::preview_bank_allocation_candidates(&conn, transaction_id)
}

#[tauri::command]
pub fn preview_bank_auto_matches(
    month: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<BankAutoMatchPreviewItem>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    cashier::preview_bank_auto_matches(&conn, &month)
}

#[tauri::command]
pub fn confirm_bank_allocations(
    data: Vec<BankAllocationInput>,
    match_method: Option<String>,
    state: tauri::State<'_, Mutex<Connection>>,
    current: tauri::State<'_, cashier::CurrentOperatorState>,
) -> Result<BankAllocationBatchResult, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let method = match_method.as_deref().unwrap_or("manual");
    let result = cashier::confirm_bank_allocations(
        &conn,
        &data,
        method,
        &cashier::current_operator_name(&conn, &current),
    )?;
    // 单项人工确认失败时直接报错透出（批量时逐项跳过不阻塞）
    if result.confirmed == 0 && !result.errors.is_empty() {
        return Err(AppError::InvalidParam(result.errors.join("；")));
    }
    if result.confirmed > 0 {
        db::log_operation(
            &conn,
            "confirm_bank_allocations",
            &format!(
                "银行流水核销：成功 {} 条，跳过 {} 条（方式 {method}）",
                result.confirmed, result.skipped
            ),
            &cashier::current_operator_name(&conn, &current),
            data.iter()
                .find(|i| i.remark.as_deref().is_some_and(|r| !r.trim().is_empty()))
                .and_then(|i| i.remark.clone())
                .as_deref(),
        )?;
    }
    Ok(result)
}

#[tauri::command]
pub fn cancel_bank_allocation(
    allocation_id: i64,
    state: tauri::State<'_, Mutex<Connection>>,
    current: tauri::State<'_, cashier::CurrentOperatorState>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let operator = cashier::current_operator_name(&conn, &current);
    let result = cashier::cancel_bank_allocation(&conn, allocation_id, &operator)?;
    if result {
        db::log_operation(
            &conn,
            "cancel_bank_allocation",
            &format!("取消银行流水核销 ID={allocation_id}"),
            &operator,
            None,
        )?;
    }
    Ok(result)
}

#[tauri::command]
pub fn list_bank_allocations(
    query: BankAllocationQuery,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<BankReconciliationAllocation>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    cashier::list_bank_allocations(&conn, &query)
}

#[tauri::command]
pub fn batch_confirm_bank_auto_matches(
    month: String,
    min_score: Option<i32>,
    state: tauri::State<'_, Mutex<Connection>>,
    current: tauri::State<'_, cashier::CurrentOperatorState>,
) -> Result<BankAllocationBatchResult, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let operator = cashier::current_operator_name(&conn, &current);
    let threshold = min_score.unwrap_or(0);
    let result = cashier::batch_confirm_bank_auto_matches(&conn, &month, threshold, &operator)?;
    db::log_operation(
        &conn,
        "batch_confirm_bank_auto_matches",
        &format!(
            "自动匹配批量核销 {month}：确认 {} 条，跳过 {} 条（置信线 {}）{}",
            result.confirmed,
            result.skipped,
            threshold,
            if result.errors.is_empty() {
                String::new()
            } else {
                format!("；失败：{}", result.errors.join("；"))
            }
        ),
        &operator,
        None,
    )?;
    Ok(result)
}

#[tauri::command]
pub fn migrate_legacy_bank_matches(
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<LegacyBankMatchReport, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let report = db::migrate_legacy_bank_matches(&conn)?;
    db::log_operation(
        &conn,
        "migrate_legacy_bank_matches",
        &format!(
            "旧银行匹配迁移：旧表 {} 行（active {}），迁入 {} 条，幂等跳过 {}，未转换 {}",
            report.total,
            report.active_total,
            report.migrated,
            report.already_migrated,
            report.unconverted.len()
        ),
        "system",
        None,
    )?;
    Ok(report)
}

// ==================== Task 13：资金日记账与银行余额调节表（spec 6.1/4.10） ====================

#[tauri::command]
pub fn get_fund_journal(
    query: FundJournalQuery,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<FundJournal, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    cashier::get_fund_journal(&conn, &query)
}

#[tauri::command]
pub fn export_fund_journal(
    query: FundJournalQuery,
    path: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<String, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let journal = cashier::get_fund_journal(&conn, &query)?;
    excel::export_fund_journal_excel(&journal, &path)?;
    db::log_operation(
        &conn,
        "export_fund_journal",
        &format!(
            "导出资金日记账（{} {}~{}）到{path}",
            journal.fund_account_name,
            journal.from_month.as_deref().unwrap_or("期初"),
            journal.to_month.as_deref().unwrap_or("最新")
        ),
        "system",
        None,
    )?;
    Ok(path)
}

// ==================== Task 14：员工借款备用金与核销（spec 4.11） ====================

/// 借款台账：按借款单聚合未核销余额、逾期天数与账龄（0-30/31-60/61-90/90+）
#[tauri::command]
pub fn get_advance_ledger(
    query: AdvanceLedgerQuery,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<AdvanceLedger, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    cashier::get_advance_ledger(&conn, &query)
}

/// 核销时间线：某借款单的全部核销记录（含已取消），按时间正序
#[tauri::command]
pub fn get_advance_settlement_links(
    advance_id: i64,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<AdvanceSettlementLink>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    cashier::get_advance_settlement_links(&conn, advance_id)
}

/// 取消核销：未结算核销单→作废；已结算→冲正（需冲正归属月/日期）。
/// 取消后借款未核销余额恢复，联动作废/冲正凭证（spec 4.11）。
#[tauri::command]
pub fn cancel_advance_settlement_link(
    data: AdvanceLinkCancelInput,
    state: tauri::State<'_, Mutex<Connection>>,
    current: tauri::State<'_, cashier::CurrentOperatorState>,
) -> Result<FundDocument, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let doc = cashier::cancel_advance_settlement_link(&conn, &current, &data)?;
    db::log_operation(
        &conn,
        "cancel_advance_settlement_link",
        &format!(
            "取消借款核销（记录ID={}），核销单流转为 {} {}",
            data.link_id,
            cashier::fund_status_label(&doc.status),
            doc.document_no
        ),
        &cashier::current_operator_name(&conn, &current),
        Some(&format!(
            "link_id={} reason={}",
            data.link_id,
            data.reason.trim()
        )),
    )?;
    Ok(doc)
}

/// 导出借款备用金台账 Excel（含未核销余额、逾期与账龄）
#[tauri::command]
pub fn export_advance_ledger(
    query: AdvanceLedgerQuery,
    path: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<String, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let ledger = cashier::get_advance_ledger(&conn, &query)?;
    excel::export_advance_ledger_excel(&ledger, &path)?;
    db::log_operation(
        &conn,
        "export_advance_ledger",
        &format!(
            "导出借款备用金台账（{} 笔，未核销余额 {:.2}）到{path}",
            ledger.rows.len(),
            ledger.total_outstanding
        ),
        "system",
        None,
    )?;
    Ok(path)
}

#[tauri::command]
pub fn generate_bank_reconciliation_period(
    fund_account_id: i64,
    month: String,
    statement_opening: Option<f64>,
    statement_closing: Option<f64>,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<BankReconciliationPeriod, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let period = cashier::generate_bank_reconciliation_period(
        &conn,
        fund_account_id,
        &month,
        statement_opening,
        statement_closing,
    )?;
    db::log_operation(
        &conn,
        "generate_bank_reconciliation_period",
        &format!(
            "生成{}{}余额调节表：账面期末 {:.2}，对账单期末 {:.2}，调节差额 {:.2}",
            period.fund_account_name.as_deref().unwrap_or(""),
            period.belong_month,
            period.book_closing_balance,
            period.statement_closing_balance,
            period.difference
        ),
        "system",
        None,
    )?;
    Ok(period)
}

#[tauri::command]
pub fn confirm_bank_reconciliation_period(
    id: i64,
    state: tauri::State<'_, Mutex<Connection>>,
    current: tauri::State<'_, cashier::CurrentOperatorState>,
) -> Result<BankReconciliationPeriod, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let operator = cashier::current_operator_name(&conn, &current);
    let period = cashier::confirm_bank_reconciliation_period(&conn, id, &operator)?;
    db::log_operation(
        &conn,
        "confirm_bank_reconciliation_period",
        &format!(
            "确认{}{}余额调节表（调节差额 {:.2}）",
            period.fund_account_name.as_deref().unwrap_or(""),
            period.belong_month,
            period.difference
        ),
        &operator,
        None,
    )?;
    Ok(period)
}

#[tauri::command]
pub fn list_bank_reconciliation_periods(
    fund_account_id: Option<i64>,
    month: Option<String>,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<BankReconciliationPeriod>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    cashier::list_bank_reconciliation_periods(&conn, fund_account_id, month.as_deref())
}

#[tauri::command]
pub fn export_bank_reconciliation_period(
    id: i64,
    path: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<String, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let period = cashier::get_bank_reconciliation_period(&conn, id)?;
    excel::export_bank_reconciliation_excel(&period, &path)?;
    db::log_operation(
        &conn,
        "export_bank_reconciliation_period",
        &format!(
            "导出{}{}银行余额调节表到{path}",
            period.fund_account_name.as_deref().unwrap_or(""),
            period.belong_month
        ),
        "system",
        None,
    )?;
    Ok(path)
}

#[tauri::command]
pub fn query_budgets(
    query: BudgetQuery,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<Budget>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::query_budgets(&conn, &query)
}

#[tauri::command]
pub fn save_budget(
    data: BudgetInput,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Budget, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let budget = db::save_budget(&conn, &data)?;
    db::log_operation(
        &conn,
        "save_budget",
        &format!("保存{}预算，金额{:.2}", budget.month, budget.budget_amount),
        "system",
        data.remark.as_deref(),
    )?;
    Ok(budget)
}

#[tauri::command]
pub fn delete_budget(
    id: i64,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let result = db::delete_budget(&conn, id)?;
    if result {
        db::log_operation(
            &conn,
            "delete_budget",
            &format!("删除预算ID={id}"),
            "system",
            None,
        )?;
    }
    Ok(result)
}

#[tauri::command]
pub fn query_operation_logs(
    query: OperationLogQuery,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<OperationLog>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::query_operation_logs(&conn, &query)
}

// ==================== Data Safety Commands ====================

#[tauri::command]
pub fn get_data_safety_status(
    app: tauri::AppHandle,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<DataSafetyStatus, AppError> {
    let app_data_dir = app_data_dir(&app)?;
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    data_safety::get_status(&conn, &app_data_dir)
}

#[tauri::command]
pub fn backup_database(
    target_dir: String,
    encrypt: bool,
    app: tauri::AppHandle,
    state: tauri::State<'_, Mutex<Connection>>,
    sec: tauri::State<'_, crate::security::SecurityState>,
) -> Result<DataBackupResult, AppError> {
    let app_data_dir = app_data_dir(&app)?;
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let result = data_safety::backup_database(
        &conn,
        &app_data_dir,
        target_dir.as_ref(),
        encrypt,
        sec.inner(),
    )?;
    db::set_setting(&conn, "last_data_backup_at", &result.created_at)?;
    db::set_setting(&conn, "last_data_backup_path", &result.backup_dir)?;
    db::log_operation(
        &conn,
        "backup_database",
        &format!("备份数据库到{}", result.backup_dir),
        "system",
        None,
    )?;
    Ok(result)
}

#[tauri::command]
pub fn restore_database(
    backup_dir: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, Mutex<Connection>>,
    sec: tauri::State<'_, crate::security::SecurityState>,
) -> Result<DataRestoreResult, AppError> {
    let app_data_dir = app_data_dir(&app)?;
    let mut conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let result =
        data_safety::restore_database(&mut conn, &app_data_dir, backup_dir.as_ref(), sec.inner())?;
    db::log_operation(
        &conn,
        "restore_database",
        &format!("从{}恢复数据库", result.restored_from),
        "system",
        Some(&format!("自动保护备份: {}", result.safety_backup_dir)),
    )?;
    Ok(result)
}

#[tauri::command]
pub fn verify_database(
    app: tauri::AppHandle,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<DataSafetyCheckResult, AppError> {
    let app_data_dir = app_data_dir(&app)?;
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    // 传入 app_data_dir：体检同时覆盖附件目录一致性（孤儿文件/缺失文件，spec 4.6）
    let result = data_safety::verify_database(&conn, Some(&app_data_dir))?;
    db::log_operation(
        &conn,
        "verify_database",
        if result.ok {
            "数据库体检通过"
        } else {
            "数据库体检发现异常"
        },
        "system",
        Some(&result.messages.join("\n")),
    )?;
    Ok(result)
}

#[tauri::command]
pub fn compact_database(state: tauri::State<'_, Mutex<Connection>>) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    data_safety::compact_database(&conn)?;
    db::log_operation(
        &conn,
        "compact_database",
        "压缩并整理数据库",
        "system",
        None,
    )?;
    Ok(true)
}

#[tauri::command]
pub fn open_app_data_dir(app: tauri::AppHandle) -> Result<bool, AppError> {
    let app_data_dir = app_data_dir(&app)?;
    data_safety::open_app_data_dir(&app_data_dir)
}

// ==================== OCR Settings Commands ====================

#[tauri::command]
pub fn get_ocr_settings(
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<OcrSettings, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    ocr::get_ocr_settings(&conn)
}

#[tauri::command]
pub fn save_ocr_settings(
    data: OcrSettingsInput,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    ocr::save_ocr_settings(&conn, &data)
}

// ==================== Punch Card Commands ====================

#[tauri::command]
pub fn generate_punch_card_template(
    path: String,
    month: String,
    department: Option<String>,
    position: Option<String>,
    shift_type: Option<String>,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let employees = db::get_employees(&conn)?;
    let dept = department.unwrap_or_default();
    let _pos = position.unwrap_or_default();
    let shift = shift_type.as_deref().unwrap_or("day");
    excel::export_punch_card_template(&path, &month, &dept, shift, &employees)?;
    Ok(true)
}

#[tauri::command]
pub fn ocr_recognize_punch_card(
    image_path: String,
    month: String,
    shift_type: Option<String>,
    mode: Option<String>,
    conn: tauri::State<'_, Mutex<Connection>>,
    sec: tauri::State<'_, crate::security::SecurityState>,
) -> Result<OcrResult, AppError> {
    let connection = conn.lock().map_err(|e| AppError::General(e.to_string()))?;
    let shift = shift_type.as_deref().unwrap_or("day");
    let m = mode.as_deref().unwrap_or("online");
    ocr::ocr_recognize_punch_card(
        &image_path,
        &month,
        shift,
        m,
        &connection,
        Some(sec.inner()),
    )
}

// ==================== Invoice Commands ====================

#[tauri::command]
pub fn get_invoice_expense_types(
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<InvoiceExpenseType>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::get_invoice_expense_types(&conn)
}

#[tauri::command]
pub fn save_invoice_expense_type(
    data: InvoiceExpenseTypeInput,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<InvoiceExpenseType, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    if let Some(id) = data.id {
        let result = db::update_invoice_expense_type(&conn, id, &data)?;
        db::log_operation(
            &conn,
            "update_expense_type",
            &format!("更新费用类型: {}", result.name),
            "system",
            None,
        )?;
        Ok(result)
    } else {
        let result = db::insert_invoice_expense_type(&conn, &data)?;
        db::log_operation(
            &conn,
            "create_expense_type",
            &format!("新增费用类型: {}", result.name),
            "system",
            None,
        )?;
        Ok(result)
    }
}

#[tauri::command]
pub fn delete_invoice_expense_type(
    id: i64,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let result = db::delete_invoice_expense_type(&conn, id)?;
    if result {
        db::log_operation(
            &conn,
            "delete_expense_type",
            &format!("删除费用类型ID={id}"),
            "system",
            None,
        )?;
    }
    Ok(result)
}

#[tauri::command]
pub fn ocr_invoice(
    image_path: String,
    _app: tauri::AppHandle,
    state: tauri::State<'_, Mutex<Connection>>,
    sec: tauri::State<'_, crate::security::SecurityState>,
) -> Result<InvoiceOcrPreview, AppError> {
    crate::invoice::ocr_invoice(&image_path, state.inner(), Some(sec.inner()))
}

#[tauri::command]
pub fn save_invoice(
    data: InvoiceInput,
    app: tauri::AppHandle,
    state: tauri::State<'_, Mutex<Connection>>,
    sec: tauri::State<'_, crate::security::SecurityState>,
) -> Result<Invoice, AppError> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::General(format!("获取app_data_dir失败: {e}")))?;
    crate::invoice::save_invoice_with_mutex(&data, state.inner(), &app_data_dir, Some(sec.inner()))
}

#[tauri::command]
pub fn update_invoice(
    id: i64,
    data: InvoiceInput,
    app: tauri::AppHandle,
    state: tauri::State<'_, Mutex<Connection>>,
    sec: tauri::State<'_, crate::security::SecurityState>,
) -> Result<bool, AppError> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::General(format!("获取app_data_dir失败: {e}")))?;
    crate::invoice::update_invoice_with_mutex(
        id,
        &data,
        state.inner(),
        &app_data_dir,
        Some(sec.inner()),
    )
}

#[tauri::command]
pub fn delete_invoice(
    id: i64,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    crate::invoice::delete_invoice(id, &conn)
}

#[tauri::command]
pub fn query_invoices(
    query: InvoiceQuery,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<Invoice>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::query_invoices(&conn, &query)
}

#[tauri::command]
pub fn export_invoice_list(
    query: InvoiceQuery,
    path: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let invoices = db::query_invoices(&conn, &query)?;
    excel::export_invoice_list(&invoices, &path)?;
    db::log_operation(
        &conn,
        "export_invoices",
        &format!("导出发票清单: {}条到{}", invoices.len(), path),
        "system",
        None,
    )?;
    Ok(true)
}

// ==================== Reimbursement Commands ====================

#[tauri::command]
pub fn query_reimbursement_claims(
    query: ReimbursementQuery,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<ReimbursementClaim>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::query_reimbursement_claims(&conn, &query)
}

#[tauri::command]
pub fn save_reimbursement_claim(
    data: ReimbursementClaimInput,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<ReimbursementClaim, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let is_update = data.id.is_some();
    let result = db::save_reimbursement_claim(&conn, &data)?;
    db::log_operation(
        &conn,
        if is_update {
            "update_reimbursement"
        } else {
            "create_reimbursement"
        },
        &format!(
            "{}报销单: {}，金额{:.2}",
            if is_update { "更新" } else { "新增" },
            result.claim_no,
            result.total_amount
        ),
        "system",
        None,
    )?;
    Ok(result)
}

#[tauri::command]
pub fn get_reimbursement_invoices(
    claim_id: i64,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<ReimbursementInvoice>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::get_reimbursement_invoices(&conn, claim_id)
}

#[tauri::command]
pub fn update_reimbursement_claim_status(
    id: i64,
    status: Option<String>,
    payment_status: Option<String>,
    payment_date: Option<String>,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let result = db::update_reimbursement_claim_status(
        &conn,
        id,
        status.clone(),
        payment_status.clone(),
        payment_date.clone(),
    )?;
    if result {
        db::log_operation(
            &conn,
            "update_reimbursement_status",
            &format!("更新报销单ID={id}状态: {:?} / {:?}", status, payment_status),
            "system",
            None,
        )?;
    }
    Ok(result)
}

#[tauri::command]
pub fn delete_reimbursement_claim(
    id: i64,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let result = db::soft_delete_reimbursement_claim(&conn, id)?;
    if result {
        db::log_operation(
            &conn,
            "delete_reimbursement",
            &format!("作废报销单ID={id}"),
            "system",
            None,
        )?;
    }
    Ok(result)
}

// ==================== Accounting Commands ====================

#[tauri::command]
pub fn get_gl_accounts(
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<GlAccount>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    Ok(accounting::get_accounts(&conn)?)
}

#[tauri::command]
pub fn create_gl_account(
    data: GlAccountInput,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<GlAccount, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    accounting::create_account(&conn, &data)
}

#[tauri::command]
pub fn set_gl_account_active(
    code: String,
    active: bool,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    accounting::set_account_active(&conn, &code, active)
}

#[tauri::command]
pub fn get_opening_balances(
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<OpeningBalanceState, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let (month, rows) = accounting::get_opening_balances(&conn)?;
    Ok(OpeningBalanceState { month, rows })
}

#[tauri::command]
pub fn save_opening_balances(
    month: String,
    rows: Vec<OpeningBalanceRow>,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    accounting::save_opening_balances(&conn, &month, &rows)?;
    db::log_operation(
        &conn,
        "save_opening_balances",
        &format!("保存{month}期初余额"),
        "system",
        None,
    )?;
    Ok(true)
}

#[tauri::command]
pub fn get_account_mappings(
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<AccountMapping>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    Ok(accounting::get_account_mappings(&conn)?)
}

#[tauri::command]
pub fn save_account_mapping(
    data: AccountMappingInput,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<AccountMapping, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    accounting::save_account_mapping(&conn, &data)
}

#[tauri::command]
pub fn delete_account_mapping(
    id: i64,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    accounting::delete_account_mapping(&conn, id)
}

/// 未匹配银行流水手工指定科目生成凭证（bank_manual）。
/// fund_account_id 为资金账户（spec 4.7：手工凭证资金行必须带资金辅助核算）。
#[tauri::command]
pub fn create_bank_manual_voucher(
    transaction_id: i64,
    account_code: String,
    fund_account_id: i64,
    summary: Option<String>,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Voucher, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let v = accounting::create_bank_manual_voucher(
        &conn,
        transaction_id,
        &account_code,
        fund_account_id,
        summary,
    )?;
    db::log_operation(
        &conn,
        "create_bank_manual_voucher",
        &format!("流水生成凭证 {}", v.voucher_no),
        "system",
        None,
    )?;
    Ok(v)
}

#[tauri::command]
pub fn get_vouchers(
    query: VoucherQuery,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<Voucher>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    Ok(accounting::get_vouchers(&conn, &query)?)
}

// ===== Accounting（第五阶段 报表命令/导出） =====

#[tauri::command]
pub fn get_balance_sheet(
    month: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<BalanceSheet, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    Ok(accounting::build_balance_sheet(&conn, &month)?)
}

#[tauri::command]
pub fn get_income_statement(
    month: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<IncomeStatement, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    Ok(accounting::build_income_statement(&conn, &month)?)
}

#[tauri::command]
pub fn get_cash_flow_statement(
    month: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<CashFlowStatement, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    Ok(accounting::build_cash_flow_statement(&conn, &month)?)
}

/// 导出三大财务报表（report_type: balance_sheet / income_statement / cash_flow_statement）。
/// path 由前端保存对话框产生（与 export_month_close_report 等导出命令一致），返回实际写入路径。
#[tauri::command]
pub fn export_financial_report(
    month: String,
    report_type: String,
    path: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<String, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    // 文件名 资产负债表_YYYYMM.xlsx（与 brief 约定一致；后端只在路径缺扩展名时补默认名）
    let (report_name, build_and_export): (&str, fn(&Connection, &str, &str) -> AppResult<()>) =
        match report_type.as_str() {
            "balance_sheet" => (
                "资产负债表",
                |c, m, p| {
                    excel::export_balance_sheet(&accounting::build_balance_sheet(c, m)?, p)?;
                    Ok(())
                },
            ),
            "income_statement" => (
                "利润表",
                |c, m, p| {
                    excel::export_income_statement(&accounting::build_income_statement(c, m)?, p)?;
                    Ok(())
                },
            ),
            "cash_flow_statement" => (
                "现金流量表",
                |c, m, p| {
                    excel::export_cash_flow_statement(&accounting::build_cash_flow_statement(c, m)?, p)?;
                    Ok(())
                },
            ),
            _ => {
                return Err(AppError::InvalidParam(format!(
                    "未知报表类型: {report_type}（可选 balance_sheet / income_statement / cash_flow_statement）"
                )))
            }
        };
    // 前端 save() 对话框通常带扩展名；无扩展名时按约定补 资产负债表_YYYYMM.xlsx
    let final_path = PathBuf::from(&path);
    let final_path = if final_path.extension().is_some() {
        final_path
    } else {
        final_path.join(format!("{report_name}_{}.xlsx", month.replace('-', "")))
    };
    let final_path_str = final_path.to_string_lossy().to_string();
    build_and_export(&conn, &month, &final_path_str)?;
    db::log_operation(
        &conn,
        "export_financial_report",
        &format!("导出{month}{report_name}到{final_path_str}"),
        "system",
        None,
    )?;
    Ok(final_path_str)
}

#[tauri::command]
pub fn get_trial_balance(
    from_month: String,
    to_month: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<TrialBalanceReport, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    accounting::build_trial_balance(&conn, &from_month, &to_month)
}

#[tauri::command]
pub fn export_trial_balance(
    from_month: String,
    to_month: String,
    path: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<String, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let report = accounting::build_trial_balance(&conn, &from_month, &to_month)?;
    excel::export_trial_balance_excel(&report, &path)?;
    db::log_operation(
        &conn,
        "export_trial_balance",
        &format!("导出科目余额表 {from_month}~{to_month}"),
        "system",
        None,
    )?;
    Ok(path)
}

// ===== 个税年度汇总（第六阶段 Task 10） =====

#[tauri::command]
pub fn get_annual_tax_summary(
    year: i64,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<AnnualTaxSummaryRow>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::get_annual_tax_summary(&conn, year)
}

#[tauri::command]
pub fn export_annual_tax_summary(
    year: i64,
    path: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<String, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let rows = db::get_annual_tax_summary(&conn, year)?;
    excel::export_annual_tax_summary_excel(&rows, year, &path)?;
    db::log_operation(
        &conn,
        "export_annual_tax_summary",
        &format!("导出{year}年度个税汇总表到{path}"),
        "system",
        None,
    )?;
    Ok(path)
}

// ===== 社保公积金台账（第六阶段 Task 6） =====

#[tauri::command]
pub fn get_social_profiles(
    year: i64,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<SocialInsuranceProfile>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::get_social_profiles(&conn, year)
}

#[tauri::command]
pub fn save_social_profile(
    data: SocialInsuranceProfileInput,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<SocialInsuranceProfile, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let result = db::upsert_social_profile(&conn, &data)?;
    db::log_operation(
        &conn,
        "save_social_profile",
        &format!(
            "保存社保台账 {}-{}",
            result.employee_no, result.profile_year
        ),
        "system",
        None,
    )?;
    Ok(result)
}

#[tauri::command]
pub fn delete_social_profile(
    id: i64,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let ok = db::delete_social_profile(&conn, id)?;
    db::log_operation(
        &conn,
        "delete_social_profile",
        &format!("删除社保台账 #{id}"),
        "system",
        None,
    )?;
    Ok(ok)
}

#[tauri::command]
pub fn copy_social_profiles(
    from_year: i64,
    to_year: i64,
    factor: f64,
    apply_clamp: bool,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<usize, AppError> {
    let mut conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    // db 层逐行 INSERT 无事务，命令层用事务包裹保证调基原子性
    let tx = conn.transaction()?;
    let n = db::copy_social_profiles(&tx, from_year, to_year, factor, apply_clamp)?;
    db::log_operation(
        &tx,
        "copy_social_profiles",
        &format!("{from_year} 调基复制到 {to_year} 共 {n} 条"),
        "system",
        None,
    )?;
    tx.commit()?;
    Ok(n)
}

#[tauri::command]
pub fn get_social_base_limits(
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<f64>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let (a, b, c, d) = db::get_social_base_limits(&conn)?;
    Ok(vec![a, b, c, d])
}

#[tauri::command]
pub fn set_social_base_limits(
    ss_min: f64,
    ss_max: f64,
    hf_min: f64,
    hf_max: f64,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<(), AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::set_social_base_limits(&conn, ss_min, ss_max, hf_min, hf_max)?;
    db::log_operation(
        &conn,
        "set_social_base_limits",
        "保存社保基数上下限",
        "system",
        None,
    )?;
    Ok(())
}

// ==================== Cashier Commands（第七阶段 资金账户/往来单位/操作人） ====================
//
// 基础资料为主数据，不受月结保护；写日志署名用当前操作人（未选择时回退 system，
// 见 cashier::current_operator_name）。当前操作人会话由 CurrentOperatorState 管理。

#[tauri::command]
pub fn get_fund_accounts(
    query: FundAccountQuery,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<FundAccount>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    Ok(cashier::get_fund_accounts(&conn, &query)?)
}

#[tauri::command]
pub fn save_fund_account(
    data: FundAccountInput,
    state: tauri::State<'_, Mutex<Connection>>,
    current: tauri::State<'_, cashier::CurrentOperatorState>,
) -> Result<FundAccount, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let account = cashier::save_fund_account(&conn, &data)?;
    db::log_operation(
        &conn,
        "save_fund_account",
        &format!("保存资金账户 {} {}", account.account_code, account.name),
        &cashier::current_operator_name(&conn, &current),
        None,
    )?;
    Ok(account)
}

#[tauri::command]
pub fn set_active_fund_account(
    id: i64,
    active: bool,
    state: tauri::State<'_, Mutex<Connection>>,
    current: tauri::State<'_, cashier::CurrentOperatorState>,
) -> Result<FundAccount, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let account = cashier::set_active_fund_account(&conn, id, active)?;
    let action = if active { "启用" } else { "停用" };
    db::log_operation(
        &conn,
        "set_active_fund_account",
        &format!("{action}资金账户 {} {}", account.account_code, account.name),
        &cashier::current_operator_name(&conn, &current),
        None,
    )?;
    Ok(account)
}

#[tauri::command]
pub fn get_fund_migration_status(
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<FundMigrationStatus, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    Ok(cashier::get_fund_migration_status(&conn)?)
}

#[tauri::command]
pub fn preview_fund_assignment(
    entity_type: String,
    account_id: i64,
    belong_month: Option<String>,
    batch_id: Option<i64>,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<FundAssignmentPreview, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    Ok(cashier::preview_fund_assignment(
        &conn,
        &entity_type,
        account_id,
        belong_month.as_deref(),
        batch_id,
    )?)
}

#[tauri::command]
pub fn apply_fund_assignment(
    data: FundAssignmentInput,
    state: tauri::State<'_, Mutex<Connection>>,
    current: tauri::State<'_, cashier::CurrentOperatorState>,
) -> Result<FundAssignmentResult, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let result = cashier::apply_fund_assignment(&conn, &data)?;
    // 操作全量审计：对象类型、范围、目标账户与成功/联动/跳过条数全部入 operation_logs
    let (entity_label, scope) = match data.entity_type.as_str() {
        "bank_transaction" => (
            "银行流水",
            data.belong_month
                .as_deref()
                .map(str::trim)
                .filter(|m| !m.is_empty())
                .map(|m| format!("归属月 {m}"))
                .unwrap_or_else(|| "全部月份".to_string()),
        ),
        _ => (
            "付款批次",
            data.batch_id
                .map(|id| format!("批次 #{id}"))
                .unwrap_or_else(|| "全部待归集批次".to_string()),
        ),
    };
    let account = cashier::get_fund_account(&conn, data.account_id)?;
    db::log_operation(
        &conn,
        "apply_fund_assignment",
        &format!(
            "历史资金归集（{entity_label}，{scope} → {} {}）：归集 {} 条，联动资金分录 {} 条，跳过分录 {} 条",
            account.account_code,
            account.name,
            result.updated_count,
            result.linked_voucher_lines_updated,
            result.skipped_voucher_lines
        ),
        &cashier::current_operator_name(&conn, &current),
        None,
    )?;
    Ok(result)
}

#[tauri::command]
pub fn get_business_partners(
    query: BusinessPartnerQuery,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<BusinessPartner>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    Ok(cashier::get_business_partners(&conn, &query)?)
}

#[tauri::command]
pub fn save_business_partner(
    data: BusinessPartnerInput,
    state: tauri::State<'_, Mutex<Connection>>,
    current: tauri::State<'_, cashier::CurrentOperatorState>,
) -> Result<BusinessPartner, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let partner = cashier::save_business_partner(&conn, &data)?;
    db::log_operation(
        &conn,
        "save_business_partner",
        &format!("保存往来单位 {} {}", partner.partner_code, partner.name),
        &cashier::current_operator_name(&conn, &current),
        None,
    )?;
    Ok(partner)
}

#[tauri::command]
pub fn set_active_business_partner(
    id: i64,
    active: bool,
    state: tauri::State<'_, Mutex<Connection>>,
    current: tauri::State<'_, cashier::CurrentOperatorState>,
) -> Result<BusinessPartner, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let partner = cashier::set_active_business_partner(&conn, id, active)?;
    let action = if active { "启用" } else { "停用" };
    db::log_operation(
        &conn,
        "set_active_business_partner",
        &format!("{action}往来单位 {} {}", partner.partner_code, partner.name),
        &cashier::current_operator_name(&conn, &current),
        None,
    )?;
    Ok(partner)
}

#[tauri::command]
pub fn get_operator_profiles(
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<OperatorProfile>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    Ok(cashier::get_operator_profiles(&conn)?)
}

#[tauri::command]
pub fn save_operator_profile(
    data: OperatorProfileInput,
    state: tauri::State<'_, Mutex<Connection>>,
    current: tauri::State<'_, cashier::CurrentOperatorState>,
) -> Result<OperatorProfile, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    // 停用当前操作人时 cashier 层会先清会话，署名由其在变更前捕获返回，避免退化为 system
    let (profile, operator) = cashier::save_operator_profile(&conn, &current, &data)?;
    db::log_operation(
        &conn,
        "save_operator_profile",
        &format!("保存操作人 {}（{}）", profile.name, profile.role),
        &operator,
        None,
    )?;
    Ok(profile)
}

#[tauri::command]
pub fn set_active_operator_profile(
    id: i64,
    active: bool,
    state: tauri::State<'_, Mutex<Connection>>,
    current: tauri::State<'_, cashier::CurrentOperatorState>,
) -> Result<OperatorProfile, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    // 停用当前操作人时 cashier 层会先清会话，署名由其在变更前捕获返回，避免退化为 system
    let (profile, operator) = cashier::set_active_operator_profile(&conn, &current, id, active)?;
    let action = if active { "启用" } else { "停用" };
    db::log_operation(
        &conn,
        "set_active_operator_profile",
        &format!("{action}操作人 {}", profile.name),
        &operator,
        None,
    )?;
    Ok(profile)
}

/// 切换当前操作人（选择操作人自身署名，避免首次选择时无署名可用）
#[tauri::command]
pub fn set_current_operator(
    operator_id: i64,
    state: tauri::State<'_, Mutex<Connection>>,
    current: tauri::State<'_, cashier::CurrentOperatorState>,
) -> Result<OperatorProfile, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let profile = cashier::set_current_operator(&conn, &current, operator_id)?;
    db::log_operation(
        &conn,
        "set_current_operator",
        &format!("切换当前操作人为 {}", profile.name),
        &profile.name,
        None,
    )?;
    Ok(profile)
}

#[tauri::command]
pub fn get_current_operator(
    state: tauri::State<'_, Mutex<Connection>>,
    current: tauri::State<'_, cashier::CurrentOperatorState>,
) -> Result<Option<OperatorProfile>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    Ok(cashier::get_current_operator(&conn, &current)?)
}

// ==================== Cashier Commands（第七阶段 通用加密业务附件） ====================

/// 上传业务附件：源文件经文件对话框选取，后端归档到 attachments/ 并按 DEK 状态加密落库。
/// 须已选择当前操作人（本地署名）；预览走 `get_decrypted_attachment_url`。
#[tauri::command]
pub fn add_business_attachment(
    data: BusinessAttachmentInput,
    app: tauri::AppHandle,
    state: tauri::State<'_, Mutex<Connection>>,
    sec: tauri::State<'_, crate::security::SecurityState>,
    current: tauri::State<'_, cashier::CurrentOperatorState>,
) -> Result<BusinessAttachment, AppError> {
    let app_data_dir = app_data_dir(&app)?;
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let att = cashier::add_business_attachment(&conn, sec.inner(), &current, &app_data_dir, &data)?;
    db::log_operation(
        &conn,
        "add_business_attachment",
        &format!(
            "上传附件 {}（{}，实体 {} ID={}）",
            att.file_name,
            if att.encrypted {
                "已加密"
            } else {
                "未加密"
            },
            att.entity_type,
            att.entity_id
        ),
        &cashier::current_operator_name(&conn, &current),
        Some(&format!(
            "attachment_id={} file_size={:?}",
            att.id, att.file_size
        )),
    )?;
    Ok(att)
}

#[tauri::command]
pub fn list_business_attachments(
    entity_type: String,
    entity_id: i64,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<BusinessAttachment>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    Ok(cashier::list_business_attachments(
        &conn,
        &entity_type,
        entity_id,
    )?)
}

/// 删除业务附件（未提交实体的附件才允许删除；已提交须先反审批/驳回）。返回被删原文件名。
#[tauri::command]
pub fn delete_business_attachment(
    id: i64,
    state: tauri::State<'_, Mutex<Connection>>,
    current: tauri::State<'_, cashier::CurrentOperatorState>,
) -> Result<String, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let file_name = cashier::delete_business_attachment(&conn, &current, id)?;
    db::log_operation(
        &conn,
        "delete_business_attachment",
        &format!("删除附件 {file_name}（附件ID={id}）"),
        &cashier::current_operator_name(&conn, &current),
        None,
    )?;
    Ok(file_name)
}

// ==================== Cashier Commands（第七阶段 资金单据与审批） ====================
//
// 资金单状态只能经状态机命令流转（spec 2/5.1）；cashier 层函数自带事务（状态更新与
// approval_events 同事务），命令层直接调用并在成功后写操作日志。get 类命令不记日志。

#[tauri::command]
pub fn get_fund_documents(
    query: FundDocumentQuery,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<FundDocument>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    Ok(cashier::get_fund_documents(&conn, &query)?)
}

#[tauri::command]
pub fn get_fund_document_detail(
    id: i64,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<FundDocumentDetail, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    Ok(cashier::get_fund_document_detail(&conn, id)?)
}

/// 按实体查询审批轨迹（报销单与资金单共用 approval_events，spec 4.5）
#[tauri::command]
pub fn list_approval_events(
    entity_type: String,
    entity_id: i64,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<ApprovalEvent>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    Ok(cashier::list_approval_events(
        &conn,
        &entity_type,
        entity_id,
    )?)
}

#[tauri::command]
pub fn get_maker_checker_enabled(
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    Ok(cashier::get_maker_checker_enabled(&conn)?)
}

#[tauri::command]
pub fn set_maker_checker_enabled(
    enabled: bool,
    state: tauri::State<'_, Mutex<Connection>>,
    current: tauri::State<'_, cashier::CurrentOperatorState>,
) -> Result<(), AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    cashier::set_maker_checker_enabled(&conn, enabled)?;
    let action = if enabled { "开启" } else { "关闭" };
    db::log_operation(
        &conn,
        "set_maker_checker_enabled",
        &format!("{action}经办复核（提交人与审批人不得相同）"),
        &cashier::current_operator_name(&conn, &current),
        None,
    )?;
    Ok(())
}

#[tauri::command]
pub fn create_fund_document(
    data: FundDocumentInput,
    state: tauri::State<'_, Mutex<Connection>>,
    current: tauri::State<'_, cashier::CurrentOperatorState>,
) -> Result<FundDocument, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let doc = cashier::create_fund_document(&conn, &current, &data)?;
    db::log_operation(
        &conn,
        "create_fund_document",
        &format!(
            "新增{} {} 金额 {:.2}",
            cashier::fund_document_type_label(&doc.document_type),
            doc.document_no,
            doc.amount
        ),
        &cashier::current_operator_name(&conn, &current),
        Some(&format!("document_id={}", doc.id)),
    )?;
    Ok(doc)
}

#[tauri::command]
pub fn update_fund_document(
    data: FundDocumentInput,
    state: tauri::State<'_, Mutex<Connection>>,
    current: tauri::State<'_, cashier::CurrentOperatorState>,
) -> Result<FundDocument, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let doc = cashier::update_fund_document(&conn, &current, &data)?;
    db::log_operation(
        &conn,
        "update_fund_document",
        &format!(
            "修改{} {}（仅草稿可编辑）",
            cashier::fund_document_type_label(&doc.document_type),
            doc.document_no
        ),
        &cashier::current_operator_name(&conn, &current),
        Some(&format!("document_id={}", doc.id)),
    )?;
    Ok(doc)
}

/// 状态机命令族：submit/approve/reject/withdraw/void/settle（approve/reject/void 意见必填，
/// 由 cashier 层校验并随审批事件落库）。
#[tauri::command]
pub fn submit_fund_document(
    id: i64,
    comment: Option<String>,
    state: tauri::State<'_, Mutex<Connection>>,
    current: tauri::State<'_, cashier::CurrentOperatorState>,
) -> Result<FundDocument, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let doc = cashier::submit_fund_document(&conn, &current, id, comment.as_deref())?;
    db::log_operation(
        &conn,
        "submit_fund_document",
        &format!(
            "提交{} {} 金额 {:.2}",
            cashier::fund_document_type_label(&doc.document_type),
            doc.document_no,
            doc.amount
        ),
        &cashier::current_operator_name(&conn, &current),
        Some(&format!("document_id={}", doc.id)),
    )?;
    Ok(doc)
}

#[tauri::command]
pub fn approve_fund_document(
    id: i64,
    comment: String,
    state: tauri::State<'_, Mutex<Connection>>,
    current: tauri::State<'_, cashier::CurrentOperatorState>,
) -> Result<FundDocument, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let doc = cashier::approve_fund_document(&conn, &current, id, &comment)?;
    db::log_operation(
        &conn,
        "approve_fund_document",
        &format!(
            "审批通过{} {} 金额 {:.2}",
            cashier::fund_document_type_label(&doc.document_type),
            doc.document_no,
            doc.amount
        ),
        &cashier::current_operator_name(&conn, &current),
        Some(&format!(
            "document_id={} comment={}",
            doc.id,
            comment.trim()
        )),
    )?;
    Ok(doc)
}

#[tauri::command]
pub fn reject_fund_document(
    id: i64,
    comment: String,
    state: tauri::State<'_, Mutex<Connection>>,
    current: tauri::State<'_, cashier::CurrentOperatorState>,
) -> Result<FundDocument, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let doc = cashier::reject_fund_document(&conn, &current, id, &comment)?;
    db::log_operation(
        &conn,
        "reject_fund_document",
        &format!(
            "驳回{} {}",
            cashier::fund_document_type_label(&doc.document_type),
            doc.document_no
        ),
        &cashier::current_operator_name(&conn, &current),
        Some(&format!(
            "document_id={} comment={}",
            doc.id,
            comment.trim()
        )),
    )?;
    Ok(doc)
}

#[tauri::command]
pub fn withdraw_fund_document(
    id: i64,
    comment: Option<String>,
    state: tauri::State<'_, Mutex<Connection>>,
    current: tauri::State<'_, cashier::CurrentOperatorState>,
) -> Result<FundDocument, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let doc = cashier::withdraw_fund_document(&conn, &current, id, comment.as_deref())?;
    db::log_operation(
        &conn,
        "withdraw_fund_document",
        &format!(
            "撤回{} {} 至草稿",
            cashier::fund_document_type_label(&doc.document_type),
            doc.document_no
        ),
        &cashier::current_operator_name(&conn, &current),
        Some(&format!("document_id={}", doc.id)),
    )?;
    Ok(doc)
}

#[tauri::command]
pub fn void_fund_document(
    id: i64,
    comment: String,
    state: tauri::State<'_, Mutex<Connection>>,
    current: tauri::State<'_, cashier::CurrentOperatorState>,
) -> Result<FundDocument, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let doc = cashier::void_fund_document(&conn, &current, id, &comment)?;
    db::log_operation(
        &conn,
        "void_fund_document",
        &format!(
            "作废{} {}",
            cashier::fund_document_type_label(&doc.document_type),
            doc.document_no
        ),
        &cashier::current_operator_name(&conn, &current),
        Some(&format!(
            "document_id={} comment={}",
            doc.id,
            comment.trim()
        )),
    )?;
    Ok(doc)
}

/// 结算：收款/转账/借款核销单 approved 后直接结算；付款/借款单经付款批次标记付款后从
/// batched 结算（spec 5.1）。凭证联动由 Task 8 挂接。
#[tauri::command]
pub fn settle_fund_document(
    id: i64,
    state: tauri::State<'_, Mutex<Connection>>,
    current: tauri::State<'_, cashier::CurrentOperatorState>,
) -> Result<FundDocument, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let doc = cashier::settle_fund_document(&conn, &current, id)?;
    db::log_operation(
        &conn,
        "settle_fund_document",
        &format!(
            "结算{} {} 金额 {:.2}",
            cashier::fund_document_type_label(&doc.document_type),
            doc.document_no,
            doc.amount
        ),
        &cashier::current_operator_name(&conn, &current),
        Some(&format!("document_id={}", doc.id)),
    )?;
    Ok(doc)
}

/// 冲正（settled → reversed）：创建相反方向冲正单（立即结算）并将原单置为已冲正，
/// 原因必填；原单月份与冲正月份均须未月结（spec 5.1）。
#[tauri::command]
pub fn reverse_fund_document(
    data: FundDocumentReverseInput,
    state: tauri::State<'_, Mutex<Connection>>,
    current: tauri::State<'_, cashier::CurrentOperatorState>,
) -> Result<FundDocument, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let reversal = cashier::reverse_fund_document(&conn, &current, &data)?;
    db::log_operation(
        &conn,
        "reverse_fund_document",
        &format!(
            "冲正资金单据（原单ID={}）生成冲正单 {} 金额 {:.2}",
            data.document_id, reversal.document_no, reversal.amount
        ),
        &cashier::current_operator_name(&conn, &current),
        Some(&format!(
            "document_id={} reversal_id={} comment={}",
            data.document_id,
            reversal.id,
            data.comment.trim()
        )),
    )?;
    Ok(reversal)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("salary-{name}-{}", uuid::Uuid::new_v4()))
    }

    fn setup_closed_month_package_db(app_dir: &std::path::Path) -> Connection {
        fs::create_dir_all(app_dir).unwrap();
        let conn = db::init_db(&app_dir.to_string_lossy()).unwrap();
        conn.execute_batch(
            "
            INSERT INTO employees
                (id, employee_no, name, department, status, bank_account, bank_name, base_salary, created_at, updated_at)
            VALUES
                (1, 'E001', '张三', '销售部', 'active', '62220001', '测试银行', 10000, '2026-08-01', '2026-08-01');

            INSERT INTO attendance_records
                (salary_month, employee_no, name, expected_days, actual_days, late_count, early_leave_count, absent_days, created_at, updated_at)
            VALUES
                ('2026-08', 'E001', '张三', 22, 22, 0, 0, 0, '2026-08-31', '2026-08-31');

            INSERT INTO salary_monthly_results
                (salary_month, employee_no, name, department, base_salary, gross_salary, net_salary,
                 social_security_personal, housing_fund_personal, attendance_deduction, tax_amount,
                 other_deduction, status, locked, created_at, updated_at)
            VALUES
                ('2026-08', 'E001', '张三', '销售部', 10000, 10000, 7800, 1000, 1200, 0, 0, 0, 'locked', 1, '2026-08-31', '2026-08-31');

            INSERT INTO invoices
                (id, invoice_code, invoice_number, total_amount, expense_type_code, employee_id, belong_month, status, created_at, updated_at)
            VALUES
                (1, 'A', '001', 300, 'office', 1, '2026-08', 'normal', '2026-08-10', '2026-08-10');

            INSERT INTO reimbursement_claims
                (id, claim_no, employee_id, belong_month, title, total_amount, invoice_count, status, payment_status, payment_date, created_at, updated_at)
            VALUES
                (1, 'BX202608001', 1, '2026-08', '销售报销', 300, 1, 'approved', 'paid', '2026-08-31', '2026-08-15', '2026-08-15');

            INSERT INTO reimbursement_claim_invoices (claim_id, invoice_id, created_at)
            VALUES (1, 1, '2026-08-15');

            INSERT INTO payment_batches
                (id, batch_no, belong_month, batch_type, status, total_amount, item_count, payment_date, created_at, updated_at)
            VALUES
                (1, 'GZ202608TEST', '2026-08', 'salary', 'paid', 7800, 1, '2026-08-31', '2026-08-31', '2026-08-31'),
                (2, 'BX202608TEST', '2026-08', 'reimbursement', 'paid', 300, 1, '2026-08-31', '2026-08-31', '2026-08-31');

            INSERT INTO payment_items
                (batch_id, source_type, source_id, employee_id, employee_no, employee_name,
                 bank_name, bank_account, amount, status, remark, created_at)
            VALUES
                (1, 'salary_result', 1, 1, 'E001', '张三', '测试银行', '62220001', 7800, 'paid', '工资代发', '2026-08-31'),
                (2, 'reimbursement_claim', 1, 1, 'E001', '张三', '测试银行', '62220001', 300, 'paid', 'BX202608001', '2026-08-31');

            INSERT INTO bank_transactions
                (id, transaction_date, belong_month, summary, counterparty_name, counterparty_account,
                 income_amount, expense_amount, balance, status, created_at, updated_at)
            VALUES
                (1, '2026-08-31', '2026-08', 'GZ202608TEST 工资代发', '张三', '62220001', 0, 7800, 10000, 'matched', '2026-08-31', '2026-08-31'),
                (2, '2026-08-31', '2026-08', 'BX202608TEST 报销付款', '张三', '62220001', 0, 300, 9700, 'matched', '2026-08-31', '2026-08-31');

            INSERT INTO bank_transaction_matches
                (transaction_id, payment_batch_id, match_score, remark, status, created_at)
            VALUES
                (1, 1, 100, '测试匹配', 'active', '2026-08-31'),
                (2, 2, 100, '测试匹配', 'active', '2026-08-31');
            ",
        )
        .unwrap();
        db::close_month(&conn, "2026-08", "system", Some("导出测试")).unwrap();
        conn
    }

    #[test]
    fn test_export_month_close_package_includes_expected_files() {
        let app_dir = temp_dir("app-data");
        let output_root = temp_dir("package-output");
        let conn = setup_closed_month_package_db(&app_dir);

        let result =
            export_month_close_package_to_dir(&conn, "2026-08", &output_root.to_string_lossy())
                .unwrap();

        let expected = [
            "2026-08_月结报告.xlsx",
            "2026-08_工资明细.xlsx",
            "2026-08_银行代发.xlsx",
            "2026-08_发票清单.xlsx",
            "2026-08_报销清单.xlsx",
            "2026-08_GZ202608TEST_付款明细.xlsx",
            "2026-08_BX202608TEST_付款明细.xlsx",
            "manifest.json",
        ];
        for file_name in expected {
            let path = PathBuf::from(&result.output_dir).join(file_name);
            assert!(path.exists(), "missing package file: {}", path.display());
        }
        assert_eq!(result.files.len(), expected.len());

        let manifest =
            fs::read_to_string(PathBuf::from(&result.output_dir).join("manifest.json")).unwrap();
        assert!(manifest.contains("2026-08_报销清单.xlsx"));
        assert!(manifest.contains("2026-08_GZ202608TEST_付款明细.xlsx"));
        assert!(manifest.contains("2026-08_BX202608TEST_付款明细.xlsx"));

        drop(conn);
        let _ = fs::remove_dir_all(app_dir);
        let _ = fs::remove_dir_all(output_root);
    }
}
