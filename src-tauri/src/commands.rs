use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use rusqlite::Connection;
use tauri::Manager;

use crate::data_safety;
use crate::db;
use crate::errors::AppError;
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
) -> Result<PaymentBatchDetail, AppError> {
    let mut conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let detail = db::create_payment_batch(&mut conn, &data)?;
    db::log_operation(
        &conn,
        "create_payment_batch",
        &format!(
            "生成{}付款批次{}，{}笔，金额{:.2}",
            if detail.batch.batch_type == "salary" {
                "工资"
            } else {
                "报销"
            },
            detail.batch.batch_no,
            detail.batch.item_count,
            detail.batch.total_amount
        ),
        "system",
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
    db::ensure_month_open(&conn, &detail.batch.belong_month)?;
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
) -> Result<PaymentBatch, AppError> {
    let mut conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let batch = db::mark_payment_batch_paid(&mut conn, &data)?;
    db::log_operation(
        &conn,
        "mark_payment_batch_paid",
        &format!("标记付款批次{}已付款", batch.batch_no),
        "system",
        Some(&data.payment_date),
    )?;
    Ok(batch)
}

#[tauri::command]
pub fn void_payment_batch(
    data: PaymentBatchVoidInput,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<PaymentBatch, AppError> {
    let mut conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let batch = db::void_payment_batch(&mut conn, &data)?;
    db::log_operation(
        &conn,
        "void_payment_batch",
        &format!("作废付款批次{}", batch.batch_no),
        "system",
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

#[tauri::command]
pub fn import_bank_transactions_file(
    path: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<ImportResult, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let transactions = excel::read_bank_transactions_file(&path)?;
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
        &format!("导入银行流水: 总{total}, 成功{imported}, 跳过{skipped}"),
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
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<DataSafetyCheckResult, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let result = data_safety::verify_database(&conn)?;
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
