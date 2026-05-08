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

// ==================== 仪表盘 ====================

export async function getDashboardSummary(month: string): Promise<DashboardSummary> {
  return invoke('get_dashboard_summary', { month });
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
  return invoke('get_salary_rule');
}

export async function saveSalaryRule(data: SalaryRule): Promise<SalaryRule> {
  return invoke('save_salary_rule', { data });
}

export async function getTaxRules(): Promise<TaxRule[]> {
  return invoke('get_tax_rules');
}

export async function saveTaxRules(rules: TaxRuleInput[]): Promise<TaxRule[]> {
  return invoke('save_tax_rules', { rules });
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

export async function ocrRecognize(filePath: string): Promise<OcrBatch> {
  return invoke('ocr_recognize', { filePath });
}

export async function getOcrBatches(): Promise<OcrBatch[]> {
  return invoke('get_ocr_batches');
}

export async function getOcrResults(batchId: number): Promise<OcrResult[]> {
  return invoke('get_ocr_results', { batchId });
}

export async function confirmOcrResult(resultId: number): Promise<void> {
  return invoke('confirm_ocr_result', { resultId });
}

export async function updateOcrResult(resultId: number, structuredData: Record<string, string>[]): Promise<OcrResult> {
  return invoke('update_ocr_result', { resultId, structuredData });
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
