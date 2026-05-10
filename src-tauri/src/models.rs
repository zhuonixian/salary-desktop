use serde::{Deserialize, Serialize};

// ==================== Employee ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Employee {
    pub id: i64,
    pub employee_no: String,
    pub name: String,
    pub department: Option<String>,
    pub position: Option<String>,
    pub id_card: Option<String>,
    pub phone: Option<String>,
    pub bank_account: Option<String>,
    pub bank_name: Option<String>,
    pub hire_date: Option<String>,
    pub status: String,
    pub base_salary: f64,
    pub position_salary: f64,
    pub performance_salary: f64,
    pub social_security_base: f64,
    pub housing_fund_base: f64,
    pub special_deduction: f64,
    pub remark: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmployeeInput {
    pub employee_no: String,
    pub name: String,
    pub department: Option<String>,
    pub position: Option<String>,
    pub id_card: Option<String>,
    pub phone: Option<String>,
    pub bank_account: Option<String>,
    pub bank_name: Option<String>,
    pub hire_date: Option<String>,
    pub status: Option<String>,
    pub base_salary: Option<f64>,
    pub position_salary: Option<f64>,
    pub performance_salary: Option<f64>,
    pub social_security_base: Option<f64>,
    pub housing_fund_base: Option<f64>,
    pub special_deduction: Option<f64>,
    pub remark: Option<String>,
}

// ==================== Attendance ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttendanceRecord {
    pub id: i64,
    pub salary_month: String,
    pub employee_no: String,
    pub name: Option<String>,
    pub expected_days: f64,
    pub actual_days: f64,
    pub late_count: i32,
    pub early_leave_count: i32,
    pub personal_leave_days: f64,
    pub sick_leave_days: f64,
    pub absent_days: f64,
    pub overtime_hours: f64,
    pub source_type: Option<String>,
    pub ocr_batch_id: Option<i64>,
    pub remark: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttendanceRecordInput {
    pub id: Option<i64>,
    pub salary_month: String,
    pub employee_no: String,
    pub name: Option<String>,
    pub expected_days: Option<f64>,
    pub actual_days: Option<f64>,
    pub late_count: Option<i32>,
    pub early_leave_count: Option<i32>,
    pub personal_leave_days: Option<f64>,
    pub sick_leave_days: Option<f64>,
    pub absent_days: Option<f64>,
    pub overtime_hours: Option<f64>,
    pub source_type: Option<String>,
    pub ocr_batch_id: Option<i64>,
    pub remark: Option<String>,
}

// ==================== Salary Rules ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SalaryRule {
    pub id: i64,
    pub rule_key: String,
    pub rule_name: String,
    pub rule_value: f64,
    pub rule_type: Option<String>,
    pub enabled: i32,
    pub remark: Option<String>,
}

// ==================== Tax Rules ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxRule {
    pub id: i64,
    pub min_amount: f64,
    pub max_amount: Option<f64>,
    pub tax_rate: f64,
    pub quick_deduction: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxRuleInput {
    pub min_amount: Option<f64>,
    pub max_amount: Option<f64>,
    pub tax_rate: Option<f64>,
    pub quick_deduction: Option<f64>,
}

// ==================== Salary Result ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SalaryResult {
    pub id: i64,
    pub salary_month: String,
    pub employee_no: String,
    pub name: Option<String>,
    pub department: Option<String>,
    pub base_salary: f64,
    pub position_salary: f64,
    pub performance_salary: f64,
    pub overtime_salary: f64,
    pub meal_allowance: f64,
    pub transport_allowance: f64,
    pub other_allowance: f64,
    pub gross_salary: f64,
    pub social_security_personal: f64,
    pub housing_fund_personal: f64,
    pub attendance_deduction: f64,
    pub tax_amount: f64,
    pub other_deduction: f64,
    pub net_salary: f64,
    pub status: String,
    pub locked: i32,
    pub remark: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SalaryResultUpdate {
    pub overtime_salary: Option<f64>,
    pub meal_allowance: Option<f64>,
    pub transport_allowance: Option<f64>,
    pub other_allowance: Option<f64>,
    pub other_deduction: Option<f64>,
    pub remark: Option<String>,
}

// ==================== OCR ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrBatch {
    pub id: i64,
    pub batch_name: Option<String>,
    pub salary_month: Option<String>,
    pub image_path: Option<String>,
    pub raw_text: Option<String>,
    pub parsed_json: Option<String>,
    pub status: String,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrResult {
    pub batch_id: i64,
    pub records: Vec<AttendanceRecordInput>,
    pub raw_text: Option<String>,
}

// ==================== Import ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResult {
    pub success: bool,
    pub total: i32,
    pub imported: i32,
    pub skipped: i32,
    pub errors: Vec<String>,
}

// ==================== Dashboard ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardSummary {
    pub employee_count: i32,
    pub active_employee_count: i32,
    pub calculated_count: i32,
    pub locked_count: i32,
    pub total_gross_salary: f64,
    pub total_net_salary: f64,
    pub total_social_security: f64,
    pub total_housing_fund: f64,
    pub total_tax: f64,
    pub attendance_count: i32,
}

// ==================== Operation Log ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrSettings {
    pub ocr_mode: String,
    pub baidu_api_key: String,
    pub baidu_secret_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrSettingsInput {
    pub ocr_mode: Option<String>,
    pub baidu_api_key: Option<String>,
    pub baidu_secret_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationLog {
    pub id: i64,
    pub operation_type: String,
    pub description: Option<String>,
    pub operator: Option<String>,
    pub detail: Option<String>,
    pub created_at: Option<String>,
}
