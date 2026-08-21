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
  unmatched_paid_batch_count: number;
  duplicate_amount_count: number;
  over_budget_count: number;
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

// ==================== 银行流水 ====================

export type BankTransactionStatus = 'unmatched' | 'matched' | 'ignored';

export interface BankTransaction {
  id: number;
  transaction_date: string;
  belong_month: string;
  summary?: string;
  counterparty_name?: string;
  counterparty_account?: string;
  income_amount: number;
  expense_amount: number;
  balance?: number;
  status: BankTransactionStatus;
  ignore_reason?: string;
  imported_file?: string;
  raw_json?: string;
  matched_batch_id?: number;
  matched_batch_no?: string;
  matched_batch_type?: PaymentBatchType;
  matched_amount?: number;
  match_score?: number;
  match_remark?: string;
  created_at?: string;
  updated_at?: string;
}

export interface BankTransactionQuery {
  belong_month?: string;
  status?: BankTransactionStatus;
  keyword?: string;
}

export interface BankTransactionMatch {
  id: number;
  transaction_id: number;
  payment_batch_id: number;
  match_score: number;
  remark?: string;
  created_at?: string;
}

export interface BankTransactionMatchInput {
  transaction_id: number;
  payment_batch_id: number;
  remark?: string;
}

export interface BankTransactionIgnoreInput {
  transaction_id: number;
  reason: string;
}

export interface BankAutoMatchResult {
  success: boolean;
  matched: number;
  skipped: number;
  errors: string[];
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

export interface Budget {
  id: number;
  month: string;
  department?: string;
  expense_type_code?: string;
  expense_type_name?: string;
  budget_amount: number;
  remark?: string;
  created_at?: string;
  updated_at?: string;
}

export interface BudgetInput {
  id?: number;
  month: string;
  department?: string;
  expense_type_code?: string;
  budget_amount: number;
  remark?: string;
}

export interface BudgetQuery {
  month?: string;
  department?: string;
  expense_type_code?: string;
}

export interface BudgetExecution {
  budget: Budget;
  actual_amount: number;
  usage_percent: number;
  over_amount: number;
  status: 'ok' | 'over';
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
  budget_executions: BudgetExecution[];
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

// ==================== 安全模块 ====================

// 字段与后端 src-tauri/src/models.rs SecurityStatus 1:1 对齐。
// snake_case 命名是为了对齐 serde 序列化的 JSON 字段,避免映射层。
export interface SecurityStatus {
  initialized: boolean;
  locked: boolean;
  failed_attempts: number;
  lock_until: string | null;
  idle_lock_enabled: boolean;
  idle_timeout_seconds: number;
  sensitive_reveal_seconds: number;
  migration_status: string | null;
}

export interface UnlockResult {
  unlocked: boolean;
  failed_attempts: number;
  lock_until: string | null;
}

export interface RevealResult {
  expires_at: string;
}

export interface LegacyMigrationStatus {
  status: string;
  total_invoices: number;
  processed_invoices: number;
  token_migrated: boolean;
}

// ==================== 总账科目 ====================

export interface GlAccount {
  code: string;
  name: string;
  category: 'asset' | 'liability' | 'equity' | 'cost' | 'profit_loss';
  direction: 'debit' | 'credit';
  cash_flow_category: 'operating' | 'investing' | 'financing' | 'none';
  is_system: number;
  is_active: number;
  remark?: string | null;
}

export interface GlAccountInput {
  code: string;
  name: string;
  category: GlAccount['category'];
  direction: GlAccount['direction'];
  cash_flow_category?: GlAccount['cash_flow_category'];
  remark?: string | null;
}

export interface OpeningBalanceRow {
  account_code: string;
  debit_amount: number;
  credit_amount: number;
}

export interface OpeningBalanceState {
  month: string | null;
  rows: OpeningBalanceRow[];
}

export interface AccountMapping {
  id: number;
  scope: 'expense_type' | 'department';
  key: string;
  account_code: string;
  remark?: string | null;
}

// ==================== 记账凭证 ====================

export interface VoucherLine {
  id: number;
  account_code: string;
  debit_amount: number;
  credit_amount: number;
  summary?: string | null;
  line_order: number;
}

export interface Voucher {
  id: number;
  voucher_no: string;
  voucher_date: string;
  belong_month: string;
  source_type: string;
  source_id: number;
  total_amount: number;
  status: 'active' | 'void';
  remark?: string | null;
  lines: VoucherLine[];
}

export const VOUCHER_SOURCE_LABEL: Record<string, string> = {
  salary_accrual: '工资计提',
  salary_payment: '工资代发',
  reimbursement_accrual: '报销计提',
  reimbursement_payment: '报销付款',
  invoice_expense: '发票入账',
  bank_manual: '银行流水',
};

export interface VoucherQuery {
  month?: string;
  source_type?: string;
  status?: string;
}

// ==================== 财务报表 ====================

export type FinancialReportType =
  | 'balance_sheet'
  | 'income_statement'
  | 'cash_flow_statement'
  | 'trial_balance';

export interface TrialBalanceRow {
  code: string;
  name: string;
  category: string;
  direction: string;
  opening_debit: number;
  opening_credit: number;
  period_debit: number;
  period_credit: number;
  ending_debit: number;
  ending_credit: number;
}

export interface TrialBalanceReport {
  from_month: string;
  to_month: string;
  enabled: boolean;
  rows: TrialBalanceRow[];
  balanced: boolean;
}

// 个税年度汇总行（第六阶段 Task 10）：difference 负数为多缴
export interface AnnualTaxSummaryRow {
  employee_no: string;
  name?: string | null;
  month_count: number;
  total_gross: number;
  total_ss_personal: number;
  total_hf_personal: number;
  total_special_deduction: number;
  total_tax_withheld: number;
  annual_tax_due: number;
  difference: number;
}

export interface ReportRow {
  key: string;
  label: string;
  current: number;
  comparative: number;
}

export interface BalanceSheet {
  month: string;
  enabled: boolean;
  asset_rows: ReportRow[];
  liability_equity_rows: ReportRow[];
  asset_total: number;
  liability_equity_total: number;
  balanced: boolean;
}

export interface IncomeStatement {
  month: string;
  rows: ReportRow[];
  net_profit_month: number;
  net_profit_year: number;
}

export interface UnclassifiedCashItem {
  voucher_no: string;
  summary?: string | null;
  amount: number;
}

export interface CashFlowStatement {
  month: string;
  rows: ReportRow[];
  net_increase: number;
  unclassified: UnclassifiedCashItem[];
}

export interface SocialInsuranceProfile {
  id: number;
  employee_no: string;
  profile_year: number;
  ss_base: number;
  hf_base: number;
  ss_employer_rate: number;
  ss_personal_rate: number;
  hf_employer_rate: number;
  hf_personal_rate: number;
  remark: string | null;
  created_at: string | null;
  updated_at: string | null;
}

export interface SocialInsuranceProfileInput {
  id?: number;
  employee_no: string;
  profile_year: number;
  ss_base?: number;
  hf_base?: number;
  ss_employer_rate?: number;
  ss_personal_rate?: number;
  hf_employer_rate?: number;
  hf_personal_rate?: number;
  remark?: string;
}
