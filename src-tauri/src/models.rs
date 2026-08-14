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
    pub ocr_provider: String,
    pub baidu_api_key: String,
    pub baidu_secret_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrSettingsInput {
    pub ocr_mode: Option<String>,
    pub ocr_provider: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OperationLogQuery {
    pub operation_type: Option<String>,
    pub keyword: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub limit: Option<i64>,
}

// ==================== Month Close ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthCloseSummary {
    pub month: String,
    pub active_employee_count: i32,
    pub attendance_count: i32,
    pub missing_attendance_count: i32,
    pub abnormal_attendance_count: i32,
    pub salary_count: i32,
    pub reviewed_count: i32,
    pub locked_count: i32,
    pub missing_bank_count: i32,
    pub invoice_count: i32,
    pub uncategorized_invoice_count: i32,
    pub reimbursement_count: i32,
    pub pending_reimbursement_count: i32,
    pub unpaid_reimbursement_count: i32,
    pub pending_payment_batch_count: i32,
    pub unmatched_paid_batch_count: i32,
    pub duplicate_amount_count: i32,
    pub over_budget_count: i32,
    pub total_salary_cost: f64,
    pub total_invoice_amount: f64,
    pub approved_reimbursement_amount: f64,
    pub paid_reimbursement_amount: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthCloseCheckItem {
    pub key: String,
    pub title: String,
    pub status: String,
    pub count: i32,
    pub description: String,
    pub action_route: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthCloseWorkbench {
    pub summary: MonthCloseSummary,
    pub checks: Vec<MonthCloseCheckItem>,
    pub month_close: Option<MonthCloseRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthCloseRecord {
    pub id: i64,
    pub month: String,
    pub status: String,
    pub summary_json: Option<String>,
    pub checks_json: Option<String>,
    pub closed_at: Option<String>,
    pub closed_by: Option<String>,
    pub reopened_at: Option<String>,
    pub reopen_reason: Option<String>,
    pub remark: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthCloseInput {
    pub month: String,
    pub remark: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthReopenInput {
    pub month: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthClosePackageResult {
    pub success: bool,
    pub output_dir: String,
    pub files: Vec<String>,
}

// ==================== Payment Batches ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentBatch {
    pub id: i64,
    pub batch_no: String,
    pub belong_month: String,
    pub batch_type: String,
    pub status: String,
    pub total_amount: f64,
    pub item_count: i32,
    pub payment_date: Option<String>,
    pub remark: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentItem {
    pub id: i64,
    pub batch_id: i64,
    pub source_type: String,
    pub source_id: i64,
    pub employee_id: Option<i64>,
    pub employee_no: Option<String>,
    pub employee_name: Option<String>,
    pub bank_name: Option<String>,
    pub bank_account: Option<String>,
    pub amount: f64,
    pub status: String,
    pub remark: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentBatchDetail {
    pub batch: PaymentBatch,
    pub items: Vec<PaymentItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PaymentBatchQuery {
    pub belong_month: Option<String>,
    pub batch_type: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentBatchInput {
    pub belong_month: String,
    pub batch_type: String,
    pub source_ids: Option<Vec<i64>>,
    pub remark: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentBatchPaidInput {
    pub id: i64,
    pub payment_date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentBatchVoidInput {
    pub id: i64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentBatchRemarkInput {
    pub id: i64,
    pub remark: Option<String>,
}

// ==================== Bank Transactions ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BankTransaction {
    pub id: i64,
    pub transaction_date: String,
    pub belong_month: String,
    pub summary: Option<String>,
    pub counterparty_name: Option<String>,
    pub counterparty_account: Option<String>,
    pub income_amount: f64,
    pub expense_amount: f64,
    pub balance: Option<f64>,
    pub status: String,
    pub ignore_reason: Option<String>,
    pub imported_file: Option<String>,
    pub raw_json: Option<String>,
    pub matched_batch_id: Option<i64>,
    pub matched_batch_no: Option<String>,
    pub matched_batch_type: Option<String>,
    pub matched_amount: Option<f64>,
    pub match_score: Option<i32>,
    pub match_remark: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BankTransactionQuery {
    pub belong_month: Option<String>,
    pub status: Option<String>,
    pub keyword: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BankTransactionMatch {
    pub id: i64,
    pub transaction_id: i64,
    pub payment_batch_id: i64,
    pub match_score: i32,
    pub remark: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BankTransactionMatchInput {
    pub transaction_id: i64,
    pub payment_batch_id: i64,
    pub remark: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BankTransactionIgnoreInput {
    pub transaction_id: i64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BankAutoMatchResult {
    pub success: bool,
    pub matched: i32,
    pub skipped: i32,
    pub errors: Vec<String>,
}

// ==================== Financial Analysis ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinancialAnalysisQuery {
    pub month: String,
    pub months: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepartmentCostAnalysis {
    pub department: String,
    pub employee_count: i32,
    pub gross_salary: f64,
    pub social_security: f64,
    pub housing_fund: f64,
    pub salary_cost: f64,
    pub invoice_amount: f64,
    pub reimbursement_amount: f64,
    pub total_cost: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpenseTypeTrend {
    pub month: String,
    pub expense_type_code: String,
    pub expense_type_name: String,
    pub invoice_count: i32,
    pub invoice_amount: f64,
    pub reimbursement_amount: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmployeeCostView {
    pub employee_id: Option<i64>,
    pub employee_no: String,
    pub name: String,
    pub department: String,
    pub gross_salary: f64,
    pub net_salary: f64,
    pub social_security: f64,
    pub housing_fund: f64,
    pub attendance_deduction: f64,
    pub invoice_amount: f64,
    pub reimbursement_amount: f64,
    pub abnormal_attendance_count: i32,
    pub total_cost: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Budget {
    pub id: i64,
    pub month: String,
    pub department: Option<String>,
    pub expense_type_code: Option<String>,
    pub expense_type_name: Option<String>,
    pub budget_amount: f64,
    pub remark: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetInput {
    pub id: Option<i64>,
    pub month: String,
    pub department: Option<String>,
    pub expense_type_code: Option<String>,
    pub budget_amount: f64,
    pub remark: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BudgetQuery {
    pub month: Option<String>,
    pub department: Option<String>,
    pub expense_type_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetExecution {
    pub budget: Budget,
    pub actual_amount: f64,
    pub usage_percent: f64,
    pub over_amount: f64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthlyComparison {
    pub month: String,
    pub gross_salary: f64,
    pub net_salary: f64,
    pub deduction: f64,
    pub social_security: f64,
    pub housing_fund: f64,
    pub invoice_amount: f64,
    pub reimbursement_amount: f64,
    pub total_cost: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinancialAnalysisReport {
    pub month: String,
    pub months: i32,
    pub department_costs: Vec<DepartmentCostAnalysis>,
    pub expense_trends: Vec<ExpenseTypeTrend>,
    pub employee_costs: Vec<EmployeeCostView>,
    pub budget_executions: Vec<BudgetExecution>,
    pub monthly_comparison: Vec<MonthlyComparison>,
}

// ==================== Invoice ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceExpenseType {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub sort_order: i32,
    pub enabled: i32,
    pub remark: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceExpenseTypeInput {
    pub id: Option<i64>,
    pub code: Option<String>,
    pub name: Option<String>,
    pub sort_order: Option<i32>,
    pub enabled: Option<i32>,
    pub remark: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invoice {
    pub id: i64,
    pub invoice_code: Option<String>,
    pub invoice_number: Option<String>,
    pub invoice_type: Option<String>,
    pub issue_date: Option<String>,
    pub check_code: Option<String>,
    pub amount: f64,
    pub tax_amount: f64,
    pub total_amount: f64,
    pub seller_name: Option<String>,
    pub seller_tax_id: Option<String>,
    pub buyer_name: Option<String>,
    pub buyer_tax_id: Option<String>,
    pub expense_type_code: Option<String>,
    pub employee_id: Option<i64>,
    pub belong_month: Option<String>,
    pub status: Option<String>,
    pub remark: Option<String>,
    pub image_path: Option<String>,
    pub raw_ocr_json: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceInput {
    pub invoice_code: Option<String>,
    pub invoice_number: Option<String>,
    pub invoice_type: Option<String>,
    pub issue_date: Option<String>,
    pub check_code: Option<String>,
    pub amount: Option<f64>,
    pub tax_amount: Option<f64>,
    pub total_amount: Option<f64>,
    pub seller_name: Option<String>,
    pub seller_tax_id: Option<String>,
    pub buyer_name: Option<String>,
    pub buyer_tax_id: Option<String>,
    pub expense_type_code: Option<String>,
    pub employee_id: Option<i64>,
    pub belong_month: Option<String>,
    pub remark: Option<String>,
    pub image_path: Option<String>,
    pub raw_ocr_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceOcrPreview {
    pub invoice_code: Option<String>,
    pub invoice_number: Option<String>,
    pub invoice_type: Option<String>,
    pub issue_date: Option<String>,
    pub check_code: Option<String>,
    pub amount: f64,
    pub tax_amount: f64,
    pub total_amount: f64,
    pub seller_name: Option<String>,
    pub seller_tax_id: Option<String>,
    pub buyer_name: Option<String>,
    pub buyer_tax_id: Option<String>,
    pub raw_ocr_json: String,
    pub warnings: Vec<String>,
    pub is_duplicate: bool,
    pub duplicate_invoice_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InvoiceQuery {
    pub belong_month: Option<String>,
    pub employee_id: Option<i64>,
    pub expense_type_code: Option<String>,
    pub invoice_type: Option<String>,
    pub keyword: Option<String>,
    pub status: Option<String>,
}

// ==================== Reimbursement ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReimbursementClaim {
    pub id: i64,
    pub claim_no: String,
    pub employee_id: Option<i64>,
    pub employee_name: Option<String>,
    pub department: Option<String>,
    pub belong_month: String,
    pub title: String,
    pub total_amount: f64,
    pub invoice_count: i32,
    pub status: String,
    pub payment_status: String,
    pub payment_date: Option<String>,
    pub remark: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReimbursementClaimInput {
    pub id: Option<i64>,
    pub employee_id: Option<i64>,
    pub belong_month: String,
    pub title: String,
    pub invoice_ids: Vec<i64>,
    pub status: Option<String>,
    pub payment_status: Option<String>,
    pub payment_date: Option<String>,
    pub remark: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReimbursementQuery {
    pub belong_month: Option<String>,
    pub employee_id: Option<i64>,
    pub status: Option<String>,
    pub payment_status: Option<String>,
    pub keyword: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReimbursementInvoice {
    pub claim_id: i64,
    pub invoice_id: i64,
    pub invoice_number: Option<String>,
    pub seller_name: Option<String>,
    pub expense_type_code: Option<String>,
    pub total_amount: f64,
    pub issue_date: Option<String>,
}

// ==================== Data Safety ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataTableCount {
    pub table_name: String,
    pub label: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSafetyStatus {
    pub app_data_dir: String,
    pub database_path: String,
    pub database_exists: bool,
    pub database_size: u64,
    pub invoice_dir: String,
    pub invoice_dir_exists: bool,
    pub invoice_dir_size: u64,
    pub last_backup_at: Option<String>,
    pub last_backup_path: Option<String>,
    pub last_restore_at: Option<String>,
    pub table_counts: Vec<DataTableCount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataBackupResult {
    pub success: bool,
    pub backup_dir: String,
    pub database_path: String,
    pub invoice_dir: String,
    pub manifest_path: String,
    pub database_size: u64,
    pub invoice_dir_size: u64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataRestoreResult {
    pub success: bool,
    pub restored_at: String,
    pub restored_from: String,
    pub safety_backup_dir: String,
    pub restart_recommended: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSafetyCheckResult {
    pub ok: bool,
    pub checked_at: String,
    pub integrity_check: String,
    pub messages: Vec<String>,
}

// ==================== Security ====================

/// 安全中心状态概览,前端用于渲染 Setup/Lock/Status 等界面。
/// 未初始化时返回 initialized: false, locked: true(其他字段默认值)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityStatus {
    pub initialized: bool,
    pub locked: bool,
    pub failed_attempts: u32,
    pub lock_until: Option<String>,
    pub idle_lock_enabled: bool,
    pub idle_timeout_seconds: u32,
    pub sensitive_reveal_seconds: u32,
    pub migration_status: Option<String>,
}

/// unlock 命令返回结果。Task 4 临时定义在 security.rs,Task 6 迁移到 models.rs。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnlockResult {
    pub unlocked: bool,
    pub failed_attempts: u32,
    pub lock_until: Option<String>,
}

/// reveal_sensitive_data 返回结果:授予前端的明文查看截止时刻(RFC3339)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevealResult {
    pub expires_at: String,
}

/// 旧版(明文)资源迁移进度。未初始化时默认 status=completed, token_migrated=true。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegacyMigrationStatus {
    pub status: String,
    pub total_invoices: i64,
    pub processed_invoices: i64,
    pub token_migrated: bool,
}

// ==================== Accounting（第五阶段 科目/期初/映射） ====================

/// 会计科目（gl_accounts 表行）。is_system/is_active 用 0/1 表示。
#[derive(Debug, Clone, Serialize)]
pub struct GlAccount {
    pub code: String,
    pub name: String,
    pub category: String,
    pub direction: String,
    pub cash_flow_category: String,
    pub is_system: i64,
    pub is_active: i64,
    pub remark: Option<String>,
}

/// 新增自定义科目的前端入参。
#[derive(Debug, Clone, Deserialize)]
pub struct GlAccountInput {
    pub code: String,
    pub name: String,
    pub category: String,
    pub direction: String,
    pub cash_flow_category: Option<String>,
    pub remark: Option<String>,
}

/// 期初余额行（opening_balances 表行）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpeningBalanceRow {
    pub account_code: String,
    pub debit_amount: f64,
    pub credit_amount: f64,
}

/// 期初余额状态：期初月份 + 全部行。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpeningBalanceState {
    pub month: Option<String>,
    pub rows: Vec<OpeningBalanceRow>,
}

/// 科目映射（account_mappings 表行）：费用类型/部门 → 会计科目。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountMapping {
    pub id: i64,
    pub scope: String,
    pub key: String,
    pub account_code: String,
    pub remark: Option<String>,
}

/// 保存科目映射的前端入参。
#[derive(Debug, Clone, Deserialize)]
pub struct AccountMappingInput {
    pub scope: String,
    pub key: String,
    pub account_code: String,
    pub remark: Option<String>,
}

// ==================== Accounting（第五阶段 凭证核心） ====================

/// 凭证分录草稿（生成凭证入参的行）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoucherLineDraft {
    pub account_code: String,
    pub debit_amount: f64,
    pub credit_amount: f64,
    pub summary: Option<String>,
}

/// 凭证草稿（生成凭证入参）。借贷必须平衡，科目必须存在。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoucherDraft {
    pub belong_month: String,
    pub voucher_date: String,
    pub source_type: String,
    pub source_id: i64,
    pub remark: Option<String>,
    pub lines: Vec<VoucherLineDraft>,
}

/// 凭证分录（voucher_lines 表行，按 line_order 排序）。
#[derive(Debug, Clone, Serialize)]
pub struct VoucherLine {
    pub id: i64,
    pub account_code: String,
    pub debit_amount: f64,
    pub credit_amount: f64,
    pub summary: Option<String>,
    pub line_order: i64,
}

/// 凭证（vouchers 表行 + 分录列表）。
#[derive(Debug, Clone, Serialize)]
pub struct Voucher {
    pub id: i64,
    pub voucher_no: String,
    pub voucher_date: String,
    pub belong_month: String,
    pub source_type: String,
    pub source_id: i64,
    pub total_amount: f64,
    pub status: String,
    pub remark: Option<String>,
    pub lines: Vec<VoucherLine>,
}

/// 凭证查询条件（全部可选，组合过滤）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VoucherQuery {
    pub month: Option<String>,
    pub source_type: Option<String>,
    pub status: Option<String>,
}
