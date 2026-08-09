use std::collections::HashSet;
use std::sync::Mutex;

use rusqlite::Connection;
use tauri::Manager;

use crate::db;
use crate::errors::AppError;
use crate::excel;
use crate::models::*;
use crate::ocr;
use crate::salary;

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
    db::create_attendance_record(&conn, &data)
}

#[tauri::command]
pub fn update_attendance_record(
    id: i64,
    data: AttendanceRecordInput,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::update_attendance_record(&conn, id, &data)
}

#[tauri::command]
pub fn delete_attendance_record(
    id: i64,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
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
    db::update_salary_result(&conn, id, &data)
}

#[tauri::command]
pub fn lock_salary_results(
    month: String,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
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
) -> Result<OcrResult, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    let resource_dir = app.path().resource_dir().ok();
    let mode = mode.as_deref().unwrap_or("local");
    ocr::ocr_recognize(&image_path, &month, mode, &conn, resource_dir.as_deref())
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
pub fn query_operation_logs(
    query: OperationLogQuery,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Vec<OperationLog>, AppError> {
    let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;
    db::query_operation_logs(&conn, &query)
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
) -> Result<OcrResult, AppError> {
    let connection = conn.lock().map_err(|e| AppError::General(e.to_string()))?;
    let shift = shift_type.as_deref().unwrap_or("day");
    let m = mode.as_deref().unwrap_or("online");
    ocr::ocr_recognize_punch_card(&image_path, &month, shift, m, &connection)
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
    app: tauri::AppHandle,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<InvoiceOcrPreview, AppError> {
    crate::invoice::ocr_invoice(&image_path, state.inner())
}

#[tauri::command]
pub fn save_invoice(
    data: InvoiceInput,
    app: tauri::AppHandle,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<Invoice, AppError> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::General(format!("获取app_data_dir失败: {e}")))?;
    crate::invoice::save_invoice_with_mutex(&data, state.inner(), &app_data_dir)
}

#[tauri::command]
pub fn update_invoice(
    id: i64,
    data: InvoiceInput,
    app: tauri::AppHandle,
    state: tauri::State<'_, Mutex<Connection>>,
) -> Result<bool, AppError> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::General(format!("获取app_data_dir失败: {e}")))?;
    crate::invoice::update_invoice_with_mutex(id, &data, state.inner(), &app_data_dir)
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
