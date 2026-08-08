import { invoke } from '@tauri-apps/api/core';
import type {
  Employee,
  EmployeeInput,
  AttendanceRecord,
  AttendanceRecordInput,
  SalaryRule,
  TaxRule,
  TaxRuleInput,
  SalaryResult,
  SalaryResultUpdate,
  OcrBatch,
  OcrResult,
  OcrSettings,
  OcrSettingsInput,
  ImportResult,
  DashboardSummary,
  EmployeeStatus,
  InvoiceExpenseType,
  InvoiceExpenseTypeInput,
  Invoice,
  InvoiceInput,
  InvoiceOcrPreview,
  InvoiceQuery,
  MonthCloseWorkbench,
  FinancialAnalysisQuery,
  FinancialAnalysisReport,
  OperationLog,
  OperationLogQuery,
  ReimbursementClaim,
  ReimbursementClaimInput,
  ReimbursementInvoice,
  ReimbursementQuery,
  ReimbursementStatus,
  PaymentStatus,
} from '@/types';

type BackendDashboardSummary = {
  employee_count?: number;
  active_employee_count?: number;
  calculated_count?: number;
  locked_count?: number;
  total_gross_salary?: number;
  total_net_salary?: number;
  total_social_security?: number;
  total_housing_fund?: number;
  total_tax?: number;
  attendance_count?: number;
};

type BackendEmployee = Omit<Employee, 'status'> & {
  status?: string | null;
};

type BackendSalaryRule = {
  id: number;
  rule_key: string;
  rule_value: number;
};

type BackendTaxRule = {
  id: number;
  min_amount: number;
  max_amount?: number | null;
  tax_rate: number;
  quick_deduction: number;
};

type BackendOcrBatch = {
  id: number;
  batch_name?: string | null;
  salary_month?: string | null;
  image_path?: string | null;
  raw_text?: string | null;
  parsed_json?: string | null;
  status: string;
  created_at?: string | null;
};

type BackendAttendanceRecord = {
  id: number;
  salary_month?: string | null;
  employee_no?: string | null;
  name?: string | null;
  expected_days?: number | null;
  actual_days?: number | null;
  late_count?: number | null;
  early_leave_count?: number | null;
  personal_leave_days?: number | null;
  sick_leave_days?: number | null;
  absent_days?: number | null;
  overtime_hours?: number | null;
  remark?: string | null;
  created_at?: string | null;
  updated_at?: string | null;
};

type BackendSalaryResult = {
  id: number;
  salary_month?: string | null;
  employee_no?: string | null;
  name?: string | null;
  department?: string | null;
  base_salary?: number | null;
  position_salary?: number | null;
  performance_salary?: number | null;
  overtime_salary?: number | null;
  meal_allowance?: number | null;
  transport_allowance?: number | null;
  other_allowance?: number | null;
  gross_salary?: number | null;
  social_security_personal?: number | null;
  housing_fund_personal?: number | null;
  attendance_deduction?: number | null;
  tax_amount?: number | null;
  other_deduction?: number | null;
  net_salary?: number | null;
  status?: string | null;
  locked?: boolean | null;
  remark?: string | null;
  created_at?: string | null;
  updated_at?: string | null;
};

const numberOrZero = (value: unknown): number => (typeof value === 'number' && Number.isFinite(value) ? value : 0);

const normalizeEmployeeStatus = (status?: string | null): EmployeeStatus => {
  if (status === 'inactive' || status === '离职') return '离职';
  if (status === 'probation' || status === '试用') return '试用';
  return '在职';
};

const toBackendEmployeeStatus = (status?: EmployeeStatus): string | undefined => {
  if (status === '离职') return 'inactive';
  if (status === '试用') return 'probation';
  if (status === '在职') return 'active';
  return undefined;
};

const normalizeEmployee = (employee: BackendEmployee): Employee => ({
  ...employee,
  department: employee.department ?? '',
  position: employee.position ?? '',
  id_card: employee.id_card ?? '',
  phone: employee.phone ?? '',
  bank_account: employee.bank_account ?? '',
  bank_name: employee.bank_name ?? '',
  hire_date: employee.hire_date ?? '',
  status: normalizeEmployeeStatus(employee.status),
  base_salary: numberOrZero(employee.base_salary),
  position_salary: numberOrZero(employee.position_salary),
  performance_salary: numberOrZero(employee.performance_salary),
  social_insurance_base: numberOrZero(employee.social_insurance_base),
  housing_fund_base: numberOrZero(employee.housing_fund_base),
  special_deduction: numberOrZero(employee.special_deduction),
  remark: employee.remark ?? '',
  created_at: employee.created_at ?? '',
  updated_at: employee.updated_at ?? '',
});

const toBackendEmployeeInput = (data: Partial<EmployeeInput>) => {
  const employeeNo = data.employee_no?.trim();
  return {
    ...data,
    employee_no: employeeNo,
    status: toBackendEmployeeStatus(data.status),
  };
};

const normalizeImportResult = (raw: ImportResult & { skipped?: number }): ImportResult => ({
  success: Boolean(raw.success),
  total: numberOrZero(raw.total),
  imported: numberOrZero(raw.imported),
  failed: numberOrZero(raw.failed ?? raw.skipped),
  errors: Array.isArray(raw.errors) ? raw.errors : [],
});

const normalizeAttendanceRecord = (record: BackendAttendanceRecord): AttendanceRecord => {
  const personalLeaveDays = numberOrZero(record.personal_leave_days);
  const sickLeaveDays = numberOrZero(record.sick_leave_days);

  return {
    id: record.id,
    month: record.salary_month ?? '',
    employee_id: record.id,
    employee_no: record.employee_no ?? '',
    employee_name: record.name ?? '',
    required_days: numberOrZero(record.expected_days),
    actual_days: numberOrZero(record.actual_days),
    late_count: numberOrZero(record.late_count),
    early_leave_count: numberOrZero(record.early_leave_count),
    leave_days: personalLeaveDays + sickLeaveDays,
    sick_leave_days: sickLeaveDays,
    personal_leave_days: personalLeaveDays,
    absent_days: numberOrZero(record.absent_days),
    overtime_hours: numberOrZero(record.overtime_hours),
    created_at: record.created_at ?? '',
    updated_at: record.updated_at ?? '',
  };
};

const normalizeSalaryStatus = (result: BackendSalaryResult): SalaryResult['status'] => {
  if (result.locked || result.status === 'locked' || result.status === '已锁定') return '已锁定';
  if (result.status === 'reviewed' || result.status === '已复核') return '已复核';
  return '草稿';
};

const normalizeSalaryResult = (result: BackendSalaryResult): SalaryResult => {
  const socialInsurance = numberOrZero(result.social_security_personal);
  const housingFund = numberOrZero(result.housing_fund_personal);
  const attendanceDeduction = numberOrZero(result.attendance_deduction);
  const incomeTax = numberOrZero(result.tax_amount);
  const otherDeduction = numberOrZero(result.other_deduction);

  return {
    id: result.id,
    month: result.salary_month ?? '',
    employee_id: result.id,
    employee_no: result.employee_no ?? '',
    employee_name: result.name ?? '',
    department: result.department ?? '',
    base_salary: numberOrZero(result.base_salary),
    position_salary: numberOrZero(result.position_salary),
    performance_salary: numberOrZero(result.performance_salary),
    overtime_pay: numberOrZero(result.overtime_salary),
    meal_allowance: numberOrZero(result.meal_allowance),
    transport_allowance: numberOrZero(result.transport_allowance),
    other_allowance: numberOrZero(result.other_allowance),
    gross_salary: numberOrZero(result.gross_salary),
    social_insurance: socialInsurance,
    housing_fund: housingFund,
    attendance_deduction: attendanceDeduction,
    income_tax: incomeTax,
    other_deduction: otherDeduction,
    total_deduction: socialInsurance + housingFund + attendanceDeduction + incomeTax + otherDeduction,
    net_salary: numberOrZero(result.net_salary),
    status: normalizeSalaryStatus(result),
    remark: result.remark ?? '',
    created_at: result.created_at ?? '',
    updated_at: result.updated_at ?? '',
  };
};

const toBackendAttendanceInput = (data: Partial<AttendanceRecordInput>, fallback?: AttendanceRecord) => {
  const extended = data as Partial<AttendanceRecordInput> & {
    employee_no?: string;
    employee_name?: string;
    remark?: string;
  };

  return {
    salary_month: data.month ?? fallback?.month ?? '',
    employee_no: extended.employee_no ?? fallback?.employee_no ?? '',
    name: extended.employee_name ?? fallback?.employee_name ?? '',
    expected_days: data.required_days ?? fallback?.required_days ?? 0,
    actual_days: data.actual_days ?? fallback?.actual_days ?? 0,
    late_count: data.late_count ?? fallback?.late_count ?? 0,
    early_leave_count: data.early_leave_count ?? fallback?.early_leave_count ?? 0,
    personal_leave_days: data.personal_leave_days ?? fallback?.personal_leave_days ?? 0,
    sick_leave_days: data.sick_leave_days ?? fallback?.sick_leave_days ?? 0,
    absent_days: data.absent_days ?? fallback?.absent_days ?? 0,
    overtime_hours: data.overtime_hours ?? fallback?.overtime_hours ?? 0,
    remark: extended.remark ?? '',
  };
};

export function normalizeDashboardSummary(raw: BackendDashboardSummary, month: string): DashboardSummary {
  const totalEmployees = numberOrZero(raw.employee_count);
  const calculatedCount = numberOrZero(raw.calculated_count);
  const totalGrossSalary = numberOrZero(raw.total_gross_salary);
  const totalNetSalary = numberOrZero(raw.total_net_salary);

  return {
    month,
    total_employees: totalEmployees,
    pending_count: Math.max(totalEmployees - calculatedCount, 0),
    calculated_count: calculatedCount,
    abnormal_attendance_count: numberOrZero(raw.attendance_count),
    total_gross_salary: totalGrossSalary,
    total_deduction: Math.max(totalGrossSalary - totalNetSalary, 0),
    total_net_salary: totalNetSalary,
  };
}

// ==================== 仪表盘 ====================

export async function getDashboardSummary(month: string): Promise<DashboardSummary> {
  const data = await invoke<BackendDashboardSummary>('get_dashboard_summary', { month });
  return normalizeDashboardSummary(data, month);
}

export async function getMonthCloseWorkbench(month: string): Promise<MonthCloseWorkbench> {
  return invoke<MonthCloseWorkbench>('get_month_close_workbench', { month });
}

export async function getFinancialAnalysis(query: FinancialAnalysisQuery): Promise<FinancialAnalysisReport> {
  return invoke<FinancialAnalysisReport>('get_financial_analysis', { query });
}

export async function exportDepartmentCostReport(query: FinancialAnalysisQuery, savePath: string): Promise<boolean> {
  return invoke<boolean>('export_department_cost_report', { query, path: savePath });
}

export async function exportExpenseAnalysisReport(query: FinancialAnalysisQuery, savePath: string): Promise<boolean> {
  return invoke<boolean>('export_expense_analysis_report', { query, path: savePath });
}

export async function exportMonthCloseReport(query: FinancialAnalysisQuery, savePath: string): Promise<boolean> {
  return invoke<boolean>('export_month_close_report', { query, path: savePath });
}

export async function queryOperationLogs(query: OperationLogQuery): Promise<OperationLog[]> {
  return invoke<OperationLog[]>('query_operation_logs', { query });
}

// ==================== 员工管理 ====================

export async function getEmployees(): Promise<Employee[]> {
  const employees = await invoke<BackendEmployee[]>('get_employees');
  return employees.map(normalizeEmployee);
}

export async function getEmployee(id: number): Promise<Employee> {
  const employee = await invoke<BackendEmployee>('get_employee', { id });
  return normalizeEmployee(employee);
}

export async function createEmployee(data: EmployeeInput): Promise<Employee> {
  const employee = await invoke<BackendEmployee>('create_employee', { data: toBackendEmployeeInput(data) });
  return normalizeEmployee(employee);
}

export async function updateEmployee(id: number, data: Partial<EmployeeInput>): Promise<Employee> {
  await invoke('update_employee', { id, data: toBackendEmployeeInput(data) });
  return getEmployee(id);
}

export async function deleteEmployee(id: number): Promise<void> {
  return invoke('delete_employee', { id });
}

export async function importEmployeesExcel(filePath: string): Promise<ImportResult> {
  const result = await invoke<ImportResult & { skipped?: number }>('import_employees_excel', { path: filePath });
  return normalizeImportResult(result);
}

export async function exportEmployeeImportTemplate(path: string): Promise<void> {
  await invoke('export_employee_import_template', { path });
}

// ==================== 考勤管理 ====================

export async function getAttendanceRecords(month: string): Promise<AttendanceRecord[]> {
  const records = await invoke<BackendAttendanceRecord[]>('get_attendance_records', { month });
  return records.map(normalizeAttendanceRecord);
}

export async function createAttendanceRecord(data: AttendanceRecordInput): Promise<AttendanceRecord> {
  const record = await invoke<BackendAttendanceRecord>('create_attendance_record', {
    data: toBackendAttendanceInput(data),
  });
  return normalizeAttendanceRecord(record);
}

export async function updateAttendanceRecord(
  id: number,
  data: Partial<AttendanceRecordInput>,
  fallback?: AttendanceRecord
): Promise<AttendanceRecord> {
  const existing = fallback ?? (data.month ? (await getAttendanceRecords(data.month)).find((record) => record.id === id) : undefined);
  await invoke('update_attendance_record', {
    id,
    data: toBackendAttendanceInput(data, existing),
  });
  return existing ? { ...existing, ...data } : { id, ...data } as AttendanceRecord;
}

export async function deleteAttendanceRecord(id: number): Promise<void> {
  return invoke('delete_attendance_record', { id });
}

export async function importAttendanceExcel(filePath: string, month: string): Promise<ImportResult> {
  const result = await invoke<ImportResult & { skipped?: number }>('import_attendance_excel', { path: filePath, month });
  return normalizeImportResult(result);
}

export async function exportAttendanceImportTemplate(path: string): Promise<void> {
  await invoke('export_attendance_import_template', { path });
}

// ==================== 工资规则 ====================

export async function getSalaryRule(): Promise<SalaryRule> {
  const rules = await invoke<BackendSalaryRule[]>('get_salary_rules');
  const byKey = new Map(rules.map((rule) => [rule.rule_key, rule]));

  return {
    id: 0,
    late_penalty: numberOrZero(byKey.get('late_penalty')?.rule_value),
    early_leave_penalty: numberOrZero(byKey.get('early_leave_penalty')?.rule_value),
    personal_leave_rate: numberOrZero(byKey.get('personal_leave_rate')?.rule_value),
    sick_leave_rate: numberOrZero(byKey.get('sick_leave_rate')?.rule_value),
    absent_rate: numberOrZero(byKey.get('absent_rate')?.rule_value),
    overtime_rate: numberOrZero(byKey.get('overtime_rate')?.rule_value),
    social_insurance_rate: numberOrZero(byKey.get('social_security_rate')?.rule_value) * 100,
    housing_fund_rate: numberOrZero(byKey.get('housing_fund_rate')?.rule_value) * 100,
    tax_threshold: numberOrZero(byKey.get('tax_threshold')?.rule_value),
    created_at: '',
    updated_at: '',
  };
}

export async function saveSalaryRule(data: SalaryRule): Promise<SalaryRule> {
  const rules = await invoke<BackendSalaryRule[]>('get_salary_rules');
  const byKey = new Map(rules.map((rule) => [rule.rule_key, rule.id]));
  const updates: Array<[string, number]> = [
    ['late_penalty', data.late_penalty],
    ['early_leave_penalty', data.early_leave_penalty],
    ['personal_leave_rate', data.personal_leave_rate],
    ['sick_leave_rate', data.sick_leave_rate],
    ['absent_rate', data.absent_rate],
    ['overtime_rate', data.overtime_rate],
    ['social_security_rate', data.social_insurance_rate / 100],
    ['housing_fund_rate', data.housing_fund_rate / 100],
    ['tax_threshold', data.tax_threshold],
  ];

  await Promise.all(
    updates.map(([key, ruleValue]) => {
      const id = byKey.get(key);
      return id ? invoke('update_salary_rule', { id, ruleValue }) : Promise.resolve();
    })
  );
  return getSalaryRule();
}

export async function getTaxRules(): Promise<TaxRule[]> {
  const rules = await invoke<BackendTaxRule[]>('get_tax_rules');
  return rules.map((rule, index) => ({
    id: rule.id,
    level: index + 1,
    min_amount: numberOrZero(rule.min_amount),
    max_amount: numberOrZero(rule.max_amount),
    tax_rate: numberOrZero(rule.tax_rate) * 100,
    quick_deduction: numberOrZero(rule.quick_deduction),
  }));
}

export async function saveTaxRules(rules: TaxRuleInput[]): Promise<TaxRule[]> {
  const existing = await invoke<BackendTaxRule[]>('get_tax_rules');
  await Promise.all(
    rules.slice(0, existing.length).map((rule, index) => invoke('update_tax_rule', {
      id: existing[index].id,
      data: {
        min_amount: rule.min_amount,
        max_amount: rule.max_amount || null,
        tax_rate: rule.tax_rate / 100,
        quick_deduction: rule.quick_deduction,
      },
    }))
  );
  return getTaxRules();
}

// ==================== 工资计算 ====================

export async function getSalaryResults(month: string): Promise<SalaryResult[]> {
  const results = await invoke<BackendSalaryResult[]>('get_salary_results', { month });
  return results.map(normalizeSalaryResult);
}

export async function calculateSalary(month: string): Promise<SalaryResult[]> {
  const results = await invoke<BackendSalaryResult[]>('calculate_salary', { month });
  return results.map(normalizeSalaryResult);
}

export async function recalculateSingle(month: string, employeeId: number): Promise<SalaryResult> {
  const existing = await getSalaryResults(month);
  const employeeNo = existing.find((result) => result.employee_id === employeeId)?.employee_no;
  if (!employeeNo) {
    throw new Error('未找到该员工工资记录');
  }

  const result = await invoke<BackendSalaryResult>('recalculate_employee', { month, employeeNo });
  return normalizeSalaryResult(result);
}

export async function updateSalaryResult(id: number, data: SalaryResultUpdate): Promise<SalaryResult> {
  await invoke('update_salary_result', { id, data });
  return { id, ...data } as SalaryResult;
}

export async function lockSalary(month: string): Promise<void> {
  await invoke('lock_salary_results', { month });
}

export async function reviewSalary(month: string): Promise<void> {
  await invoke('review_salary_results', { month });
}

// ==================== OCR ====================

export async function ocrRecognize(filePath: string, month: string, mode?: 'local' | 'online'): Promise<OcrResult> {
  const result = await invoke<{ batch_id: number; records: AttendanceRecordInput[]; raw_text?: string | null }>('ocr_recognize', {
    imagePath: filePath,
    month,
    mode: mode ?? 'local',
  });
  return {
    batch_id: result.batch_id,
    records: result.records,
    raw_text: result.raw_text ?? '',
  };
}

export async function getOcrBatches(month: string): Promise<OcrBatch[]> {
  const batches = await invoke<BackendOcrBatch[]>('get_ocr_batches', { month });
  return batches.map((batch) => {
    let resultCount = 0;
    if (batch.parsed_json) {
      try {
        const parsed = JSON.parse(batch.parsed_json);
        resultCount = Array.isArray(parsed) ? parsed.length : 0;
      } catch {
        resultCount = 0;
      }
    }

    return {
      id: batch.id,
      batch_name: batch.batch_name ?? `OCR-${batch.id}`,
      salary_month: batch.salary_month ?? undefined,
      file_path: batch.image_path ?? '',
      raw_text: batch.raw_text ?? '',
      parsed_json: batch.parsed_json ?? '',
      status: batch.status as OcrBatch['status'],
      result_count: resultCount,
      created_at: batch.created_at ?? '',
    };
  });
}

export async function confirmOcrResult(batchId: number, records: AttendanceRecordInput[]): Promise<void> {
  return invoke('confirm_ocr_results', { batchId, records });
}

// ==================== OCR Settings ====================

export async function getOcrSettings(): Promise<OcrSettings> {
  return invoke<OcrSettings>('get_ocr_settings');
}

export async function saveOcrSettings(data: OcrSettingsInput): Promise<void> {
  await invoke('save_ocr_settings', { data });
}

// ==================== 导出 ====================

export async function exportSalaryDetail(month: string, savePath: string): Promise<void> {
  return invoke('export_salary_detail', { month, path: savePath });
}

export async function exportBankPaymentFile(month: string, savePath: string): Promise<void> {
  return invoke('export_bank_payment_file', { month, path: savePath });
}

export async function exportSalarySlips(month: string, dir: string): Promise<void> {
  return invoke('export_salary_slips', { month, dir });
}

export async function exportAttendanceSummaryFile(month: string, savePath: string): Promise<void> {
  return invoke('export_attendance_summary_file', { month, path: savePath });
}

// ==================== Punch Card ====================

export async function generatePunchCardTemplate(
  path: string, month: string, department?: string,
): Promise<void> {
  await invoke('generate_punch_card_template', {
    path, month, department: department ?? '', position: '', shiftType: 'day',
  });
}

export async function ocrRecognizePunchCard(
  imagePath: string, month: string, mode?: 'local' | 'online',
): Promise<OcrResult> {
  const result = await invoke<{ batch_id: number; records: AttendanceRecordInput[]; raw_text?: string | null }>(
    'ocr_recognize_punch_card', {
      imagePath, month,
      shiftType: 'day',
      mode: mode ?? 'online',
    },
  );
  return { batch_id: result.batch_id, records: result.records, raw_text: result.raw_text ?? '' };
}

// ==================== 发票管理 ====================

export async function getInvoiceExpenseTypes(): Promise<InvoiceExpenseType[]> {
  return invoke<InvoiceExpenseType[]>('get_invoice_expense_types');
}

export async function saveInvoiceExpenseType(data: InvoiceExpenseTypeInput): Promise<InvoiceExpenseType> {
  return invoke<InvoiceExpenseType>('save_invoice_expense_type', { data });
}

export async function deleteInvoiceExpenseType(id: number): Promise<boolean> {
  return invoke<boolean>('delete_invoice_expense_type', { id });
}

export async function ocrInvoice(filePath: string): Promise<InvoiceOcrPreview> {
  return invoke<InvoiceOcrPreview>('ocr_invoice', { imagePath: filePath });
}

export async function saveInvoice(data: InvoiceInput): Promise<Invoice> {
  return invoke<Invoice>('save_invoice', { data });
}

export async function updateInvoice(id: number, data: InvoiceInput): Promise<boolean> {
  return invoke<boolean>('update_invoice', { id, data });
}

export async function deleteInvoice(id: number): Promise<boolean> {
  return invoke<boolean>('delete_invoice', { id });
}

export async function queryInvoices(query: InvoiceQuery): Promise<Invoice[]> {
  return invoke<Invoice[]>('query_invoices', { query });
}

export async function exportInvoiceList(query: InvoiceQuery, savePath: string): Promise<boolean> {
  return invoke<boolean>('export_invoice_list', { query, path: savePath });
}

// ==================== 报销管理 ====================

export async function queryReimbursementClaims(query: ReimbursementQuery): Promise<ReimbursementClaim[]> {
  return invoke<ReimbursementClaim[]>('query_reimbursement_claims', { query });
}

export async function saveReimbursementClaim(data: ReimbursementClaimInput): Promise<ReimbursementClaim> {
  return invoke<ReimbursementClaim>('save_reimbursement_claim', { data });
}

export async function getReimbursementInvoices(claimId: number): Promise<ReimbursementInvoice[]> {
  return invoke<ReimbursementInvoice[]>('get_reimbursement_invoices', { claimId });
}

export async function updateReimbursementClaimStatus(
  id: number,
  status?: ReimbursementStatus,
  paymentStatus?: PaymentStatus,
  paymentDate?: string,
): Promise<boolean> {
  return invoke<boolean>('update_reimbursement_claim_status', {
    id,
    status,
    paymentStatus,
    paymentDate,
  });
}

export async function deleteReimbursementClaim(id: number): Promise<boolean> {
  return invoke<boolean>('delete_reimbursement_claim', { id });
}
