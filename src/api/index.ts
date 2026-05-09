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
  ImportResult,
  DashboardSummary,
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

const numberOrZero = (value: unknown): number => (typeof value === 'number' && Number.isFinite(value) ? value : 0);

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

// ==================== 员工管理 ====================

export async function getEmployees(): Promise<Employee[]> {
  return invoke('get_employees');
}

export async function getEmployee(id: number): Promise<Employee> {
  return invoke('get_employee', { id });
}

export async function createEmployee(data: EmployeeInput): Promise<Employee> {
  return invoke('create_employee', { data });
}

export async function updateEmployee(id: number, data: Partial<EmployeeInput>): Promise<Employee> {
  return invoke('update_employee', { id, data });
}

export async function deleteEmployee(id: number): Promise<void> {
  return invoke('delete_employee', { id });
}

export async function importEmployeesExcel(filePath: string): Promise<ImportResult> {
  return invoke('import_employees_excel', { filePath });
}

// ==================== 考勤管理 ====================

export async function getAttendanceRecords(month: string): Promise<AttendanceRecord[]> {
  return invoke('get_attendance_records', { month });
}

export async function createAttendanceRecord(data: AttendanceRecordInput): Promise<AttendanceRecord> {
  return invoke('create_attendance_record', { data });
}

export async function updateAttendanceRecord(id: number, data: Partial<AttendanceRecordInput>): Promise<AttendanceRecord> {
  return invoke('update_attendance_record', { id, data });
}

export async function deleteAttendanceRecord(id: number): Promise<void> {
  return invoke('delete_attendance_record', { id });
}

export async function importAttendanceExcel(filePath: string, month: string): Promise<ImportResult> {
  return invoke('import_attendance_excel', { filePath, month });
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
  return invoke('get_salary_results', { month });
}

export async function calculateSalary(month: string): Promise<SalaryResult[]> {
  return invoke('calculate_salary', { month });
}

export async function recalculateSingle(month: string, employeeId: number): Promise<SalaryResult> {
  return invoke('recalculate_single', { month, employeeId });
}

export async function updateSalaryResult(id: number, data: SalaryResultUpdate): Promise<SalaryResult> {
  return invoke('update_salary_result', { id, data });
}

export async function lockSalary(month: string): Promise<void> {
  return invoke('lock_salary', { month });
}

export async function reviewSalary(month: string): Promise<void> {
  return invoke('review_salary', { month });
}

// ==================== OCR ====================

export async function ocrRecognize(filePath: string, month: string): Promise<OcrResult> {
  const result = await invoke<{ batch_id: number; records: AttendanceRecordInput[]; raw_text?: string | null }>('ocr_recognize', {
    imagePath: filePath,
    month,
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

// ==================== 导出 ====================

export async function exportSalaryDetail(month: string, savePath: string): Promise<void> {
  return invoke('export_salary_detail', { month, savePath });
}

export async function exportBankPaymentFile(month: string, savePath: string): Promise<void> {
  return invoke('export_bank_payment_file', { month, savePath });
}

export async function exportSalarySlips(month: string, savePath: string): Promise<void> {
  return invoke('export_salary_slips', { month, savePath });
}

export async function exportAttendanceSummaryFile(month: string, savePath: string): Promise<void> {
  return invoke('export_attendance_summary_file', { month, savePath });
}
