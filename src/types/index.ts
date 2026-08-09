// ==================== 员工相关 ====================

export type EmployeeStatus = '在职' | '离职' | '试用';

export interface Employee {
  id: number;
  employee_no: string;
  name: string;
  department: string;
  position: string;
  id_card: string;
  phone: string;
  bank_account: string;
  bank_name: string;
  hire_date: string;
  status: EmployeeStatus;
  base_salary: number;
  position_salary: number;
  performance_salary: number;
  social_insurance_base: number;
  housing_fund_base: number;
  special_deduction: number;
  remark: string;
  created_at: string;
  updated_at: string;
}

export interface EmployeeInput {
  employee_no: string;
  name: string;
  department: string;
  position: string;
  id_card?: string;
  phone?: string;
  bank_account?: string;
  bank_name?: string;
  hire_date?: string;
  status?: EmployeeStatus;
  base_salary: number;
  position_salary: number;
  performance_salary: number;
  social_insurance_base?: number;
  housing_fund_base?: number;
  special_deduction?: number;
  remark?: string;
}

// ==================== 考勤相关 ====================

export interface AttendanceRecord {
  id: number;
  month: string;
  employee_id: number;
  employee_no: string;
  employee_name: string;
  required_days: number;
  actual_days: number;
  late_count: number;
  early_leave_count: number;
  leave_days: number;
  sick_leave_days: number;
  personal_leave_days: number;
  absent_days: number;
  overtime_hours: number;
  created_at: string;
  updated_at: string;
}

export interface AttendanceRecordInput {
  month: string;
  employee_id: number;
  required_days: number;
  actual_days: number;
  late_count: number;
  early_leave_count: number;
  leave_days: number;
  sick_leave_days: number;
  personal_leave_days: number;
  absent_days: number;
  overtime_hours: number;
}

// ==================== 工资规则相关 ====================

export interface SalaryRule {
  id: number;
  late_penalty: number;
  early_leave_penalty: number;
  personal_leave_rate: number;
  sick_leave_rate: number;
  absent_rate: number;
  overtime_rate: number;
  social_insurance_rate: number;
  housing_fund_rate: number;
  tax_threshold: number;
  created_at: string;
  updated_at: string;
}

export interface TaxRule {
  id: number;
  level: number;
  min_amount: number;
  max_amount: number;
  tax_rate: number;
  quick_deduction: number;
}

export interface TaxRuleInput {
  level: number;
  min_amount: number;
  max_amount: number;
  tax_rate: number;
  quick_deduction: number;
}

// ==================== 工资结果相关 ====================

export type SalaryStatus = '草稿' | '已复核' | '已锁定';

export interface SalaryResult {
  id: number;
  month: string;
  employee_id: number;
  employee_no: string;
  employee_name: string;
  department: string;
  base_salary: number;
  position_salary: number;
  performance_salary: number;
  overtime_pay: number;
  meal_allowance: number;
  transport_allowance: number;
  other_allowance: number;
  gross_salary: number;
  social_insurance: number;
  housing_fund: number;
  attendance_deduction: number;
  income_tax: number;
  other_deduction: number;
  total_deduction: number;
  net_salary: number;
  status: SalaryStatus;
  remark: string;
  created_at: string;
  updated_at: string;
}

export interface SalaryResultUpdate {
  other_allowance?: number;
  other_deduction?: number;
  remark?: string;
  status?: SalaryStatus;
}

// ==================== OCR 相关 ====================

export type OcrStatus = '待识别' | '识别中' | '已完成' | '失败' | 'pending' | 'confirmed' | 'failed';

export interface OcrBatch {
  id: number;
  batch_name: string;
  salary_month?: string;
  file_path: string;
  raw_text?: string;
  parsed_json?: string;
  status: OcrStatus;
  result_count: number;
  created_at: string;
}

export interface OcrResult {
  batch_id: number;
  raw_text: string;
  records: AttendanceRecordInput[];
}

// ==================== 导入导出相关 ====================

export interface OcrSettings {
  ocr_mode: 'local' | 'online';
  ocr_provider: 'baidu';
  baidu_api_key: string;
  baidu_secret_key: string;
}

export interface OcrSettingsInput {
  ocr_mode?: 'local' | 'online';
  ocr_provider?: 'baidu';
  baidu_api_key?: string;
  baidu_secret_key?: string;
}

export interface ImportResult {
  success: boolean;
  total: number;
  imported: number;
  failed: number;
  errors: string[];
}

// ==================== 仪表盘相关 ====================

export interface DashboardSummary {
  month: string;
  total_employees: number;
  pending_count: number;
  calculated_count: number;
  abnormal_attendance_count: number;
  total_gross_salary: number;
  total_deduction: number;
  total_net_salary: number;
}

// ==================== 操作日志 ====================

export interface OperationLog {
  id: number;
  operation_type: string;
  description?: string;
  operator?: string;
  detail?: string;
  created_at: string;
}

export interface OperationLogQuery {
  operation_type?: string;
  keyword?: string;
  start_date?: string;
  end_date?: string;
  limit?: number;
}

// ==================== 数据安全 ====================

export interface DataTableCount {
  table_name: string;
  label: string;
  count: number;
}

export interface DataSafetyStatus {
  app_data_dir: string;
  database_path: string;
  database_exists: boolean;
  database_size: number;
  invoice_dir: string;
  invoice_dir_exists: boolean;
  invoice_dir_size: number;
  last_backup_at?: string;
  last_backup_path?: string;
  last_restore_at?: string;
  table_counts: DataTableCount[];
}

export interface DataBackupResult {
  success: boolean;
  backup_dir: string;
  database_path: string;
  invoice_dir: string;
  manifest_path: string;
  database_size: number;
  invoice_dir_size: number;
  created_at: string;
}

export interface DataRestoreResult {
  success: boolean;
  restored_at: string;
  restored_from: string;
  safety_backup_dir: string;
  restart_recommended: boolean;
}

export interface DataSafetyCheckResult {
  ok: boolean;
  checked_at: string;
  integrity_check: string;
  messages: string[];
}

// ==================== 月结工作台 ====================

export interface MonthCloseSummary {
  month: string;
  active_employee_count: number;
  attendance_count: number;
  missing_attendance_count: number;
  abnormal_attendance_count: number;
  salary_count: number;
  reviewed_count: number;
  locked_count: number;
  missing_bank_count: number;
  invoice_count: number;
  uncategorized_invoice_count: number;
  reimbursement_count: number;
  pending_reimbursement_count: number;
  unpaid_reimbursement_count: number;
  pending_payment_batch_count: number;
  total_salary_cost: number;
  total_invoice_amount: number;
  approved_reimbursement_amount: number;
  paid_reimbursement_amount: number;
}

export type MonthCloseCheckStatus = 'ok' | 'warning' | 'blocking';

export interface MonthCloseCheckItem {
  key: string;
  title: string;
  status: MonthCloseCheckStatus;
  count: number;
  description: string;
  action_route?: string;
}

export type MonthCloseStatus = 'open' | 'closed' | 'reopened';

export interface MonthCloseRecord {
  id: number;
  month: string;
  status: MonthCloseStatus;
  summary_json?: string;
  checks_json?: string;
  closed_at?: string;
  closed_by?: string;
  reopened_at?: string;
  reopen_reason?: string;
  remark?: string;
  created_at?: string;
  updated_at?: string;
}

export interface MonthClosePackageResult {
  success: boolean;
  output_dir: string;
  files: string[];
}

export interface MonthCloseWorkbench {
  summary: MonthCloseSummary;
  checks: MonthCloseCheckItem[];
  month_close?: MonthCloseRecord;
}

// ==================== 付款批次 ====================

export type PaymentBatchType = 'salary' | 'reimbursement';
export type PaymentBatchStatus = 'draft' | 'exported' | 'paid' | 'void';
export type PaymentSourceType = 'salary_result' | 'reimbursement_claim';
export type PaymentItemStatus = 'pending' | 'paid' | 'void';

export interface PaymentBatch {
  id: number;
  batch_no: string;
  belong_month: string;
  batch_type: PaymentBatchType;
  status: PaymentBatchStatus;
  total_amount: number;
  item_count: number;
  payment_date?: string;
  remark?: string;
  created_at?: string;
  updated_at?: string;
}

export interface PaymentItem {
  id: number;
  batch_id: number;
  source_type: PaymentSourceType;
  source_id: number;
  employee_id?: number;
  employee_no?: string;
  employee_name?: string;
  bank_name?: string;
  bank_account?: string;
  amount: number;
  status: PaymentItemStatus;
  remark?: string;
  created_at?: string;
}

export interface PaymentBatchDetail {
  batch: PaymentBatch;
  items: PaymentItem[];
}

export interface PaymentBatchQuery {
  belong_month?: string;
  batch_type?: PaymentBatchType;
  status?: PaymentBatchStatus;
}

export interface PaymentBatchInput {
  belong_month: string;
  batch_type: PaymentBatchType;
  source_ids?: number[];
  remark?: string;
}

export interface PaymentBatchPaidInput {
  id: number;
  payment_date: string;
}

export interface PaymentBatchVoidInput {
  id: number;
  reason: string;
}

export interface PaymentBatchRemarkInput {
  id: number;
  remark?: string;
}

// ==================== 财务分析 ====================

export interface FinancialAnalysisQuery {
  month: string;
  months?: number;
}

export interface DepartmentCostAnalysis {
  department: string;
  employee_count: number;
  gross_salary: number;
  social_security: number;
  housing_fund: number;
  salary_cost: number;
  invoice_amount: number;
  reimbursement_amount: number;
  total_cost: number;
}

export interface ExpenseTypeTrend {
  month: string;
  expense_type_code: string;
  expense_type_name: string;
  invoice_count: number;
  invoice_amount: number;
  reimbursement_amount: number;
}

export interface EmployeeCostView {
  employee_id?: number;
  employee_no: string;
  name: string;
  department: string;
  gross_salary: number;
  net_salary: number;
  social_security: number;
  housing_fund: number;
  attendance_deduction: number;
  invoice_amount: number;
  reimbursement_amount: number;
  abnormal_attendance_count: number;
  total_cost: number;
}

export interface MonthlyComparison {
  month: string;
  gross_salary: number;
  net_salary: number;
  deduction: number;
  social_security: number;
  housing_fund: number;
  invoice_amount: number;
  reimbursement_amount: number;
  total_cost: number;
}

export interface FinancialAnalysisReport {
  month: string;
  months: number;
  department_costs: DepartmentCostAnalysis[];
  expense_trends: ExpenseTypeTrend[];
  employee_costs: EmployeeCostView[];
  monthly_comparison: MonthlyComparison[];
}

// ==================== 发票相关 ====================

export type InvoiceStatus = 'normal' | 'void';

export interface InvoiceExpenseType {
  id: number;
  code: string;
  name: string;
  sort_order: number;
  enabled: number;
  remark?: string;
}

export interface InvoiceExpenseTypeInput {
  id?: number;
  code?: string;
  name?: string;
  sort_order?: number;
  enabled?: number;
  remark?: string;
}

export interface Invoice {
  id: number;
  invoice_code?: string;
  invoice_number?: string;
  invoice_type?: string;
  issue_date?: string;
  check_code?: string;
  amount: number;
  tax_amount: number;
  total_amount: number;
  seller_name?: string;
  seller_tax_id?: string;
  buyer_name?: string;
  buyer_tax_id?: string;
  expense_type_code?: string;
  employee_id?: number;
  belong_month?: string;
  status?: string;
  remark?: string;
  image_path?: string;
  raw_ocr_json?: string;
  created_at?: string;
  updated_at?: string;
}

export interface InvoiceInput {
  invoice_code?: string;
  invoice_number?: string;
  invoice_type?: string;
  issue_date?: string;
  check_code?: string;
  amount?: number;
  tax_amount?: number;
  total_amount?: number;
  seller_name?: string;
  seller_tax_id?: string;
  buyer_name?: string;
  buyer_tax_id?: string;
  expense_type_code?: string;
  employee_id?: number;
  belong_month?: string;
  remark?: string;
  image_path?: string;
  raw_ocr_json?: string;
}

export interface InvoiceOcrPreview {
  invoice_code?: string;
  invoice_number?: string;
  invoice_type?: string;
  issue_date?: string;
  check_code?: string;
  amount: number;
  tax_amount: number;
  total_amount: number;
  seller_name?: string;
  seller_tax_id?: string;
  buyer_name?: string;
  buyer_tax_id?: string;
  raw_ocr_json: string;
  warnings: string[];
  is_duplicate: boolean;
  duplicate_invoice_id?: number;
}

export interface InvoiceQuery {
  belong_month?: string;
  employee_id?: number;
  expense_type_code?: string;
  invoice_type?: string;
  keyword?: string;
  status?: InvoiceStatus;
}

// ==================== 报销相关 ====================

export type ReimbursementStatus = 'draft' | 'submitted' | 'approved' | 'rejected' | 'void';
export type PaymentStatus = 'unpaid' | 'paid';

export interface ReimbursementClaim {
  id: number;
  claim_no: string;
  employee_id?: number;
  employee_name?: string;
  department?: string;
  belong_month: string;
  title: string;
  total_amount: number;
  invoice_count: number;
  status: ReimbursementStatus;
  payment_status: PaymentStatus;
  payment_date?: string;
  remark?: string;
  created_at?: string;
  updated_at?: string;
}

export interface ReimbursementClaimInput {
  id?: number;
  employee_id?: number;
  belong_month: string;
  title: string;
  invoice_ids: number[];
  status?: ReimbursementStatus;
  payment_status?: PaymentStatus;
  payment_date?: string;
  remark?: string;
}

export interface ReimbursementQuery {
  belong_month?: string;
  employee_id?: number;
  status?: ReimbursementStatus;
  payment_status?: PaymentStatus;
  keyword?: string;
}

export interface ReimbursementInvoice {
  claim_id: number;
  invoice_id: number;
  invoice_number?: string;
  seller_name?: string;
  expense_type_code?: string;
  total_amount: number;
  issue_date?: string;
}
