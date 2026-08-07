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
  baidu_api_key: string;
  baidu_secret_key: string;
}

export interface OcrSettingsInput {
  ocr_mode?: 'local' | 'online';
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
  operation: string;
  module: string;
  detail: string;
  operator: string;
  created_at: string;
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
