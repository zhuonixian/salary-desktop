import { invoke as tauriInvoke } from '@tauri-apps/api/core';
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
  MonthCloseRecord,
  MonthClosePackageResult,
  PaymentBatch,
  PaymentBatchDetail,
  PaymentBatchInput,
  PaymentBatchPaidInput,
  PaymentBatchQuery,
  PaymentBatchRemarkInput,
  PaymentBatchVoidInput,
  PaymentItem,
  BankAutoMatchPreviewItem,
  BankAutoMatchResult,
  BankAllocationBatchResult,
  BankAllocationInput,
  BankAllocationQuery,
  BankImportPreview,
  BankReconciliationAllocation,
  BankTransaction,
  BankTransactionIgnoreInput,
  BankTransactionMatch,
  BankTransactionMatchInput,
  BankTransactionQuery,
  LegacyBankMatchReport,
  FundJournal,
  FundJournalQuery,
  FundDailyReport,
  InputTaxLedgerReport,
  BankReconciliationPeriod,
  Budget,
  BudgetInput,
  BudgetQuery,
  FinancialAnalysisQuery,
  FinancialAnalysisReport,
  OperationLog,
  OperationLogQuery,
  DataSafetyStatus,
  DataBackupResult,
  DataRestoreResult,
  DataSafetyCheckResult,
  ReimbursementClaim,
  ReimbursementClaimInput,
  ReimbursementInvoice,
  ReimbursementQuery,
  ReminderItem,
  SecurityStatus,
  UnlockResult,
  RevealResult,
  LegacyMigrationStatus,
  GlAccount,
  GlAccountInput,
  OpeningBalanceRow,
  OpeningBalanceState,
  AccountMapping,
  Voucher,
  VoucherQuery,
  BalanceSheet,
  IncomeStatement,
  CashFlowStatement,
  TrialBalanceReport,
  AnnualTaxSummaryRow,
  FinancialReportType,
  SocialInsuranceProfile,
  SocialInsuranceProfileInput,
  FundAccount,
  FundAccountInput,
  FundAccountQuery,
  FundAssignmentEntityType,
  FundAssignmentInput,
  FundAssignmentPreview,
  FundAssignmentResult,
  FundMigrationStatus,
  BusinessPartner,
  BusinessPartnerInput,
  BusinessPartnerQuery,
  OperatorProfile,
  OperatorProfileInput,
  BusinessAttachment,
  BusinessAttachmentInput,
  ApprovalEvent,
  FundDocument,
  FundDocumentInput,
  FundDocumentQuery,
  AdvanceLedger,
  AdvanceLedgerQuery,
  AdvanceLedgerRow,
  AdvanceLinkCancelInput,
  AdvanceSettlementLink,
  FundDocumentDetail,
  FundDocumentReverseInput,
  NegotiableInstrument,
  InstrumentEndorsement,
  InstrumentQuery,
  InstrumentRegisterInput,
  InstrumentEndorseInput,
  InstrumentDiscountInput,
  InstrumentCollectConfirmInput,
  InstrumentSettleInput,
  InstrumentReverseInput,
  InstrumentEndorseResult,
  InstrumentDetail,
  CashCountSheet,
  CashCountDenomination,
  CashCountDenominationInput,
  CashCountCreateInput,
  CashCountUpdateInput,
  CashCountQuery,
  CashCountSheetDetail,
} from '@/types';
import { INSTRUMENT_STATUS_LABEL } from '@/types';

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
  fund_pending_approval_count?: number;
  fund_unpaid_count?: number;
  unassigned_bank_tx_count?: number;
  unreconciled_tx_count?: number;
  advance_overdue_count?: number;
  advance_overdue_amount?: number;
  fund_total_balance?: number;
};

type BackendEmployee = Omit<Employee, 'status'> & {
  status?: string | null;
};

type BackendSalaryRule = {
  id: number;
  rule_key: string;
  rule_value: number;
  enabled?: number;
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
  social_security_employer?: number | null;
  housing_fund_personal?: number | null;
  housing_fund_employer?: number | null;
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

const isTauriRuntime = (): boolean =>
  typeof window !== 'undefined' && Boolean((window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__);

const emptyMonthCloseSummary = (month = '') => ({
  month,
  active_employee_count: 0,
  attendance_count: 0,
  missing_attendance_count: 0,
  abnormal_attendance_count: 0,
  salary_count: 0,
  reviewed_count: 0,
  locked_count: 0,
  missing_bank_count: 0,
  invoice_count: 0,
  uncategorized_invoice_count: 0,
  reimbursement_count: 0,
  pending_reimbursement_count: 0,
  unpaid_reimbursement_count: 0,
  pending_payment_batch_count: 0,
  unmatched_paid_batch_count: 0,
  duplicate_amount_count: 0,
  over_budget_count: 0,
  total_salary_cost: 0,
  total_invoice_amount: 0,
  approved_reimbursement_amount: 0,
  paid_reimbursement_amount: 0,
  fund_pending_approval_count: 0,
  fund_unpaid_count: 0,
  unassigned_bank_tx_count: 0,
  partial_allocation_count: 0,
  advance_overdue_count: 0,
  advance_overdue_amount: 0,
});

// ==================== 出纳基础资料预览数据（第七阶段） ====================
// 浏览器预览用种子数据；字段结构与后端 models.rs 对应类型 1:1。
const mockFundAccounts: FundAccount[] = [
  {
    id: 1,
    account_code: 'BANK-001',
    name: '基本存款账户',
    account_type: 'bank',
    bank_name: '工商银行',
    account_no: '6222021234567890',
    currency: 'CNY',
    gl_account_code: '1002',
    opening_date: '2026-01-01',
    opening_balance: 50000,
    is_default: true,
    is_active: true,
    strict_reconciliation: false,
    remark: null,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
  },
  {
    id: 2,
    account_code: 'CASH-001',
    name: '备用金现金库',
    account_type: 'cash',
    bank_name: null,
    account_no: null,
    currency: 'CNY',
    gl_account_code: '1001',
    opening_date: '2026-01-01',
    opening_balance: 3000,
    is_default: true,
    is_active: true,
    strict_reconciliation: false,
    remark: null,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
  },
];

const mockBusinessPartners: BusinessPartner[] = [
  {
    id: 1,
    partner_code: 'GYS-001',
    name: '示例供应商',
    partner_type: 'supplier',
    tax_id: '91110000MA01X',
    contact_person: '王经理',
    phone: '13800138000',
    bank_name: '建设银行',
    bank_account: '6217001234567890',
    gl_account_code: null,
    status: 'active',
    remark: null,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
  },
];

const mockOperatorProfiles: OperatorProfile[] = [
  {
    id: 1,
    name: '张会计',
    role: 'cashier',
    is_active: true,
    remark: null,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
  },
  {
    id: 2,
    name: '李出纳',
    role: 'approver',
    is_active: true,
    remark: null,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
  },
];

// 预览模式下的当前操作人（内存态，模拟后端 CurrentOperatorState 会话）
let mockCurrentOperatorId: number | null = 1;

// 预览模式下的业务附件（内存态，模拟 business_attachments 表）
const mockBusinessAttachments: BusinessAttachment[] = [];

// ==================== 资金单据预览数据（第七阶段 Task 7） ====================
// 内存态模拟 fund_documents / approval_events / maker_checker_enabled；
// 状态流转与后端状态机同规则（仅演示用，完整校验以后端为准）。

const mockDocMonth = new Date().toISOString().slice(0, 7);

const baseMockFundDoc = (over: Partial<FundDocument>): FundDocument => ({
  id: 0,
  document_no: '',
  document_type: 'receipt',
  belong_month: mockDocMonth,
  document_date: `${mockDocMonth}-05`,
  amount: 0,
  summary: '',
  department: null,
  expense_type: null,
  remark: null,
  partner_id: null,
  employee_id: null,
  source_account_id: null,
  target_account_id: null,
  counter_account_code: null,
  settlement_mode: null,
  due_date: null,
  status: 'draft',
  payment_batch_id: null,
  reversal_of_id: null,
  submitted_by: null,
  submitted_at: null,
  approved_by: null,
  approved_at: null,
  settled_by: null,
  settled_at: null,
  voided_by: null,
  voided_at: null,
  created_by: null,
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
  ...over,
});

const mockFundDocuments: FundDocument[] = [
  baseMockFundDoc({
    id: 3,
    document_no: `SK${mockDocMonth.replace('-', '')}0003`,
    document_type: 'receipt',
    amount: 12000,
    summary: '收到客户样品款',
    partner_id: 1,
    target_account_id: 1,
    counter_account_code: '6001',
    status: 'settled',
    submitted_by: 1,
    submitted_at: '2026-01-02T00:00:00Z',
    approved_by: 2,
    approved_at: '2026-01-02T01:00:00Z',
    settled_by: 1,
    settled_at: '2026-01-02T02:00:00Z',
  }),
  baseMockFundDoc({
    id: 2,
    document_no: `FK${mockDocMonth.replace('-', '')}0002`,
    document_type: 'payment',
    amount: 5600,
    summary: '支付供应商货款',
    partner_id: 1,
    source_account_id: 1,
    counter_account_code: '2202',
    status: 'submitted',
    submitted_by: 1,
    submitted_at: '2026-01-02T00:00:00Z',
  }),
  baseMockFundDoc({
    id: 1,
    document_no: `NB${mockDocMonth.replace('-', '')}0001`,
    document_type: 'transfer',
    amount: 800,
    summary: '备用金划转现金库',
    source_account_id: 1,
    target_account_id: 2,
    status: 'draft',
  }),
];

// Task 14 预览种子：两笔借款（一笔部分核销、一笔未核销）
mockFundDocuments.unshift(
  baseMockFundDoc({
    id: 5,
    document_no: `JK${mockDocMonth.replace('-', '')}0005`,
    document_type: 'advance',
    amount: 2000,
    summary: '员工差旅备用金',
    employee_id: 2,
    source_account_id: 1,
    due_date: `${mockDocMonth}-28`,
    status: 'settled',
    submitted_by: 1,
    submitted_at: '2026-01-02T00:00:00Z',
    approved_by: 2,
    approved_at: '2026-01-02T01:00:00Z',
    settled_by: 1,
    settled_at: '2026-01-02T02:00:00Z',
  }),
  baseMockFundDoc({
    id: 4,
    document_no: `JK${mockDocMonth.replace('-', '')}0004`,
    document_type: 'advance',
    amount: 3000,
    summary: '员工出差借款',
    employee_id: 1,
    source_account_id: 1,
    due_date: `${mockDocMonth}-20`,
    status: 'settled',
    submitted_by: 1,
    submitted_at: '2026-01-02T00:00:00Z',
    approved_by: 2,
    approved_at: '2026-01-02T01:00:00Z',
    settled_by: 1,
    settled_at: '2026-01-02T02:00:00Z',
  }),
  baseMockFundDoc({
    id: 6,
    document_no: `HX${mockDocMonth.replace('-', '')}0006`,
    document_type: 'advance_settlement',
    amount: 1000,
    summary: '借款核销-现金归还',
    employee_id: 1,
    target_account_id: 2,
    settlement_mode: 'cash_return',
    status: 'settled',
    submitted_by: 1,
    submitted_at: '2026-01-03T00:00:00Z',
    approved_by: 2,
    approved_at: '2026-01-03T01:00:00Z',
    settled_by: 1,
    settled_at: '2026-01-03T02:00:00Z',
  }),
);

// 借款核销关系预览数据（模拟 advance_settlement_links）
const mockAdvanceLinks: AdvanceSettlementLink[] = [
  {
    id: 1,
    advance_id: 4,
    settlement_id: 6,
    allocated_amount: 1000,
    status: 'active',
    remark: null,
    created_at: '2026-01-03T02:00:00Z',
    cancelled_at: null,
    cancel_reason: null,
    settlement_document_no: `HX${mockDocMonth.replace('-', '')}0006`,
    settlement_date: `${mockDocMonth}-05`,
    settlement_status: 'settled',
    settlement_mode: 'cash_return',
  },
];

const mockNextAdvanceLinkId = (): number =>
  mockAdvanceLinks.reduce((max, l) => Math.max(max, l.id), 0) + 1;

const mockApprovalEvents: ApprovalEvent[] = [
  {
    id: 1,
    entity_type: 'fund_document',
    entity_id: 3,
    action: 'submit',
    from_status: 'draft',
    to_status: 'submitted',
    operator_id: 1,
    comment: null,
    created_at: '2026-01-02T00:00:00Z',
  },
  {
    id: 2,
    entity_type: 'fund_document',
    entity_id: 3,
    action: 'approve',
    from_status: 'submitted',
    to_status: 'approved',
    operator_id: 2,
    comment: '同意',
    created_at: '2026-01-02T01:00:00Z',
  },
  {
    id: 3,
    entity_type: 'fund_document',
    entity_id: 3,
    action: 'settle',
    from_status: 'approved',
    to_status: 'settled',
    operator_id: 1,
    comment: null,
    created_at: '2026-01-02T02:00:00Z',
  },
];

let mockMakerChecker = false;
// 账期提醒提前天数（预览态内存值，缺省 7 与后端 app_settings 缺省一致）
let mockReminderAdvanceDays = 7;

// ==================== 付款批次预览数据（第七阶段 Task 9） ====================
// 内存态模拟 payment_batches / payment_items 与批次-资金单状态机联动（演示用，
// 完整校验以后端为准）。general 批次与 mock 资金单联动：勾选单据 batched，付款后 settled。
const mockPaymentBatches: PaymentBatch[] = [];
const mockPaymentItems: PaymentItem[] = [];

const mockNextBatchId = (): number =>
  mockPaymentBatches.reduce((max, b) => Math.max(max, b.id), 0) + 1;
const mockNextItemId = (): number =>
  mockPaymentItems.reduce((max, i) => Math.max(max, i.id), 0) + 1;

const MOCK_BATCH_TYPE_PREFIX: Record<string, string> = {
  salary: 'GZ',
  reimbursement: 'BX',
  general: 'TY',
};

const mockFindBatch = (id: number): PaymentBatch => {
  const batch = mockPaymentBatches.find((b) => b.id === id);
  if (!batch) throw new Error(`付款批次ID=${id}未找到`);
  return batch;
};

const mockBatchItems = (batchId: number): PaymentItem[] =>
  mockPaymentItems.filter((i) => i.batch_id === batchId);

const mockNextFundDocId = (): number =>
  mockFundDocuments.reduce((max, d) => Math.max(max, d.id), 0) + 1;

const mockNextEventId = (): number =>
  mockApprovalEvents.reduce((max, e) => Math.max(max, e.id), 0) + 1;

const mockPushEvent = (
  entityId: number,
  action: string,
  fromStatus: string | null,
  toStatus: string | null,
  comment?: string | null,
): void => {
  mockApprovalEvents.push({
    id: mockNextEventId(),
    entity_type: 'fund_document',
    entity_id: entityId,
    action,
    from_status: fromStatus,
    to_status: toStatus,
    operator_id: mockCurrentOperatorId,
    comment: comment ?? null,
    created_at: new Date().toISOString(),
  });
};

// ==================== 票据台账预览数据（第八阶段） ====================
// 内存态轻量状态机：与后端 notes.rs 同规则（登记四组合、托收在途不记账、红字冲正恢复状态、
// 支票登记即终态/冲正即作废），演示 收到承兑→背书/贴现/托收到账 与 开出→兑付 全流程。
const mockMonthDate = (monthShift: number, day: number): string => {
  const d = new Date();
  d.setDate(1);
  d.setMonth(d.getMonth() + monthShift);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(day).padStart(2, '0')}`;
};

const baseMockInstrument = (over: Partial<NegotiableInstrument> & Pick<NegotiableInstrument, 'id' | 'instrument_type' | 'direction' | 'instrument_no' | 'face_amount' | 'issue_date' | 'due_date' | 'status'>): NegotiableInstrument => ({
  drawer: null,
  acceptor: null,
  payee: null,
  partner_id: null,
  fund_account_id: 1,
  counter_account_code: null,
  voucher_id: null,
  remark: null,
  created_by: '张会计',
  created_at: new Date().toISOString(),
  updated_at: new Date().toISOString(),
  ...over,
});

const mockInstruments: NegotiableInstrument[] = [
  // 收到承兑：holding（可走 背书/贴现/托收/作废）
  baseMockInstrument({
    id: 1, instrument_type: 'bank_acceptance', direction: 'received', instrument_no: 'YZ2026A001',
    face_amount: 100000, issue_date: mockMonthDate(0, 1), due_date: mockMonthDate(3, 1),
    drawer: '出票人甲公司', acceptor: 'XX银行', payee: '本公司', status: 'holding',
    remark: '收货款承兑',
  }),
  // 开出承兑：issued_outstanding（可兑付/作废）
  baseMockInstrument({
    id: 2, instrument_type: 'commercial_acceptance', direction: 'issued', instrument_no: 'YC2026B001',
    face_amount: 80000, issue_date: mockMonthDate(0, 5), due_date: mockMonthDate(2, 5),
    drawer: '本公司', acceptor: '本公司', payee: '供应商丙公司', status: 'issued_outstanding',
  }),
  // 收到支票：collected（登记即到账终态，冲正恢复走作废）
  baseMockInstrument({
    id: 3, instrument_type: 'check', direction: 'received', instrument_no: 'ZP2026C001',
    face_amount: 5000, issue_date: mockMonthDate(0, 10), due_date: mockMonthDate(0, 10),
    drawer: '客户丁公司', payee: '本公司', status: 'collected',
  }),
  // 开出支票：paid（登记即付款终态）
  baseMockInstrument({
    id: 4, instrument_type: 'check', direction: 'issued', instrument_no: 'ZP2026C002',
    face_amount: 3000, issue_date: mockMonthDate(0, 12), due_date: mockMonthDate(0, 12),
    drawer: '本公司', payee: '房东戊', status: 'paid',
  }),
  // 已背书转出（含背书链一条，冲正可恢复 holding）
  baseMockInstrument({
    id: 5, instrument_type: 'commercial_acceptance', direction: 'received', instrument_no: 'YZ2026A002',
    face_amount: 120000, issue_date: mockMonthDate(-1, 20), due_date: mockMonthDate(2, 20),
    drawer: '出票人己公司', acceptor: '己公司', payee: '本公司', status: 'endorsed_out',
  }),
  // 已贴现（冲正可恢复 holding）
  baseMockInstrument({
    id: 6, instrument_type: 'bank_acceptance', direction: 'received', instrument_no: 'YZ2026A003',
    face_amount: 60000, issue_date: mockMonthDate(-1, 15), due_date: mockMonthDate(1, 15),
    drawer: '出票人庚公司', acceptor: 'YY银行', payee: '本公司', status: 'discounted',
  }),
  // 托收中（可到账确认）
  baseMockInstrument({
    id: 7, instrument_type: 'bank_acceptance', direction: 'received', instrument_no: 'YZ2026A004',
    face_amount: 90000, issue_date: mockMonthDate(-2, 10), due_date: mockMonthDate(0, 30),
    drawer: '出票人辛公司', acceptor: 'ZZ银行', payee: '本公司', status: 'collecting',
  }),
  // 开出承兑已兑付（冲正可恢复 issued_outstanding）
  baseMockInstrument({
    id: 8, instrument_type: 'bank_acceptance', direction: 'issued', instrument_no: 'YC2026B002',
    face_amount: 70000, issue_date: mockMonthDate(-1, 1), due_date: mockMonthDate(0, 1),
    drawer: '本公司', acceptor: '本公司', payee: '供应商壬公司', status: 'paid',
  }),
  // 已作废（纯终态，无操作）
  baseMockInstrument({
    id: 9, instrument_type: 'check', direction: 'received', instrument_no: 'ZP2026C003',
    face_amount: 2000, issue_date: mockMonthDate(-1, 8), due_date: mockMonthDate(-1, 8),
    drawer: '客户癸公司', payee: '本公司', status: 'void',
    remark: '票面录错作废',
  }),
];

const mockInstrumentEndorsements: InstrumentEndorsement[] = [
  {
    id: 1,
    instrument_id: 5,
    endorse_order: 1,
    endorsee: '供应商丙公司',
    endorse_date: mockMonthDate(0, 2),
    purpose: '付货款',
    amount: 120000,
    voucher_id: 9001,
    created_by: '张会计',
    created_at: new Date().toISOString(),
  },
];

const mockNextInstrumentId = (): number =>
  mockInstruments.reduce((max, i) => Math.max(max, i.id), 0) + 1;

const mockFindInstrument = (id: number): NegotiableInstrument => {
  const inst = mockInstruments.find((i) => i.id === id);
  if (!inst) throw new Error(`票据ID=${id}未找到`);
  return inst;
};

const mockPushInstrumentEvent = (
  entityId: number,
  action: string,
  fromStatus: string | null,
  toStatus: string | null,
  comment?: string | null,
): void => {
  mockApprovalEvents.push({
    id: mockNextEventId(),
    entity_type: 'negotiable_instrument',
    entity_id: entityId,
    action,
    from_status: fromStatus,
    to_status: toStatus,
    operator_id: mockCurrentOperatorId,
    comment: comment ?? null,
    created_at: new Date().toISOString(),
  });
};

// 票据轻量状态机：与后端 notes.rs transition_instrument 同规则（来源状态门禁 + 原因必填）
const mockTransitionInstrument = (
  id: number,
  fromStatuses: string[],
  toStatus: string,
  action: string,
  comment?: string | null,
  requireComment = false,
): NegotiableInstrument => {
  const inst = mockFindInstrument(id);
  if (!fromStatuses.includes(inst.status)) {
    throw new Error(
      `票据 ${inst.instrument_no} 当前状态「${INSTRUMENT_STATUS_LABEL[inst.status] ?? inst.status}」不允许该操作`,
    );
  }
  const trimmed = (comment ?? '').trim();
  if (requireComment && !trimmed) throw new Error('该操作必须填写原因');
  inst.status = toStatus;
  inst.updated_at = new Date().toISOString();
  mockPushInstrumentEvent(inst.id, action, fromStatuses[0] ?? null, toStatus, trimmed || null);
  return inst;
};

// ==================== 现金盘点单（第八阶段 Task 6） ====================
// 内存态轻量模拟，校验口径与后端 cash_count.rs 一致（账户限 cash、面额合计=实存、
// 差异≠0 原因必填、confirmed 不可改、作废差异凭证走红字冲正）；凭证不模拟（voucher_id 占位）。

interface MockCashCountSheet extends CashCountSheet {
  denominations: CashCountDenominationInput[];
  reversed: boolean;
}

let mockCashCountSeq = 0;
const mockCashCountSheets: MockCashCountSheet[] = [];

const baseMockCashCountSheet = (
  over: Partial<MockCashCountSheet> &
    Pick<MockCashCountSheet, 'id' | 'count_date' | 'fund_account_id' | 'book_balance' | 'counted_amount' | 'status'>,
): MockCashCountSheet => ({
  belong_month: over.count_date.slice(0, 7),
  difference: Number(((over.counted_amount ?? 0) - (over.book_balance ?? 0)).toFixed(2)),
  difference_reason: null,
  voucher_id: null,
  remark: null,
  created_by: '张会计',
  created_at: new Date().toISOString(),
  updated_at: new Date().toISOString(),
  denominations: [],
  reversed: false,
  ...over,
});

// 演示数据：现金账户 id=2（mock 备用金现金库，期初 3000）；mock 不落凭证，voucher_id 置空
mockCashCountSheets.push(
  baseMockCashCountSheet({
    id: ++mockCashCountSeq,
    count_date: mockMonthDate(0, 5),
    fund_account_id: 2,
    book_balance: 3000,
    counted_amount: 3000,
    status: 'confirmed',
    remark: '月末例行盘点',
  }),
  baseMockCashCountSheet({
    id: ++mockCashCountSeq,
    count_date: mockMonthDate(0, 12),
    fund_account_id: 2,
    book_balance: 3000,
    counted_amount: 2950,
    difference: -50,
    difference_reason: '找零垫付未入账',
    status: 'draft',
    denominations: [
      { denomination: 100, quantity: 20 },
      { denomination: 50, quantity: 10 },
      { denomination: 20, quantity: 10 },
      { denomination: 10, quantity: 12 },
      { denomination: 5, quantity: 6 },
      { denomination: 1, quantity: 20 },
      { denomination: 0.5, quantity: 4 },
      { denomination: 0.1, quantity: 10 },
    ],
  }),
  baseMockCashCountSheet({
    id: ++mockCashCountSeq,
    count_date: mockMonthDate(-1, 28),
    fund_account_id: 2,
    book_balance: 2800,
    counted_amount: 2800,
    status: 'void',
    remark: '盘面录错作废',
  }),
);

const mockFindCashCountSheet = (id: number): MockCashCountSheet => {
  const sheet = mockCashCountSheets.find((s) => s.id === id);
  if (!sheet) throw new Error(`盘点单不存在：id=${id}`);
  return sheet;
};

// 剥离 mock 辅助字段（denominations/reversed），返回与后端 CashCountSheet 同构的公开结构
const mockSheetToPublic = (sheet: MockCashCountSheet): CashCountSheet => ({
  id: sheet.id,
  count_date: sheet.count_date,
  belong_month: sheet.belong_month,
  fund_account_id: sheet.fund_account_id,
  book_balance: sheet.book_balance,
  counted_amount: sheet.counted_amount,
  difference: sheet.difference,
  difference_reason: sheet.difference_reason,
  status: sheet.status,
  voucher_id: sheet.voucher_id,
  remark: sheet.remark,
  created_by: sheet.created_by,
  created_at: sheet.created_at,
  updated_at: sheet.updated_at,
});

const mockValidateDenominations = (
  counted: number,
  denominations: CashCountDenominationInput[] | null | undefined,
): void => {
  if (!denominations || denominations.length === 0) return;
  const seen = new Set<number>();
  let total = 0;
  for (const d of denominations) {
    if (!(Number(d.denomination) > 0)) throw new Error(`面额必须大于 0：${d.denomination}`);
    if (Number(d.quantity) < 0) throw new Error(`面额 ${d.denomination} 张数不能为负`);
    const key = Math.round(Number(d.denomination) * 1_000_000);
    if (seen.has(key)) throw new Error(`面额 ${d.denomination} 重复录入，请合并为一条`);
    seen.add(key);
    total += Number(d.denomination) * Number(d.quantity);
  }
  if (Math.abs(total - counted) > 0.005) {
    throw new Error(`面额明细合计 ${total.toFixed(2)} 与实存金额 ${counted.toFixed(2)} 不一致，请核对后保存`);
  }
};

const mockAssertCashAccount = (accountId: number): void => {
  const account = mockFundAccounts.find((a) => a.id === accountId);
  if (!account) throw new Error(`资金账户不存在：id=${accountId}`);
  if (account.account_type !== 'cash') {
    throw new Error(`现金盘点仅支持现金类账户，账户「${account.name}」类型为 ${account.account_type}`);
  }
};

// 轻量状态机模拟：与后端 cashier.rs 同规则（演示完整 草稿→提交→审批→结算 流程）
const mockTransitionFundDocument = (
  id: number,
  fromStatuses: string[],
  toStatus: string,
  action: string,
  comment?: string | null,
  requireComment = false,
): FundDocument => {
  const doc = mockFundDocuments.find((d) => d.id === id);
  if (!doc) throw new Error(`资金单据ID=${id}未找到`);
  if (!fromStatuses.includes(doc.status)) {
    throw new Error(`单据 ${doc.document_no} 当前状态不允许该操作`);
  }
  const trimmed = (comment ?? '').trim();
  if (requireComment && !trimmed) throw new Error('该操作必须填写意见或原因');
  if (
    action === 'approve' &&
    mockMakerChecker &&
    doc.submitted_by !== null &&
    doc.submitted_by === mockCurrentOperatorId
  ) {
    throw new Error('经办复核已启用：审批人与提交人不能是同一人，请切换操作人后再审批');
  }
  if (action === 'settle' && doc.status === 'approved' &&
      !['receipt', 'transfer', 'advance_settlement'].includes(doc.document_type)) {
    throw new Error('付款单/借款单须经付款批次标记付款后结算');
  }
  const now = new Date().toISOString();
  if (action === 'submit') {
    doc.submitted_by = mockCurrentOperatorId;
    doc.submitted_at = now;
  } else if (action === 'approve') {
    doc.approved_by = mockCurrentOperatorId;
    doc.approved_at = now;
  } else if (action === 'settle') {
    doc.settled_by = mockCurrentOperatorId;
    doc.settled_at = now;
  } else if (action === 'void') {
    doc.voided_by = mockCurrentOperatorId;
    doc.voided_at = now;
  }
  doc.status = toStatus;
  doc.updated_at = now;
  mockPushEvent(doc.id, action, fromStatuses[0] ?? null, toStatus, trimmed || null);
  return doc;
};

const mockTauriResponse = (command: string, args?: Record<string, unknown>): unknown => {
  switch (command) {
    case 'get_fund_accounts':
      return mockFundAccounts;
    case 'save_fund_account': {
      const data = args?.data as FundAccountInput | undefined;
      return {
        id: data?.id ?? Date.now(),
        currency: 'CNY',
        opening_balance: 0,
        is_default: false,
        is_active: true,
        strict_reconciliation: false,
        bank_name: null,
        account_no: null,
        opening_date: null,
        remark: null,
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString(),
        ...data,
      };
    }
    case 'set_active_fund_account': {
      const seed = mockFundAccounts[0];
      return { ...seed, is_active: Boolean(args?.active) };
    }
    // 历史归集向导（Task 10）：mock 为零待归集静态回显，完整校验以后端为准
    case 'get_fund_migration_status':
      return {
        unassigned_bank_transactions: 0,
        unassigned_payment_batches: 0,
        unassigned_voucher_lines: 0,
        pending_count: 0,
        bank_months: [],
        pending_batches: [],
        unlinked_voucher_lines: 0,
        completed_at: new Date().toISOString(),
        last_applied_at: null,
      };
    case 'preview_fund_assignment':
      return {
        entity_type: String(args?.entityType ?? ''),
        item_count: 0,
        affected_voucher_lines: 0,
        skipped_voucher_lines: 0,
      };
    case 'apply_fund_assignment':
      return { updated_count: 0, linked_voucher_lines_updated: 0, skipped_voucher_lines: 0 };
    case 'get_business_partners':
      return mockBusinessPartners;
    case 'save_business_partner': {
      const data = args?.data as BusinessPartnerInput | undefined;
      return {
        id: data?.id ?? Date.now(),
        status: 'active',
        tax_id: null,
        contact_person: null,
        phone: null,
        bank_name: null,
        bank_account: null,
        gl_account_code: null,
        remark: null,
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString(),
        ...data,
      };
    }
    case 'set_active_business_partner': {
      const seed = mockBusinessPartners[0];
      return { ...seed, status: args?.active ? 'active' : 'inactive' };
    }
    case 'get_operator_profiles':
      return mockOperatorProfiles;
    case 'save_operator_profile': {
      const data = args?.data as OperatorProfileInput | undefined;
      const saved: OperatorProfile = {
        id: data?.id ?? Date.now(),
        name: data?.name ?? '预览操作人',
        role: data?.role ?? 'cashier',
        is_active: data?.is_active ?? true,
        remark: data?.remark ?? null,
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString(),
      };
      const idx = mockOperatorProfiles.findIndex((p) => p.id === saved.id);
      if (idx >= 0) {
        mockOperatorProfiles[idx] = saved;
      } else {
        mockOperatorProfiles.push(saved);
      }
      return saved;
    }
    case 'set_active_operator_profile': {
      const id = Number(args?.id ?? 0);
      const active = Boolean(args?.active);
      const target = mockOperatorProfiles.find((p) => p.id === id) ?? mockOperatorProfiles[0];
      const updated = { ...target, is_active: active };
      const idx = mockOperatorProfiles.findIndex((p) => p.id === updated.id);
      if (idx >= 0) mockOperatorProfiles[idx] = updated;
      // 与后端一致：停用当前操作人时清空会话，要求重新选择。
      if (!active && mockCurrentOperatorId === updated.id) mockCurrentOperatorId = null;
      return updated;
    }
    case 'set_current_operator': {
      const id = Number(args?.operatorId ?? 0);
      const target = mockOperatorProfiles.find((p) => p.id === id && p.is_active);
      if (!target) {
        throw new Error('操作人不存在或已停用，请重新选择');
      }
      mockCurrentOperatorId = target.id;
      return target;
    }
    case 'get_current_operator':
      return mockOperatorProfiles.find((p) => p.id === mockCurrentOperatorId && p.is_active) ?? null;
    // 业务附件 mock：内存态回显，字段结构与后端 BusinessAttachment 一致
    case 'add_business_attachment': {
      const data = args?.data as BusinessAttachmentInput | undefined;
      const saved: BusinessAttachment = {
        id: Date.now(),
        entity_type: data?.entity_type ?? 'fund_document',
        entity_id: data?.entity_id ?? 0,
        file_name: data?.file_name || data?.file_path?.split('/').pop() || 'attachment.bin',
        file_path: data?.file_path ?? '',
        encrypted: true,
        file_size: data?.file_size ?? 0,
        belong_month: data?.belong_month ?? null,
        uploaded_by: '张会计',
        created_at: new Date().toISOString(),
      };
      mockBusinessAttachments.push(saved);
      return saved;
    }
    case 'list_business_attachments':
      return mockBusinessAttachments.filter(
        (a) =>
          a.entity_type === (args?.entityType as string) &&
          a.entity_id === Number(args?.entityId ?? 0),
      );
    case 'delete_business_attachment': {
      const id = Number(args?.id ?? 0);
      const idx = mockBusinessAttachments.findIndex((a) => a.id === id);
      if (idx < 0) throw new Error(`附件ID=${id}未找到`);
      const [removed] = mockBusinessAttachments.splice(idx, 1);
      return removed.file_name;
    }
    case 'get_decrypted_attachment_url':
      // 浏览器预览无本地文件系统：返回空串，页面按"预览不可用"处理
      return '';
    // 付款批次 mock：内存态批次 + 与 mock 资金单的状态机联动（spec 5.3 同规则）
    case 'query_payment_batches': {
      const query = (args?.query ?? {}) as PaymentBatchQuery;
      return mockPaymentBatches
        .filter((b) => {
          if (query.belong_month && b.belong_month !== query.belong_month) return false;
          if (query.batch_type && b.batch_type !== query.batch_type) return false;
          if (query.status && b.status !== query.status) return false;
          return true;
        })
        .sort((a, b) => (a.created_at ?? '').localeCompare(b.created_at ?? '') || b.id - a.id);
    }
    case 'get_payment_batch_detail': {
      const batch = mockFindBatch(Number(args?.id ?? 0));
      return { batch, items: mockBatchItems(batch.id) };
    }
    case 'create_payment_batch': {
      const data = args?.data as PaymentBatchInput | undefined;
      if (!data?.belong_month || !['salary', 'reimbursement', 'general'].includes(data.batch_type)) {
        throw new Error('付款批次类型无效');
      }
      if (!data.fund_account_id) throw new Error('请选择付款资金账户');
      const account = mockFundAccounts.find((a) => a.id === data.fund_account_id && a.is_active);
      if (!account) throw new Error('资金账户不存在或已停用');

      // 候选明细：general 从 mock 已审批付款/借款单勾选（同账户同月）；
      // salary/reimbursement 生成两条员工示例明细
      let candidates: PaymentItem[];
      const now = new Date().toISOString();
      if (data.batch_type === 'general') {
        const picked = mockFundDocuments.filter((d) => {
          if (d.status !== 'approved' || !['payment', 'advance'].includes(d.document_type)) return false;
          if (d.belong_month !== data.belong_month) return false;
          if (d.source_account_id !== data.fund_account_id) return false;
          if (data.source_ids && !data.source_ids.includes(d.id)) return false;
          return !mockPaymentItems.some(
            (i) => i.source_type === 'fund_document' && i.source_id === d.id && i.status !== 'void',
          );
        });
        if (!picked.length) throw new Error('没有可生成付款批次的明细');
        candidates = picked.map((d) => ({
          id: mockNextItemId(),
          batch_id: 0,
          source_type: 'fund_document' as const,
          source_id: d.id,
          employee_id: d.employee_id ?? undefined,
          employee_no: d.employee_id != null ? `E${d.employee_id}` : d.partner_id != null ? `P${d.partner_id}` : undefined,
          employee_name:
            d.employee_id != null
              ? `员工${d.employee_id}`
              : mockBusinessPartners.find((p) => p.id === d.partner_id)?.name ?? '往来单位',
          bank_name: '工商银行',
          bank_account: '6222021234567890',
          amount: d.amount,
          status: 'pending',
          remark: d.document_no,
          created_at: now,
        }));
      } else {
        candidates = [1, 2].map((n) => ({
          id: mockNextItemId(),
          batch_id: 0,
          source_type: (data.batch_type === 'salary' ? 'salary_result' : 'reimbursement_claim') as PaymentItem['source_type'],
          source_id: n,
          employee_id: n,
          employee_no: `E00${n}`,
          employee_name: n === 1 ? '张三' : '李四',
          bank_name: '工商银行',
          bank_account: n === 1 ? '6222021234567891' : '6222021234567892',
          amount: data.batch_type === 'salary' ? 7800 : 500,
          status: 'pending',
          remark: undefined,
          created_at: now,
        }));
      }
      if (data.batch_type === 'general') {
        for (const item of candidates) {
          mockTransitionFundDocument(item.source_id, ['approved'], 'batched', 'batch');
        }
      }
      const batch: PaymentBatch = {
        id: mockNextBatchId(),
        batch_no: `${MOCK_BATCH_TYPE_PREFIX[data.batch_type]}${data.belong_month.replace('-', '')}${String(Date.now()).slice(-6)}`,
        belong_month: data.belong_month,
        batch_type: data.batch_type,
        status: 'draft',
        total_amount: candidates.reduce((s, i) => s + i.amount, 0),
        item_count: candidates.length,
        payment_date: undefined,
        remark: data.remark,
        fund_account_id: account.id,
        fund_account_name: account.name,
        created_at: now,
        updated_at: now,
      };
      mockPaymentBatches.unshift(batch);
      for (const item of candidates) {
        mockPaymentItems.push({ ...item, batch_id: batch.id });
      }
      return { batch, items: mockBatchItems(batch.id) };
    }
    case 'export_payment_batch_file': {
      const batch = mockFindBatch(Number(args?.id ?? 0));
      if (batch.status === 'void') throw new Error('已作废付款批次不能导出');
      if (batch.status !== 'paid') batch.status = 'exported';
      return batch;
    }
    case 'mark_payment_batch_paid': {
      const data = args?.data as PaymentBatchPaidInput | undefined;
      const batch = mockFindBatch(Number(data?.id ?? 0));
      if (batch.status === 'void') throw new Error('已作废付款批次不能标记付款');
      if (batch.status !== 'exported') throw new Error('付款批次必须先导出后才能标记已付款');
      if (batch.batch_type === 'general') {
        for (const item of mockBatchItems(batch.id)) {
          if (item.source_type === 'fund_document') {
            mockTransitionFundDocument(item.source_id, ['batched'], 'settled', 'settle');
          }
        }
      }
      batch.status = 'paid';
      batch.payment_date = data?.payment_date;
      batch.updated_at = new Date().toISOString();
      return batch;
    }
    case 'void_payment_batch': {
      const data = args?.data as PaymentBatchVoidInput | undefined;
      const batch = mockFindBatch(Number(data?.id ?? 0));
      if (batch.status === 'void') return batch;
      if (batch.batch_type === 'general' && batch.status === 'paid') {
        throw new Error('已付款的通用付款批次不可作废，付款错误请通过资金单冲正处理');
      }
      if (batch.batch_type === 'general') {
        for (const item of mockBatchItems(batch.id)) {
          if (item.source_type === 'fund_document') {
            mockTransitionFundDocument(item.source_id, ['batched'], 'approved', 'unbatch');
          }
        }
      }
      batch.status = 'void';
      batch.remark = data?.reason ?? batch.remark;
      batch.updated_at = new Date().toISOString();
      for (const item of mockBatchItems(batch.id)) item.status = 'void';
      return batch;
    }
    case 'update_payment_batch_remark': {
      const data = args?.data as PaymentBatchRemarkInput | undefined;
      const batch = mockFindBatch(Number(data?.id ?? 0));
      if (batch.status === 'void') throw new Error('已作废付款批次不能修改备注');
      batch.remark = data?.remark ?? batch.remark;
      return batch;
    }
    // 资金单据 mock：内存态轻量状态机，字段结构与后端 FundDocument/ApprovalEvent 一致
    case 'get_fund_documents': {
      const query = (args?.query ?? {}) as FundDocumentQuery;
      return mockFundDocuments.filter((d) => {
        if (query.belong_month && d.belong_month !== query.belong_month) return false;
        if (query.document_type && d.document_type !== query.document_type) return false;
        if (query.status && d.status !== query.status) return false;
        if (query.partner_id && d.partner_id !== query.partner_id) return false;
        if (query.employee_id && d.employee_id !== query.employee_id) return false;
        if (
          query.account_id &&
          d.source_account_id !== query.account_id &&
          d.target_account_id !== query.account_id
        ) {
          return false;
        }
        if (query.keyword && !`${d.document_no}${d.summary}`.includes(query.keyword)) return false;
        return true;
      });
    }
    case 'get_fund_document_detail': {
      const id = Number(args?.id ?? 0);
      const document = mockFundDocuments.find((d) => d.id === id);
      if (!document) throw new Error(`资金单据ID=${id}未找到`);
      return {
        document,
        events: mockApprovalEvents.filter(
          (e) => e.entity_type === 'fund_document' && e.entity_id === id,
        ),
      };
    }
    case 'list_approval_events':
      return mockApprovalEvents.filter(
        (e) =>
          e.entity_type === (args?.entityType as string) &&
          e.entity_id === Number(args?.entityId ?? 0),
      );
    case 'get_maker_checker_enabled':
      return mockMakerChecker;
    case 'set_maker_checker_enabled':
      mockMakerChecker = Boolean(args?.enabled);
      return undefined;
    case 'create_fund_document': {
      const data = args?.data as FundDocumentInput | undefined;
      if (!data || !(Number(data.amount) > 0) || !data.summary?.trim()) {
        throw new Error('单据金额必须为正数且摘要必填');
      }
      const now = new Date().toISOString();
      const prefixes: Record<string, string> = {
        receipt: 'SK', payment: 'FK', transfer: 'NB', advance: 'JK', advance_settlement: 'HX',
      };
      // Task 14：核销单必须关联已发放借款且分摊合计一致（与后端 validate_advance_allocations 同口径）
      const allocs = data.advance_allocations ?? [];
      if (data.document_type === 'advance_settlement') {
        if (allocs.length === 0) throw new Error('借款核销单必须关联至少一笔员工借款单');
        const allocated = allocs.reduce((s, a) => s + Number(a.amount || 0), 0);
        if (Math.abs(allocated - Number(data.amount)) > 0.005) {
          throw new Error('核销关联金额合计与单据金额不一致');
        }
        // 同单去重：同一借款重复关联时逐条校验各自通过、合计击穿借款上限（与后端同口径）
        const perAdvance = new Map<number, number>();
        for (const a of allocs) {
          if (perAdvance.has(a.advance_id)) throw new Error('同一张核销单不能重复关联同一笔借款');
          perAdvance.set(a.advance_id, (perAdvance.get(a.advance_id) ?? 0) + Number(a.amount || 0));
        }
        for (const alloc of allocs) {
          const loan = mockFundDocuments.find((d) => d.id === alloc.advance_id);
          if (!loan || loan.document_type !== 'advance') throw new Error('关联的员工借款单不存在');
          if (loan.status !== 'settled') throw new Error(`借款单 ${loan.document_no} 尚未发放，只能核销已发放的借款`);
          if (loan.employee_id !== data.employee_id) throw new Error('借款单的员工与核销单不一致，不能核销他人借款');
          const settledSum = mockAdvanceLinks
            .filter((l) => l.advance_id === loan.id && l.status === 'active')
            .reduce((s, l) => s + l.allocated_amount, 0);
          const allocAmount = perAdvance.get(alloc.advance_id) ?? Number(alloc.amount);
          if (settledSum + allocAmount > loan.amount + 0.005) {
            throw new Error(`借款单 ${loan.document_no} 累计核销超过借款金额`);
          }
        }
      }
      const doc = baseMockFundDoc({
        ...data,
        id: mockNextFundDocId(),
        document_no: `${prefixes[data.document_type] ?? 'CZ'}${data.document_date.replace(/-/g, '')}${String(Date.now()).slice(-4)}`,
        amount: Number(data.amount),
        summary: data.summary.trim(),
        created_at: now,
        updated_at: now,
      });
      mockFundDocuments.unshift(doc);
      if (data.document_type === 'advance_settlement') {
        for (const alloc of allocs) {
          mockAdvanceLinks.push({
            id: mockNextAdvanceLinkId(),
            advance_id: alloc.advance_id,
            settlement_id: doc.id,
            allocated_amount: Number(alloc.amount),
            status: 'active',
            remark: null,
            created_at: now,
            cancelled_at: null,
            cancel_reason: null,
            settlement_document_no: doc.document_no,
            settlement_date: doc.document_date,
            settlement_status: doc.status,
            settlement_mode: data.settlement_mode ?? 'cash_return',
          });
        }
      }
      return doc;
    }
    case 'update_fund_document': {
      const data = args?.data as FundDocumentInput | undefined;
      const doc = mockFundDocuments.find((d) => d.id === Number(data?.id ?? 0));
      if (!doc) throw new Error('资金单据未找到');
      if (doc.status !== 'draft') throw new Error('仅草稿可编辑；已提交单据请先撤回');
      Object.assign(doc, data, { updated_at: new Date().toISOString() });
      return doc;
    }
    case 'submit_fund_document':
      return mockTransitionFundDocument(
        Number(args?.id ?? 0), ['draft'], 'submitted', 'submit', args?.comment as string | undefined,
      );
    case 'approve_fund_document':
      return mockTransitionFundDocument(
        Number(args?.id ?? 0), ['submitted'], 'approved', 'approve', args?.comment as string, true,
      );
    case 'reject_fund_document':
      return mockTransitionFundDocument(
        Number(args?.id ?? 0), ['submitted'], 'rejected', 'reject', args?.comment as string, true,
      );
    case 'withdraw_fund_document':
      return mockTransitionFundDocument(
        Number(args?.id ?? 0), ['submitted', 'rejected'], 'draft', 'withdraw', args?.comment as string | undefined,
      );
    case 'void_fund_document':
      return mockTransitionFundDocument(
        Number(args?.id ?? 0), ['draft', 'submitted', 'approved', 'rejected'], 'void', 'void', args?.comment as string, true,
      );
    case 'settle_fund_document':
      return mockTransitionFundDocument(Number(args?.id ?? 0), ['approved', 'batched'], 'settled', 'settle');
    case 'reverse_fund_document': {
      const data = args?.data as FundDocumentReverseInput | undefined;
      const original = mockFundDocuments.find((d) => d.id === Number(data?.document_id ?? 0));
      if (!original) throw new Error('资金单据未找到');
      if (original.status !== 'settled') throw new Error('仅已结算单据可冲正');
      if (!data?.comment?.trim()) throw new Error('冲正必须填写原因');
      const now = new Date().toISOString();
      const reversal = baseMockFundDoc({
        id: mockNextFundDocId(),
        document_no: `CZ${data.document_date.replace(/-/g, '')}${String(Date.now()).slice(-4)}`,
        document_type: 'reversal',
        belong_month: data.belong_month,
        document_date: data.document_date,
        amount: original.amount,
        summary: `冲正：${original.summary}`,
        department: original.department,
        expense_type: original.expense_type,
        remark: `冲正原单 ${original.document_no}，原因：${data.comment.trim()}`,
        partner_id: original.partner_id,
        employee_id: original.employee_id,
        source_account_id: original.target_account_id,
        target_account_id: original.source_account_id,
        counter_account_code: original.counter_account_code,
        status: 'settled',
        reversal_of_id: original.id,
        settled_by: mockCurrentOperatorId,
        settled_at: now,
        created_at: now,
        updated_at: now,
      });
      mockFundDocuments.unshift(reversal);
      original.status = 'reversed';
      original.updated_at = now;
      mockPushEvent(original.id, 'reverse', 'settled', 'reversed', data.comment.trim());
      mockPushEvent(reversal.id, 'reverse', null, 'settled', data.comment.trim());
      return reversal;
    }
    // ==================== 票据台账（第八阶段） ====================
    // 内存态轻量状态机，校验口径与后端 notes.rs 一致（支票登记即终态、托收在途不记账、
    // 贴现实收不得大于票面、背书全额、void/reverse 原因必填）；凭证不模拟（voucher_id 置空/占位）。
    case 'get_negotiable_instruments': {
      const query = (args?.query ?? {}) as InstrumentQuery;
      return mockInstruments.filter((i) => {
        if (query.direction && i.direction !== query.direction) return false;
        if (query.instrument_type && i.instrument_type !== query.instrument_type) return false;
        if (query.status && i.status !== query.status) return false;
        if (query.belong_month && i.issue_date.slice(0, 7) !== query.belong_month) return false;
        if (
          query.keyword &&
          !`${i.instrument_no}${i.drawer ?? ''}${i.acceptor ?? ''}${i.payee ?? ''}${i.remark ?? ''}`.includes(
            query.keyword,
          )
        ) {
          return false;
        }
        return true;
      });
    }
    case 'get_instrument_detail': {
      const instrument = mockFindInstrument(Number(args?.id ?? 0));
      return {
        instrument,
        endorsements: mockInstrumentEndorsements
          .filter((e) => e.instrument_id === instrument.id)
          .sort((a, b) => a.endorse_order - b.endorse_order),
      };
    }
    case 'register_instrument': {
      const data = args?.data as InstrumentRegisterInput | undefined;
      if (!data) throw new Error('票据登记入参缺失');
      const no = data.instrument_no?.trim();
      if (!no) throw new Error('票据号码必填');
      if (!(Number(data.face_amount) > 0)) throw new Error('票面金额必须大于 0');
      if (!['bank_acceptance', 'commercial_acceptance', 'check'].includes(data.instrument_type)) {
        throw new Error(`票据类型无效：${data.instrument_type}`);
      }
      if (!['received', 'issued'].includes(data.direction)) {
        throw new Error(`票据方向无效：${data.direction}`);
      }
      if (data.due_date < data.issue_date) throw new Error('到期日不能早于出票日');
      if (
        mockInstruments.some(
          (i) =>
            i.instrument_type === data.instrument_type &&
            i.instrument_no === no &&
            i.status !== 'void',
        )
      ) {
        throw new Error(`票据号 ${no} 已存在同类型有效票据（作废后同号可重新登记）`);
      }
      const isCheck = data.instrument_type === 'check';
      if (isCheck && !data.fund_account_id) {
        throw new Error(
          data.direction === 'received'
            ? '收到支票登记即到账，必须选择入账资金账户'
            : '开出支票登记即付款，必须选择出账资金账户',
        );
      }
      const now = new Date().toISOString();
      const inst = baseMockInstrument({
        id: mockNextInstrumentId(),
        instrument_type: data.instrument_type,
        direction: data.direction,
        instrument_no: no,
        face_amount: Number(data.face_amount),
        issue_date: data.issue_date,
        due_date: data.due_date,
        drawer: data.drawer ?? null,
        acceptor: data.acceptor ?? null,
        payee: data.payee ?? null,
        partner_id: data.partner_id ?? null,
        fund_account_id: data.fund_account_id ?? null,
        counter_account_code: data.counter_account_code ?? null,
        // 支票登记即终态；承兑按方向进 holding / issued_outstanding
        status: isCheck
          ? data.direction === 'received'
            ? 'collected'
            : 'paid'
          : data.direction === 'received'
            ? 'holding'
            : 'issued_outstanding',
        remark: data.remark ?? null,
        created_at: now,
        updated_at: now,
      });
      mockInstruments.unshift(inst);
      return inst;
    }
    case 'endorse_instrument': {
      const data = args?.data as InstrumentEndorseInput | undefined;
      const inst = mockFindInstrument(Number(data?.instrument_id ?? 0));
      if (inst.status !== 'holding') {
        throw new Error(`票据 ${inst.instrument_no} 当前状态「${INSTRUMENT_STATUS_LABEL[inst.status] ?? inst.status}」不允许背书`);
      }
      const endorsee = data?.endorsee?.trim();
      if (!endorsee) throw new Error('被背书人必填');
      if (Math.abs(Number(data?.amount ?? 0) - inst.face_amount) > 0.005) {
        throw new Error(
          `背书金额 ${data?.amount} 必须等于票面 ${inst.face_amount}（本期仅支持全额背书）`,
        );
      }
      const updated = mockTransitionInstrument(inst.id, ['holding'], 'endorsed_out', 'endorse');
      const endorsement: InstrumentEndorsement = {
        id: Date.now(),
        instrument_id: inst.id,
        endorse_order:
          mockInstrumentEndorsements.filter((e) => e.instrument_id === inst.id).length + 1,
        endorsee,
        endorse_date: data?.endorse_date ?? '',
        purpose: data?.purpose?.trim() || null,
        amount: Number(data?.amount ?? 0),
        // mock 不生成凭证，背书凭证 id 用 9xxx 占位段
        voucher_id: 9000 + mockInstrumentEndorsements.length + 1,
        created_by: '张会计',
        created_at: new Date().toISOString(),
      };
      mockInstrumentEndorsements.push(endorsement);
      return { instrument: updated, endorsement };
    }
    case 'discount_instrument': {
      const data = args?.data as InstrumentDiscountInput | undefined;
      const inst = mockFindInstrument(Number(data?.instrument_id ?? 0));
      const proceeds = Number(data?.proceeds ?? 0);
      if (proceeds <= 0) throw new Error('贴现实收金额必须大于 0');
      if (proceeds > inst.face_amount) {
        throw new Error(`贴现实收 ${proceeds.toFixed(2)} 不得大于票面 ${inst.face_amount.toFixed(2)}`);
      }
      const account = data?.fund_account_id ?? inst.fund_account_id;
      if (!account) throw new Error('贴现必须选择贴现入账资金账户');
      const updated = mockTransitionInstrument(inst.id, ['holding'], 'discounted', 'discount');
      updated.fund_account_id = account;
      return updated;
    }
    case 'start_collection': {
      const id = Number(args?.id ?? 0);
      mockFindInstrument(id);
      return mockTransitionInstrument(id, ['holding'], 'collecting', 'collect');
    }
    case 'confirm_collection': {
      const data = args?.data as InstrumentCollectConfirmInput | undefined;
      const inst = mockFindInstrument(Number(data?.instrument_id ?? 0));
      const account = data?.fund_account_id ?? inst.fund_account_id;
      if (!account) throw new Error('到账确认必须选择入账资金账户');
      const updated = mockTransitionInstrument(
        inst.id, ['collecting'], 'collected', 'confirm_collect',
      );
      updated.fund_account_id = account;
      return updated;
    }
    case 'settle_issued_instrument': {
      const data = args?.data as InstrumentSettleInput | undefined;
      const inst = mockFindInstrument(Number(data?.instrument_id ?? 0));
      const account = data?.fund_account_id ?? inst.fund_account_id;
      if (!account) throw new Error('兑付必须选择出账资金账户');
      const updated = mockTransitionInstrument(inst.id, ['issued_outstanding'], 'paid', 'settle');
      updated.fund_account_id = account;
      return updated;
    }
    case 'void_instrument': {
      const id = Number(args?.id ?? 0);
      const reason = String(args?.reason ?? '').trim();
      if (!reason) throw new Error('作废必须填写原因');
      return mockTransitionInstrument(id, ['holding', 'issued_outstanding'], 'void', 'void', reason, true);
    }
    case 'reverse_instrument_flow': {
      const data = args?.data as InstrumentReverseInput | undefined;
      const inst = mockFindInstrument(Number(data?.instrument_id ?? 0));
      if (!data?.reason?.trim()) throw new Error('冲正必须填写原因');
      const isCheck = inst.instrument_type === 'check';
      // 恢复状态与后端 reverse_instrument_flow 同口径：撤销最后一次流转；支票登记即终态的
      // 纠错直接作废（同号可重录）
      const restore: Record<string, string> = isCheck
        ? { collected: 'void', paid: 'void' }
        : { endorsed_out: 'holding', discounted: 'holding', collected: 'holding', paid: 'issued_outstanding' };
      const toStatus = restore[inst.status];
      if (!toStatus) {
        throw new Error(
          `票据 ${inst.instrument_no} 当前无可冲正的流转（仅已背书/已贴现/已到账/已兑付可冲正；未流转票据请使用作废）`,
        );
      }
      return mockTransitionInstrument(inst.id, [inst.status], toStatus, 'reverse', data.reason.trim(), true);
    }
    // ==================== 现金盘点单（第八阶段 Task 6，spec 6） ====================
    case 'get_count_sheets': {
      const query = (args?.query ?? {}) as CashCountQuery;
      return mockCashCountSheets
        .filter((s) => {
          if (query.status && s.status !== query.status) return false;
          if (query.belong_month && s.count_date.slice(0, 7) !== query.belong_month) return false;
          if (query.fund_account_id && s.fund_account_id !== query.fund_account_id) return false;
          return true;
        })
        .sort((a, b) => b.id - a.id)
        .map(mockSheetToPublic);
    }
    case 'get_count_sheet_detail': {
      const sheet = mockFindCashCountSheet(Number(args?.id ?? 0));
      const denominations: CashCountDenomination[] = [...sheet.denominations]
        .sort((a, b) => b.denomination - a.denomination)
        .map((d, i) => ({
          id: sheet.id * 100 + i,
          sheet_id: sheet.id,
          denomination: d.denomination,
          quantity: d.quantity,
          subtotal: Number((d.denomination * d.quantity).toFixed(2)),
        }));
      return { sheet: mockSheetToPublic(sheet), denominations };
    }
    case 'create_count_sheet': {
      const data = args?.data as CashCountCreateInput | undefined;
      if (!data) throw new Error('盘点单入参缺失');
      if (!data.count_date) throw new Error('盘点日期必填');
      mockAssertCashAccount(Number(data.fund_account_id));
      if (Number(data.counted_amount) < 0) throw new Error('实存金额不能为负（无现金请填 0）');
      mockValidateDenominations(Number(data.counted_amount), data.denominations);
      const account = mockFundAccounts.find((a) => a.id === Number(data.fund_account_id));
      const book = account?.opening_balance ?? 0;
      const difference = Number((Number(data.counted_amount) - book).toFixed(2));
      const reason = data.difference_reason?.trim();
      if (Math.abs(difference) > 0.005 && !reason) {
        throw new Error(`新建盘点单差异 ${difference.toFixed(2)} 不为 0，必须填写差异原因`);
      }
      const sheet = baseMockCashCountSheet({
        id: ++mockCashCountSeq,
        count_date: data.count_date,
        fund_account_id: Number(data.fund_account_id),
        book_balance: book,
        counted_amount: Number(data.counted_amount),
        status: 'draft',
      });
      sheet.difference = difference;
      sheet.difference_reason = reason || null;
      sheet.remark = data.remark?.trim() || null;
      sheet.denominations = (data.denominations ?? []).map((d) => ({ ...d }));
      mockCashCountSheets.unshift(sheet);
      return mockSheetToPublic(sheet);
    }
    case 'update_count_sheet': {
      const data = args?.data as CashCountUpdateInput | undefined;
      const sheet = mockFindCashCountSheet(Number(args?.id ?? 0));
      if (sheet.status !== 'draft') {
        throw new Error(`盘点单当前状态「${sheet.status}」，不允许修改（仅草稿可修改）`);
      }
      mockAssertCashAccount(Number(data?.fund_account_id ?? sheet.fund_account_id));
      const counted = Number(data?.counted_amount ?? sheet.counted_amount);
      if (counted < 0) throw new Error('实存金额不能为负（无现金请填 0）');
      mockValidateDenominations(counted, data?.denominations);
      const account = mockFundAccounts.find((a) => a.id === Number(data?.fund_account_id ?? sheet.fund_account_id));
      const book = account?.opening_balance ?? 0;
      const difference = Number((counted - book).toFixed(2));
      const reason = data?.difference_reason?.trim();
      if (Math.abs(difference) > 0.005 && !reason) {
        throw new Error(`修改盘点单差异 ${difference.toFixed(2)} 不为 0，必须填写差异原因`);
      }
      sheet.count_date = data?.count_date ?? sheet.count_date;
      sheet.belong_month = sheet.count_date.slice(0, 7);
      sheet.fund_account_id = Number(data?.fund_account_id ?? sheet.fund_account_id);
      sheet.book_balance = book;
      sheet.counted_amount = counted;
      sheet.difference = difference;
      sheet.difference_reason = reason || null;
      sheet.remark = data?.remark?.trim() || null;
      sheet.denominations = (data?.denominations ?? []).map((d) => ({ ...d }));
      sheet.updated_at = new Date().toISOString();
      return mockSheetToPublic(sheet);
    }
    case 'confirm_count_sheet': {
      const sheet = mockFindCashCountSheet(Number(args?.id ?? 0));
      if (sheet.status !== 'draft') {
        throw new Error(`盘点单当前状态「${sheet.status}」，不允许确认（仅草稿可确认）`);
      }
      if (sheet.denominations.length > 0) {
        const total = sheet.denominations.reduce(
          (s, d) => s + d.denomination * d.quantity,
          0,
        );
        if (Math.abs(total - sheet.counted_amount) > 0.005) {
          throw new Error(`面额明细合计 ${total.toFixed(2)} 与实存金额 ${sheet.counted_amount.toFixed(2)} 不一致，不能确认`);
        }
      }
      if (Math.abs(sheet.difference) > 0.005 && !sheet.difference_reason?.trim()) {
        throw new Error(`确认盘点单差异 ${sheet.difference.toFixed(2)} 不为 0，必须填写差异原因`);
      }
      sheet.status = 'confirmed';
      // mock 不落凭证：差异≠0 用 9xxx 段占位，差异 0 保持 null
      sheet.voucher_id = Math.abs(sheet.difference) > 0.005 ? 9100 + sheet.id : null;
      sheet.updated_at = new Date().toISOString();
      return mockSheetToPublic(sheet);
    }
    case 'void_count_sheet': {
      const sheet = mockFindCashCountSheet(Number(args?.id ?? 0));
      if (sheet.status === 'void') throw new Error('盘点单已作废，不能重复作废');
      const reason = String(args?.reason ?? '').trim();
      if (sheet.status === 'confirmed' && sheet.voucher_id) {
        if (!reason) throw new Error('已确认盘点单存在差异凭证，作废走红字冲正必须填写原因');
        sheet.reversed = true;
        sheet.voucher_id = null;
      }
      sheet.status = 'void';
      sheet.updated_at = new Date().toISOString();
      return mockSheetToPublic(sheet);
    }
    // ==================== Task 14：员工借款备用金与核销（spec 4.11） ====================
    case 'get_advance_ledger': {
      const q = (args?.query ?? {}) as AdvanceLedgerQuery;
      const asOf = q.as_of_date ? new Date(`${q.as_of_date}T00:00:00Z`) : new Date();
      const loans = mockFundDocuments.filter(
        (d) => d.document_type === 'advance' && d.status !== 'void' && d.status !== 'reversed',
      );
      let rows: AdvanceLedgerRow[] = loans.map((loan) => {
        const settled = mockAdvanceLinks
          .filter((l) => l.advance_id === loan.id && l.status === 'active')
          .reduce((s, l) => s + l.allocated_amount, 0);
        const rawOutstanding = loan.amount - settled;
        const outstanding = Math.abs(rawOutstanding) < 0.005 ? 0 : rawOutstanding;
        const docDate = new Date(`${loan.document_date}T00:00:00Z`);
        const daysOutstanding = Math.max(0, Math.round((asOf.getTime() - docDate.getTime()) / 86400000));
        const overdueDays =
          outstanding > 0 && loan.due_date
            ? Math.max(
                0,
                Math.round((asOf.getTime() - new Date(`${loan.due_date}T00:00:00Z`).getTime()) / 86400000),
              )
            : 0;
        const bucket =
          outstanding <= 0
            ? '已结清'
            : daysOutstanding <= 30
              ? '0-30天'
              : daysOutstanding <= 60
                ? '31-60天'
                : daysOutstanding <= 90
                  ? '61-90天'
                  : '90天以上';
        return {
          advance_id: loan.id,
          document_no: loan.document_no,
          belong_month: loan.belong_month,
          document_date: loan.document_date,
          due_date: loan.due_date,
          employee_id: loan.employee_id,
          employee_name: loan.employee_id === 1 ? '张三' : loan.employee_id === 2 ? '李四' : null,
          department: loan.department,
          summary: loan.summary,
          amount: loan.amount,
          settled_amount: settled,
          outstanding_amount: outstanding,
          days_outstanding: daysOutstanding,
          overdue_days: overdueDays,
          aging_bucket: bucket,
          advance_status: loan.status,
        };
      });
      if (q.employee_id) rows = rows.filter((r) => r.employee_id === q.employee_id);
      if (q.keyword?.trim()) {
        const kw = q.keyword.trim();
        rows = rows.filter(
          (r) =>
            r.document_no.includes(kw) ||
            r.summary.includes(kw) ||
            (r.employee_name ?? '').includes(kw),
        );
      }
      if (q.only_outstanding) rows = rows.filter((r) => r.outstanding_amount > 0.005);
      return {
        rows,
        total_amount: rows.reduce((s, r) => s + r.amount, 0),
        total_settled: rows.reduce((s, r) => s + r.settled_amount, 0),
        total_outstanding: rows.reduce((s, r) => s + r.outstanding_amount, 0),
      };
    }
    case 'get_advance_settlement_links':
      return mockAdvanceLinks.filter((l) => l.advance_id === Number(args?.advanceId ?? 0));
    case 'cancel_advance_settlement_link': {
      const data = args?.data as AdvanceLinkCancelInput | undefined;
      const link = mockAdvanceLinks.find((l) => l.id === Number(data?.link_id ?? 0));
      if (!link) throw new Error('核销记录不存在');
      if (link.status !== 'active') throw new Error('该核销记录已取消，无需重复取消');
      if (!data?.reason?.trim()) throw new Error('取消核销必须填写原因');
      const settlement = mockFundDocuments.find((d) => d.id === link.settlement_id);
      if (!settlement) throw new Error('核销单不存在');
      if (settlement.status === 'settled') {
        throw new Error('预览模式不支持冲正取消核销，请在桌面应用中操作');
      }
      const now = new Date().toISOString();
      mockTransitionFundDocument(
        settlement.id,
        ['draft', 'submitted', 'approved', 'rejected'],
        'void',
        'void',
        data.reason,
        true,
      );
      link.status = 'cancelled';
      link.cancelled_at = now;
      link.cancel_reason = data.reason.trim();
      link.settlement_status = 'void';
      return { ...settlement, status: 'void' };
    }
    case 'export_advance_ledger':
      return String(args?.path ?? '');
    // 预览模式跳过启动密码/锁屏（仅在非 Tauri 环境生效），让业务页面可打开。
    case 'is_security_initialized':
      return true;
    case 'unlock':
      return { unlocked: true, failed_attempts: 0, lock_until: null };
    case 'get_security_status':
      return {
        initialized: true,
        locked: false,
        failed_attempts: 0,
        lock_until: null,
        idle_lock_enabled: false,
        idle_timeout_seconds: 300,
        sensitive_reveal_seconds: 300,
        migration_status: null,
      };
    case 'get_dashboard_summary':
      return {
        employee_count: 0,
        active_employee_count: 0,
        calculated_count: 0,
        locked_count: 0,
        total_gross_salary: 0,
        total_net_salary: 0,
        total_social_security: 0,
        total_housing_fund: 0,
        total_tax: 0,
        attendance_count: 0,
      };
    case 'get_month_close_workbench':
      return { summary: emptyMonthCloseSummary(String(args?.month ?? '')), checks: [], month_close: undefined };
    case 'get_month_close_status':
      return undefined;
    case 'close_month':
      return {
        id: Date.now(),
        month: String((args?.data as { month?: string } | undefined)?.month ?? ''),
        status: 'closed',
        closed_at: new Date().toISOString(),
        closed_by: 'system',
      };
    case 'reopen_month':
      return {
        id: Date.now(),
        month: String((args?.data as { month?: string } | undefined)?.month ?? ''),
        status: 'reopened',
        reopened_at: new Date().toISOString(),
        reopen_reason: String((args?.data as { reason?: string } | undefined)?.reason ?? ''),
      };
    case 'export_month_close_package':
      return { success: true, output_dir: String(args?.dir ?? ''), files: [] };
    case 'get_vouchers':
      return [];
    case 'preview_bank_transaction_import':
      return {
        fund_account_id: Number((args?.fundAccountId as number | undefined) ?? 0),
        fund_account_name: '演示账户',
        file_path: String(args?.path ?? ''),
        headers: ['交易日期', '摘要', '对方户名', '收入', '支出', '余额'],
        total_rows: 0,
        ok_rows: 0,
        duplicate_rows: 0,
        warning_rows: 0,
        error_rows: 0,
        income_total: 0,
        expense_total: 0,
        rows: [],
      };
    case 'import_bank_transactions_file':
      return { success: true, total: 0, imported: 0, skipped: 0, errors: [] };
    case 'query_bank_transactions':
      return [];
    case 'auto_match_bank_transactions':
      return { success: true, matched: 0, skipped: 0, errors: [] };
    case 'confirm_bank_transaction_match':
      return {
        id: Date.now(),
        transaction_id: Number((args?.data as { transaction_id?: number } | undefined)?.transaction_id ?? 0),
        payment_batch_id: Number((args?.data as { payment_batch_id?: number } | undefined)?.payment_batch_id ?? 0),
        match_score: 100,
      };
    case 'cancel_bank_transaction_match':
    case 'ignore_bank_transaction':
      return true;
    case 'preview_bank_allocation_candidates':
      return {
        transaction_id: Number(args?.transactionId ?? 0),
        transaction_date: '',
        belong_month: '',
        income_amount: 0,
        expense_amount: 0,
        remaining_amount: 0,
        fund_account_id: 0,
        candidates: [],
      };
    case 'preview_bank_auto_matches':
      return [];
    case 'confirm_bank_allocations':
      return { confirmed: 0, skipped: 0, errors: [], allocation_ids: [] };
    case 'cancel_bank_allocation':
      return true;
    case 'list_bank_allocations':
      return [];
    case 'batch_confirm_bank_auto_matches':
      return { confirmed: 0, skipped: 0, errors: [], allocation_ids: [] };
    case 'migrate_legacy_bank_matches':
      return {
        total: 0,
        active_total: 0,
        migrated: 0,
        already_migrated: 0,
        unconverted: [],
      };
    case 'get_fund_journal': {
      const jq = (args?.query ?? {}) as FundJournalQuery;
      return {
        fund_account_id: Number(jq.fund_account_id ?? 0),
        fund_account_name: '预览账户',
        account_type: 'bank',
        from_month: jq.from_month ?? null,
        to_month: jq.to_month ?? null,
        opening_balance: 0,
        closing_balance: 0,
        total_income: 0,
        total_expense: 0,
        rows: [],
      };
    }
    // ==================== 资金日报（第八阶段 Task 7，spec 7） ====================
    case 'get_fund_daily_report': {
      const date = String(args?.date ?? '').slice(0, 10);
      const accounts = mockFundAccounts.filter((a) => a.is_active);
      const reportAccounts = accounts.map((a, i) => {
        // 演示口径：首账户当日有一笔收 1200，其余账户无业务（opening=closing）
        const income = i === 0 ? 1200 : 0;
        return {
          account_id: a.id,
          account_name: a.name,
          account_type: a.account_type,
          opening: a.opening_balance,
          income,
          expense: 0,
          closing: a.opening_balance + income,
        };
      });
      const dayMovements = reportAccounts.filter((a) => a.income > 0 || a.expense > 0);
      const entries = dayMovements.map((a, i) => ({
        account_id: a.account_id,
        account_name: a.account_name,
        voucher_id: 9000 + i,
        voucher_no: `JZ-${date.replace(/-/g, '')}-00${i + 1}`,
        voucher_date: date,
        summary: '演示收款',
        income_amount: a.income,
        expense_amount: a.expense,
        balance: a.closing,
      }));
      const trend = Array.from({ length: 7 }, (_, k) => {
        const d = new Date(date);
        d.setDate(d.getDate() - (6 - k));
        const pointDate = d.toISOString().slice(0, 10);
        const isToday = pointDate === date;
        return {
          date: pointDate,
          balances: reportAccounts.map((a) => ({
            account_id: a.account_id,
            closing: isToday ? a.closing : a.opening,
          })),
        };
      });
      return { date, accounts: reportAccounts, entries, trend };
    }
    case 'export_fund_daily_report':
      return String(args?.path ?? '');
    // ==================== 进项台账（第八阶段 Task 10，spec 9） ====================
    case 'get_input_tax_ledger': {
      const fromMonth = String(args?.fromMonth ?? '');
      const toMonth = String(args?.toMonth ?? '');
      return {
        from_month: fromMonth,
        to_month: toMonth,
        rows: [],
        monthly_subtotals: [],
        invoice_count: 0,
        sum_amount: 0,
        sum_tax_amount: 0,
        sum_total_amount: 0,
      };
    }
    case 'export_input_tax_ledger':
      return String(args?.path ?? '');
    case 'generate_bank_reconciliation_period':
      return {
        id: 0,
        fund_account_id: Number(args?.fundAccountId ?? 0),
        fund_account_name: '预览账户',
        belong_month: String(args?.month ?? ''),
        statement_opening_balance: 0,
        statement_closing_balance: 0,
        statement_source: 'empty',
        book_closing_balance: 0,
        outstanding_tx_amount: 0,
        outstanding_line_amount: 0,
        adjusted_book_balance: 0,
        adjusted_bank_balance: 0,
        difference: 0,
        status: 'draft',
        detail_json: null,
        confirmed_by: null,
        confirmed_at: null,
        created_at: '',
        updated_at: '',
      };
    case 'confirm_bank_reconciliation_period':
      throw new Error('预览模式不支持该操作，请在桌面应用中操作');
    case 'query_budgets':
      return [];
    case 'save_budget':
      return {
        id: Number((args?.data as { id?: number } | undefined)?.id ?? Date.now()),
        month: String((args?.data as { month?: string } | undefined)?.month ?? ''),
        budget_amount: Number((args?.data as { budget_amount?: number } | undefined)?.budget_amount ?? 0),
      };
    case 'delete_budget':
      return true;
    case 'get_financial_analysis': {
      const query = args?.query as FinancialAnalysisQuery | undefined;
      return {
        month: query?.month ?? '',
        months: query?.months ?? 1,
        department_costs: [],
        expense_trends: [],
        employee_costs: [],
        budget_executions: [],
        monthly_comparison: [],
      };
    }
    case 'get_ocr_settings':
      return { ocr_mode: 'online', ocr_provider: 'baidu', baidu_api_key: '', baidu_secret_key: '' };
    case 'get_data_safety_status':
      return {
        app_data_dir: '',
        database_path: '',
        database_exists: true,
        database_size: 0,
        invoice_dir: '',
        invoice_dir_exists: true,
        invoice_dir_size: 0,
        table_counts: [],
        attachment_count: 0,
        attachment_encrypted_count: 0,
        attachment_orphan_count: 0,
        attachment_missing_count: 0,
        stage7_migration_status: 'done',
        stage7_pending_count: 0,
      };
    case 'backup_database':
      return {
        success: true,
        backup_dir: String(args?.targetDir ?? ''),
        database_path: '',
        invoice_dir: '',
        manifest_path: '',
        database_size: 0,
        invoice_dir_size: 0,
        created_at: new Date().toISOString(),
      };
    case 'restore_database':
      return {
        success: true,
        restored_at: new Date().toISOString(),
        restored_from: String(args?.backupDir ?? ''),
        safety_backup_dir: '',
        restart_recommended: true,
      };
    case 'verify_database':
      return { ok: true, checked_at: new Date().toISOString(), integrity_check: 'ok', messages: ['前端预览模式'] };
    case 'ocr_recognize':
    case 'ocr_recognize_punch_card':
      return { batch_id: 0, records: [], raw_text: '' };
    case 'create_employee':
      return { id: Date.now(), ...(args?.data as object), created_at: '', updated_at: '' };
    case 'save_invoice_expense_type':
      return { id: Date.now(), code: '', name: '', sort_order: 0, enabled: 1, ...(args?.data as object) };
    case 'save_invoice':
      return { id: Date.now(), amount: 0, tax_amount: 0, total_amount: 0, ...(args?.data as object) };
    case 'save_reimbursement_claim':
      return { id: Date.now(), total_amount: 0, invoice_count: 0, ...(args?.data as object) };
    case 'ocr_invoice':
      return {};
    case 'get_gl_accounts':
    case 'get_account_mappings':
      return [];
    case 'get_opening_balances':
      return { month: '', rows: [] };
    case 'set_gl_account_active':
    case 'save_opening_balances':
    case 'delete_account_mapping':
      return true;
    case 'create_gl_account':
    case 'save_account_mapping':
      throw new Error('预览模式不支持该操作，请在桌面应用中操作');
    case 'get_balance_sheet':
      return {
        month: String(args?.month ?? ''),
        enabled: false,
        asset_rows: [],
        liability_equity_rows: [],
        asset_total: 0,
        liability_equity_total: 0,
        balanced: true,
      };
    case 'get_income_statement':
      return {
        month: String(args?.month ?? ''),
        rows: [],
        net_profit_month: 0,
        net_profit_year: 0,
      };
    case 'get_cash_flow_statement':
      return {
        month: String(args?.month ?? ''),
        rows: [],
        net_increase: 0,
        unclassified: [],
      };
    case 'export_financial_report':
      return '';
    case 'get_trial_balance':
      return {
        from_month: String(args?.fromMonth ?? ''),
        to_month: String(args?.toMonth ?? ''),
        enabled: true,
        rows: [],
        balanced: true,
      };
    case 'export_trial_balance':
      return '';
    case 'get_annual_tax_summary':
      return [];
    case 'export_annual_tax_summary':
      return '';
    case 'export_tax_withholding_declaration':
      return String(args?.path ?? '');
    case 'create_bank_manual_voucher':
      throw new Error('预览模式不支持生成凭证，请在桌面应用中操作');
    case 'unlock_salary_results':
      throw new Error('预览模式不支持该操作，请在桌面应用中操作');
    case 'get_social_profiles':
      return [];
    case 'get_social_base_limits':
      return [0, 0, 0, 0];
    case 'save_social_profile':
    case 'copy_social_profiles':
    case 'set_social_base_limits':
      throw new Error('预览模式不支持该操作，请在桌面应用中操作');
    // ==================== 账期提醒（第八阶段 Task 4） ====================
    // 三类别各一条：借款临期 / 票据逾期 / 报销滞留；日期相对今天生成，预览始终有内容。
    case 'get_dashboard_reminders': {
      const dayStr = (offset: number) =>
        new Date(Date.now() + offset * 86400000).toISOString().slice(0, 10);
      return [
        {
          category: 'advance_due',
          title: '张三 借款 JK2026090005',
          due_date: dayStr(3),
          days_left: 3,
          amount: 2000,
          ref_id: 1,
        },
        {
          category: 'instrument_due',
          title: '银行承兑汇票 YZ2026001',
          due_date: dayStr(-2),
          days_left: -2,
          amount: 100000,
          ref_id: 2,
        },
        {
          category: 'payable_stuck',
          title: '报销单 BX202608002 技术报销',
          due_date: dayStr(-12),
          days_left: 12,
          amount: 500,
          ref_id: 3,
        },
      ];
    }
    case 'get_reminder_advance_days':
      return mockReminderAdvanceDays;
    case 'set_reminder_advance_days':
      mockReminderAdvanceDays = Number(args?.days ?? 7);
      return true;
    default: {
      // 预览模式兜底（Minor 8）：不再无差别 return true——只读命令按语义返回空集合，
      // 其余（写操作/状态机命令）抛中文错误，避免浏览器预览里
      // “提示成功、数据未变”的假反馈（如冲正/取消核销/审批类命令）。
      if (command.startsWith('get_') || command.startsWith('query_') || command.startsWith('list_')) {
        return [];
      }
      if (command.startsWith('export_')) {
        return '';
      }
      if (command.startsWith('is_') || command.startsWith('has_')) {
        return false;
      }
      throw new Error(`预览模式暂不支持「${command}」，请在桌面应用中操作`);
    }
  }
};

const invoke = async <T>(command: string, args?: Record<string, unknown>): Promise<T> => {
  if (isTauriRuntime()) {
    return tauriInvoke<T>(command, args);
  }
  return mockTauriResponse(command, args) as T;
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
  const socialInsuranceEmployer = numberOrZero(result.social_security_employer);
  const housingFund = numberOrZero(result.housing_fund_personal);
  const housingFundEmployer = numberOrZero(result.housing_fund_employer);
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
    social_insurance_employer: socialInsuranceEmployer,
    housing_fund: housingFund,
    housing_fund_employer: housingFundEmployer,
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
    fund_pending_approval_count: numberOrZero(raw.fund_pending_approval_count),
    fund_unpaid_count: numberOrZero(raw.fund_unpaid_count),
    unassigned_bank_tx_count: numberOrZero(raw.unassigned_bank_tx_count),
    unreconciled_tx_count: numberOrZero(raw.unreconciled_tx_count),
    advance_overdue_count: numberOrZero(raw.advance_overdue_count),
    advance_overdue_amount: numberOrZero(raw.advance_overdue_amount),
    fund_total_balance: numberOrZero(raw.fund_total_balance),
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

// ==================== 账期提醒（第八阶段 Task 4，spec 5） ====================

export async function getDashboardReminders(): Promise<ReminderItem[]> {
  return invoke<ReminderItem[]>('get_dashboard_reminders');
}

export async function getReminderAdvanceDays(): Promise<number> {
  const days = numberOrZero(await invoke<number>('get_reminder_advance_days'));
  return days > 0 ? days : 7;
}

export async function setReminderAdvanceDays(days: number): Promise<void> {
  await invoke<void>('set_reminder_advance_days', { days });
}

export async function getMonthCloseStatus(month: string): Promise<MonthCloseRecord | undefined> {
  return invoke<MonthCloseRecord | undefined>('get_month_close_status', { month });
}

export async function closeMonth(month: string, remark?: string): Promise<MonthCloseRecord> {
  return invoke<MonthCloseRecord>('close_month', { data: { month, remark } });
}

export async function reopenMonth(month: string, reason: string): Promise<MonthCloseRecord> {
  return invoke<MonthCloseRecord>('reopen_month', { data: { month, reason } });
}

export async function exportMonthClosePackage(month: string, dir: string): Promise<MonthClosePackageResult> {
  return invoke<MonthClosePackageResult>('export_month_close_package', { month, dir });
}

export async function queryPaymentBatches(query: PaymentBatchQuery): Promise<PaymentBatch[]> {
  return invoke<PaymentBatch[]>('query_payment_batches', { query });
}

export async function getPaymentBatchDetail(id: number): Promise<PaymentBatchDetail> {
  return invoke<PaymentBatchDetail>('get_payment_batch_detail', { id });
}

export async function createPaymentBatch(data: PaymentBatchInput): Promise<PaymentBatchDetail> {
  return invoke<PaymentBatchDetail>('create_payment_batch', { data });
}

export async function exportPaymentBatchFile(id: number, savePath: string): Promise<PaymentBatch> {
  return invoke<PaymentBatch>('export_payment_batch_file', { id, path: savePath });
}

export async function markPaymentBatchPaid(data: PaymentBatchPaidInput): Promise<PaymentBatch> {
  return invoke<PaymentBatch>('mark_payment_batch_paid', { data });
}

export async function voidPaymentBatch(data: PaymentBatchVoidInput): Promise<PaymentBatch> {
  return invoke<PaymentBatch>('void_payment_batch', { data });
}

export async function updatePaymentBatchRemark(data: PaymentBatchRemarkInput): Promise<PaymentBatch> {
  return invoke<PaymentBatch>('update_payment_batch_remark', { data });
}

export async function previewBankTransactionImport(
  filePath: string,
  fundAccountId: number,
): Promise<BankImportPreview> {
  return invoke<BankImportPreview>('preview_bank_transaction_import', {
    path: filePath,
    fundAccountId,
  });
}

export async function importBankTransactionsFile(
  filePath: string,
  fundAccountId: number,
): Promise<ImportResult> {
  const result = await invoke<ImportResult & { skipped?: number }>('import_bank_transactions_file', {
    path: filePath,
    fundAccountId,
  });
  return normalizeImportResult(result);
}

export async function queryBankTransactions(query: BankTransactionQuery): Promise<BankTransaction[]> {
  return invoke<BankTransaction[]>('query_bank_transactions', { query });
}

export async function autoMatchBankTransactions(month: string): Promise<BankAutoMatchResult> {
  return invoke<BankAutoMatchResult>('auto_match_bank_transactions', { month });
}

export async function confirmBankTransactionMatch(data: BankTransactionMatchInput): Promise<BankTransactionMatch> {
  return invoke<BankTransactionMatch>('confirm_bank_transaction_match', { data });
}

export async function cancelBankTransactionMatch(transactionId: number): Promise<boolean> {
  return invoke<boolean>('cancel_bank_transaction_match', { transaction_id: transactionId });
}

// ==================== 银行流水多对多核销（Task 12，spec 4.9/6.2/6.3） ====================

/** 单条流水的候选资金分录预览（人工核销用） */
export async function previewBankAllocationCandidates(
  transactionId: number,
): Promise<BankAutoMatchPreviewItem> {
  return invoke<BankAutoMatchPreviewItem>('preview_bank_allocation_candidates', {
    transaction_id: transactionId,
  });
}

/** 自动匹配预览：只返回候选与 score，不写库（spec 6.2） */
export async function previewBankAutoMatches(
  month: string,
): Promise<BankAutoMatchPreviewItem[]> {
  return invoke<BankAutoMatchPreviewItem[]>('preview_bank_auto_matches', { month });
}

/** 批量确认核销（manual/auto）；单项全败时后端抛错 */
export async function confirmBankAllocations(
  data: BankAllocationInput[],
  matchMethod: 'manual' | 'auto' = 'manual',
): Promise<BankAllocationBatchResult> {
  return invoke<BankAllocationBatchResult>('confirm_bank_allocations', {
    data,
    matchMethod,
  });
}

/** 取消核销（状态标记 cancelled，释放两侧余额） */
export async function cancelBankAllocation(allocationId: number): Promise<boolean> {
  return invoke<boolean>('cancel_bank_allocation', { allocation_id: allocationId });
}

/** 核销明细查询（对账页展示与追溯） */
export async function listBankAllocations(
  query: BankAllocationQuery = {},
): Promise<BankReconciliationAllocation[]> {
  return invoke<BankReconciliationAllocation[]>('list_bank_allocations', { query });
}

/** 批量确认自动匹配：只处理高置信且金额相等的候选 */
export async function batchConfirmBankAutoMatches(
  month: string,
  minScore?: number,
): Promise<BankAllocationBatchResult> {
  return invoke<BankAllocationBatchResult>('batch_confirm_bank_auto_matches', {
    month,
    minScore,
  });
}

/** 旧 bank_transaction_matches 迁移为 allocation（spec 4.9/9.4，幂等可重跑） */
export async function migrateLegacyBankMatches(): Promise<LegacyBankMatchReport> {
  return invoke<LegacyBankMatchReport>('migrate_legacy_bank_matches');
}

// ==================== Task 13：资金日记账与银行余额调节表（spec 6.1/4.10） ====================

export async function getFundJournal(query: FundJournalQuery): Promise<FundJournal> {
  return invoke<FundJournal>('get_fund_journal', { query });
}

export async function exportFundJournal(query: FundJournalQuery, path: string): Promise<string> {
  return invoke<string>('export_fund_journal', { query, path });
}

// ==================== 资金日报（第八阶段 Task 7，spec 7） ====================

/** 资金日报（只读）：账户汇总勾稽 + 当日明细 + 近 7 日趋势 */
export async function getFundDailyReport(date: string): Promise<FundDailyReport> {
  return invoke<FundDailyReport>('get_fund_daily_report', { date });
}

/** 导出资金日报 Excel（两 sheet：账户汇总 + 当日明细；敏感导出） */
export async function exportFundDailyReport(date: string, path: string): Promise<string> {
  return invoke<string>('export_fund_daily_report', { date, path });
}

// ==================== 增值税进项台账（第八阶段 Task 10，spec 9） ====================

/** 进项台账（只读）：发票维度明细 + 月度小计 + 区间合计（排除 void；登记 ≠ 认证） */
export async function getInputTaxLedger(
  fromMonth: string,
  toMonth: string,
): Promise<InputTaxLedgerReport> {
  return invoke<InputTaxLedgerReport>('get_input_tax_ledger', { fromMonth, toMonth });
}

/** 导出进项台账 Excel（明细 + 月度小计 + 区间合计；敏感导出） */
export async function exportInputTaxLedger(
  fromMonth: string,
  toMonth: string,
  path: string,
): Promise<string> {
  return invoke<string>('export_input_tax_ledger', { fromMonth, toMonth, path });
}

export async function generateBankReconciliationPeriod(
  fundAccountId: number,
  month: string,
  statementOpening?: number,
  statementClosing?: number,
): Promise<BankReconciliationPeriod> {
  return invoke<BankReconciliationPeriod>('generate_bank_reconciliation_period', {
    fundAccountId,
    month,
    statementOpening: statementOpening ?? null,
    statementClosing: statementClosing ?? null,
  });
}

export async function confirmBankReconciliationPeriod(
  id: number,
): Promise<BankReconciliationPeriod> {
  return invoke<BankReconciliationPeriod>('confirm_bank_reconciliation_period', { id });
}

export async function listBankReconciliationPeriods(
  fundAccountId?: number,
  month?: string,
): Promise<BankReconciliationPeriod[]> {
  return invoke<BankReconciliationPeriod[]>('list_bank_reconciliation_periods', {
    fundAccountId: fundAccountId ?? null,
    month: month ?? null,
  });
}

export async function exportBankReconciliationPeriod(id: number, path: string): Promise<string> {
  return invoke<string>('export_bank_reconciliation_period', { id, path });
}

export async function ignoreBankTransaction(data: BankTransactionIgnoreInput): Promise<boolean> {
  return invoke<boolean>('ignore_bank_transaction', { data });
}

export async function createBankManualVoucher(
  transactionId: number,
  accountCode: string,
  fundAccountId: number,
  summary?: string,
): Promise<Voucher> {
  return invoke<Voucher>('create_bank_manual_voucher', {
    transactionId,
    accountCode,
    fundAccountId,
    summary,
  });
}

export async function queryBudgets(query: BudgetQuery): Promise<Budget[]> {
  return invoke<Budget[]>('query_budgets', { query });
}

export async function saveBudget(data: BudgetInput): Promise<Budget> {
  return invoke<Budget>('save_budget', { data });
}

export async function deleteBudget(id: number): Promise<boolean> {
  return invoke<boolean>('delete_budget', { id });
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

export async function getDataSafetyStatus(): Promise<DataSafetyStatus> {
  return invoke<DataSafetyStatus>('get_data_safety_status');
}

export async function backupDatabase(
  targetDir: string,
  encrypt = false,
): Promise<DataBackupResult> {
  return invoke<DataBackupResult>('backup_database', { targetDir, encrypt });
}

export async function restoreDatabase(backupDir: string): Promise<DataRestoreResult> {
  return invoke<DataRestoreResult>('restore_database', { backupDir });
}

export async function verifyDatabase(): Promise<DataSafetyCheckResult> {
  return invoke<DataSafetyCheckResult>('verify_database');
}

export async function compactDatabase(): Promise<boolean> {
  return invoke<boolean>('compact_database');
}

export async function openAppDataDir(): Promise<boolean> {
  return invoke<boolean>('open_app_data_dir');
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

/** 全局三险个人分摊份额（占社保个人总额 %；null = 未配置，申报表导出合并展示，Minor 13） */
export interface SocialShareRates {
  pension: number | null;
  medical: number | null;
  unemployment: number | null;
}

export async function getSocialShareRates(): Promise<SocialShareRates> {
  const rules = await invoke<BackendSalaryRule[]>('get_salary_rules');
  const byKey = new Map(
    rules.filter((rule) => rule.enabled !== 0).map((rule) => [rule.rule_key, rule]),
  );
  // 份额 0 视为未配置（导出退合并展示），与后端 valid_rates 口径一致
  const pct = (key: string): number | null => {
    const raw = byKey.get(key)?.rule_value ?? 0;
    return raw > 0 ? raw * 100 : null;
  };
  return {
    pension: pct('pension_personal_rate'),
    medical: pct('medical_personal_rate'),
    unemployment: pct('unemployment_personal_rate'),
  };
}

export async function saveSocialShareRates(rates: SocialShareRates): Promise<void> {
  const entries: Array<[string, string, number]> = [
    ['pension_personal_rate', '养老保险个人分摊份额', (rates.pension ?? 0) / 100],
    ['medical_personal_rate', '医疗保险个人分摊份额', (rates.medical ?? 0) / 100],
    ['unemployment_personal_rate', '失业保险个人分摊份额', (rates.unemployment ?? 0) / 100],
  ];
  await Promise.all(
    entries.map(([key, name, value]) =>
      invoke('upsert_salary_rule_key', { key, ruleName: name, ruleValue: value }),
    ),
  );
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

export const unlockSalaryResults = (password: string, month: string, reason: string) =>
  invoke<boolean>('unlock_salary_results', { password, month, reason });

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

// Task 15 治理（spec 5.2）：审批状态只能经状态机命令流转（署名 + 审批轨迹）；
// approve/reject/unapprove 意见必填；付款状态只能由付款批次事务更新，
// 不再提供 update_reimbursement_claim_status 直写通道。

export async function submitReimbursementClaim(
  id: number,
  comment?: string,
): Promise<ReimbursementClaim> {
  return invoke<ReimbursementClaim>('submit_reimbursement_claim', { id, comment });
}

export async function approveReimbursementClaim(
  id: number,
  comment: string,
): Promise<ReimbursementClaim> {
  return invoke<ReimbursementClaim>('approve_reimbursement_claim', { id, comment });
}

export async function rejectReimbursementClaim(
  id: number,
  comment: string,
): Promise<ReimbursementClaim> {
  return invoke<ReimbursementClaim>('reject_reimbursement_claim', { id, comment });
}

export async function withdrawReimbursementClaim(
  id: number,
  comment?: string,
): Promise<ReimbursementClaim> {
  return invoke<ReimbursementClaim>('withdraw_reimbursement_claim', { id, comment });
}

export async function unapproveReimbursementClaim(
  id: number,
  comment: string,
): Promise<ReimbursementClaim> {
  return invoke<ReimbursementClaim>('unapprove_reimbursement_claim', { id, comment });
}

export async function deleteReimbursementClaim(id: number, comment: string): Promise<ReimbursementClaim> {
  return invoke<ReimbursementClaim>('delete_reimbursement_claim', { id, comment });
}

// ==================== 安全模块 ====================
// 命名约定：invoke 参数 key 用 camelCase（Tauri 2 默认会自动转 snake_case），
// 与本文件中 imagePath / batchId / paymentStatus 等既有调用一致。
// changePassword 因为 `new` 是 JS 保留字,参数名加 P 后缀做别名。

export async function isSecurityInitialized(): Promise<boolean> {
  return invoke<boolean>('is_security_initialized');
}

export async function setupSecurity(
  password: string,
  recoveryCode: string,
  securityQuestion: string,
  answer: string,
): Promise<void> {
  await invoke<void>('setup_security', {
    password,
    recoveryCode,
    securityQuestion,
    answer,
  });
}

export async function unlock(password: string): Promise<UnlockResult> {
  return invoke<UnlockResult>('unlock', { password });
}

export async function lockApp(): Promise<void> {
  await invoke<void>('lock');
}

export async function getSecurityStatus(): Promise<SecurityStatus> {
  return invoke<SecurityStatus>('get_security_status');
}

export async function changePassword(oldPwd: string, newPwd: string): Promise<void> {
  await invoke<void>('change_password', { old: oldPwd, new: newPwd });
}

export async function resetPasswordByRecovery(code: string, newPassword: string): Promise<void> {
  await invoke<void>('reset_password_by_recovery', { code, newPassword });
}

export async function resetPasswordByQuestion(answer: string, newPassword: string): Promise<void> {
  await invoke<void>('reset_password_by_question', { answer, newPassword });
}

export async function updateIdleSettings(enabled: boolean, seconds: number): Promise<void> {
  await invoke<void>('update_idle_settings', { enabled, seconds });
}

export async function updateSensitiveRevealSettings(seconds: number): Promise<void> {
  await invoke<void>('update_sensitive_reveal_settings', { seconds });
}

export async function revealSensitiveData(password: string): Promise<RevealResult> {
  return invoke<RevealResult>('reveal_sensitive_data', { password });
}

export async function getDecryptedInvoiceUrl(invoiceId: number): Promise<string> {
  return invoke<string>('get_decrypted_invoice_url', { invoiceId });
}

export async function getLegacyMigrationStatus(): Promise<LegacyMigrationStatus> {
  return invoke<LegacyMigrationStatus>('get_legacy_migration_status');
}

export async function migrateLegacyResources(): Promise<void> {
  await invoke<void>('migrate_legacy_resources');
}

// ==================== 总账科目 ====================

export async function getGlAccounts(): Promise<GlAccount[]> {
  return invoke<GlAccount[]>('get_gl_accounts');
}

export async function createGlAccount(data: GlAccountInput): Promise<GlAccount> {
  return invoke<GlAccount>('create_gl_account', { data });
}

export async function setGlAccountActive(code: string, active: boolean): Promise<boolean> {
  return invoke<boolean>('set_gl_account_active', { code, active });
}

export async function getOpeningBalances(): Promise<OpeningBalanceState> {
  return invoke<OpeningBalanceState>('get_opening_balances');
}

export async function saveOpeningBalances(month: string, rows: OpeningBalanceRow[]): Promise<boolean> {
  return invoke<boolean>('save_opening_balances', { month, rows });
}

export async function getAccountMappings(): Promise<AccountMapping[]> {
  return invoke<AccountMapping[]>('get_account_mappings');
}

export async function saveAccountMapping(data: Omit<AccountMapping, 'id'>): Promise<AccountMapping> {
  return invoke<AccountMapping>('save_account_mapping', { data });
}

export async function deleteAccountMapping(id: number): Promise<boolean> {
  return invoke<boolean>('delete_account_mapping', { id });
}

// ==================== 记账凭证 ====================

export async function getVouchers(query: VoucherQuery): Promise<Voucher[]> {
  return invoke<Voucher[]>('get_vouchers', { query });
}

// ==================== 财务报表 ====================
// invoke 参数 key 用 camelCase（Tauri 2 自动映射 snake_case），与本文件既有约定一致。

export async function getBalanceSheet(month: string): Promise<BalanceSheet> {
  return invoke<BalanceSheet>('get_balance_sheet', { month });
}

export async function getIncomeStatement(month: string): Promise<IncomeStatement> {
  return invoke<IncomeStatement>('get_income_statement', { month });
}

export async function getCashFlowStatement(month: string): Promise<CashFlowStatement> {
  return invoke<CashFlowStatement>('get_cash_flow_statement', { month });
}

export async function exportFinancialReport(
  month: string,
  reportType: FinancialReportType,
  path: string,
): Promise<string> {
  return invoke<string>('export_financial_report', { month, reportType, path });
}

export async function getTrialBalance(fromMonth: string, toMonth: string): Promise<TrialBalanceReport> {
  return invoke<TrialBalanceReport>('get_trial_balance', { fromMonth, toMonth });
}

export async function exportTrialBalance(
  fromMonth: string,
  toMonth: string,
  path: string,
): Promise<string> {
  return invoke<string>('export_trial_balance', { fromMonth, toMonth, path });
}

// ==================== 个税年度汇总（第六阶段 Task 10） ====================

export async function getAnnualTaxSummary(year: number): Promise<AnnualTaxSummaryRow[]> {
  return invoke<AnnualTaxSummaryRow[]>('get_annual_tax_summary', { year });
}

export async function exportAnnualTaxSummary(year: number, path: string): Promise<string> {
  return invoke<string>('export_annual_tax_summary', { year, path });
}

// ==================== 个税扣缴申报表导出（第八阶段 Task 9，spec 8） ====================

/** 导出个税扣缴申报表 Excel（仅锁定月份；三险拆列，比例未配置退合并+尾注） */
export async function exportTaxWithholdingDeclaration(month: string, path: string): Promise<string> {
  return invoke<string>('export_tax_withholding_declaration', { month, path });
}

export async function getSocialProfiles(year: number): Promise<SocialInsuranceProfile[]> {
  return invoke<SocialInsuranceProfile[]>('get_social_profiles', { year });
}

export async function saveSocialProfile(data: SocialInsuranceProfileInput): Promise<SocialInsuranceProfile> {
  return invoke<SocialInsuranceProfile>('save_social_profile', { data });
}

export async function deleteSocialProfile(id: number): Promise<boolean> {
  return invoke<boolean>('delete_social_profile', { id });
}

export async function copySocialProfiles(
  fromYear: number,
  toYear: number,
  factor: number,
  applyClamp: boolean,
): Promise<number> {
  return invoke<number>('copy_social_profiles', { fromYear, toYear, factor, applyClamp });
}

export async function getSocialBaseLimits(): Promise<number[]> {
  return invoke<number[]>('get_social_base_limits');
}

export async function setSocialBaseLimits(
  ssMin: number,
  ssMax: number,
  hfMin: number,
  hfMax: number,
): Promise<void> {
  await invoke<void>('set_social_base_limits', { ssMin, ssMax, hfMin, hfMax });
}

// ==================== 出纳基础资料（第七阶段 Task 4） ====================
// invoke 参数 key 用 camelCase（Tauri 2 自动映射 snake_case），与本文件既有约定一致。
// 入参可空字段为 patch 语义：undefined=保留原值，''=清空。

export async function getFundAccounts(query: FundAccountQuery = {}): Promise<FundAccount[]> {
  return invoke<FundAccount[]>('get_fund_accounts', { query });
}

export async function saveFundAccount(data: FundAccountInput): Promise<FundAccount> {
  return invoke<FundAccount>('save_fund_account', { data });
}

export async function setFundAccountActive(id: number, active: boolean): Promise<FundAccount> {
  return invoke<FundAccount>('set_active_fund_account', { id, active });
}

// ==================== 历史资金归集向导（第七阶段 Task 10） ====================

export async function getFundMigrationStatus(): Promise<FundMigrationStatus> {
  return invoke<FundMigrationStatus>('get_fund_migration_status');
}

export async function previewFundAssignment(params: {
  entity_type: FundAssignmentEntityType;
  account_id: number;
  belong_month?: string | null;
  batch_id?: number | null;
}): Promise<FundAssignmentPreview> {
  return invoke<FundAssignmentPreview>('preview_fund_assignment', {
    entityType: params.entity_type,
    accountId: params.account_id,
    belongMonth: params.belong_month ?? null,
    batchId: params.batch_id ?? null,
  });
}

export async function applyFundAssignment(data: FundAssignmentInput): Promise<FundAssignmentResult> {
  return invoke<FundAssignmentResult>('apply_fund_assignment', { data });
}

export async function getBusinessPartners(query: BusinessPartnerQuery = {}): Promise<BusinessPartner[]> {
  return invoke<BusinessPartner[]>('get_business_partners', { query });
}

export async function saveBusinessPartner(data: BusinessPartnerInput): Promise<BusinessPartner> {
  return invoke<BusinessPartner>('save_business_partner', { data });
}

export async function setBusinessPartnerActive(id: number, active: boolean): Promise<BusinessPartner> {
  return invoke<BusinessPartner>('set_active_business_partner', { id, active });
}

export async function getOperatorProfiles(): Promise<OperatorProfile[]> {
  return invoke<OperatorProfile[]>('get_operator_profiles');
}

export async function saveOperatorProfile(data: OperatorProfileInput): Promise<OperatorProfile> {
  return invoke<OperatorProfile>('save_operator_profile', { data });
}

export async function setOperatorProfileActive(id: number, active: boolean): Promise<OperatorProfile> {
  return invoke<OperatorProfile>('set_active_operator_profile', { id, active });
}

export async function setCurrentOperator(operatorId: number): Promise<OperatorProfile> {
  return invoke<OperatorProfile>('set_current_operator', { operatorId });
}

export async function getCurrentOperator(): Promise<OperatorProfile | null> {
  return invoke<OperatorProfile | null>('get_current_operator');
}

// ==================== 通用加密业务附件（第七阶段 Task 5） ====================
// invoke 参数 key 用 camelCase（Tauri 2 自动映射 snake_case），与本文件既有约定一致。
// add 入参 file_path 为源文件绝对路径；返回体的 file_path 为归档路径（encrypted 由后端按
// DEK 状态裁决）。getDecryptedAttachmentUrl 返回临时解密文件绝对路径，渲染时用
// convertFileSrc() 包一层（与发票预览 getDecryptedInvoiceUrl 同模式）。

export async function addBusinessAttachment(data: BusinessAttachmentInput): Promise<BusinessAttachment> {
  return invoke<BusinessAttachment>('add_business_attachment', { data });
}

export async function listBusinessAttachments(
  entityType: string,
  entityId: number,
): Promise<BusinessAttachment[]> {
  return invoke<BusinessAttachment[]>('list_business_attachments', { entityType, entityId });
}

export async function deleteBusinessAttachment(id: number): Promise<string> {
  return invoke<string>('delete_business_attachment', { id });
}

export async function getDecryptedAttachmentUrl(attachmentId: number): Promise<string> {
  return invoke<string>('get_decrypted_attachment_url', { attachmentId });
}

// ==================== 资金单据与审批（第七阶段 Task 7） ====================
// invoke 参数 key 用 camelCase（Tauri 2 自动映射 snake_case）。状态命令的按钮可见性
// 完全由后端返回的 status 决定；approve/reject/void/reverse 意见必填由后端校验。

export async function getFundDocuments(query: FundDocumentQuery = {}): Promise<FundDocument[]> {
  return invoke<FundDocument[]>('get_fund_documents', { query });
}

export async function getFundDocumentDetail(id: number): Promise<FundDocumentDetail> {
  return invoke<FundDocumentDetail>('get_fund_document_detail', { id });
}

export async function listApprovalEvents(entityType: string, entityId: number): Promise<ApprovalEvent[]> {
  return invoke<ApprovalEvent[]>('list_approval_events', { entityType, entityId });
}

export async function getMakerCheckerEnabled(): Promise<boolean> {
  return invoke<boolean>('get_maker_checker_enabled');
}

export async function setMakerCheckerEnabled(enabled: boolean): Promise<void> {
  await invoke<void>('set_maker_checker_enabled', { enabled });
}

export async function createFundDocument(data: FundDocumentInput): Promise<FundDocument> {
  return invoke<FundDocument>('create_fund_document', { data });
}

export async function updateFundDocument(data: FundDocumentInput): Promise<FundDocument> {
  return invoke<FundDocument>('update_fund_document', { data });
}

export async function submitFundDocument(id: number, comment?: string): Promise<FundDocument> {
  return invoke<FundDocument>('submit_fund_document', { id, comment });
}

export async function approveFundDocument(id: number, comment: string): Promise<FundDocument> {
  return invoke<FundDocument>('approve_fund_document', { id, comment });
}

export async function rejectFundDocument(id: number, comment: string): Promise<FundDocument> {
  return invoke<FundDocument>('reject_fund_document', { id, comment });
}

export async function withdrawFundDocument(id: number, comment?: string): Promise<FundDocument> {
  return invoke<FundDocument>('withdraw_fund_document', { id, comment });
}

export async function voidFundDocument(id: number, comment: string): Promise<FundDocument> {
  return invoke<FundDocument>('void_fund_document', { id, comment });
}

export async function settleFundDocument(id: number): Promise<FundDocument> {
  return invoke<FundDocument>('settle_fund_document', { id });
}

export async function reverseFundDocument(data: FundDocumentReverseInput): Promise<FundDocument> {
  return invoke<FundDocument>('reverse_fund_document', { data });
}

// ==================== 员工借款备用金与核销（第七阶段 Task 14，spec 4.11） ====================

export async function getAdvanceLedger(query: AdvanceLedgerQuery = {}): Promise<AdvanceLedger> {
  return invoke<AdvanceLedger>('get_advance_ledger', { query });
}

export async function getAdvanceSettlementLinks(advanceId: number): Promise<AdvanceSettlementLink[]> {
  return invoke<AdvanceSettlementLink[]>('get_advance_settlement_links', { advanceId });
}

export async function cancelAdvanceSettlementLink(
  data: AdvanceLinkCancelInput,
): Promise<FundDocument> {
  return invoke<FundDocument>('cancel_advance_settlement_link', { data });
}

export async function exportAdvanceLedger(
  query: AdvanceLedgerQuery,
  path: string,
): Promise<string> {
  return invoke<string>('export_advance_ledger', { query, path });
}

// ==================== 票据台账（第八阶段 Task 3） ====================
// invoke 参数 key 用 camelCase（Tauri 2 自动映射 snake_case）。状态命令的按钮可见性
// 完全由后端返回的 status 决定；void/reverse 原因必填、贴现实收不超票面、背书全额
// 等校验以后端为准。

export async function getNegotiableInstruments(
  query: InstrumentQuery = {},
): Promise<NegotiableInstrument[]> {
  return invoke<NegotiableInstrument[]>('get_negotiable_instruments', { query });
}

export async function getInstrumentDetail(id: number): Promise<InstrumentDetail> {
  return invoke<InstrumentDetail>('get_instrument_detail', { id });
}

export async function registerInstrument(
  data: InstrumentRegisterInput,
): Promise<NegotiableInstrument> {
  return invoke<NegotiableInstrument>('register_instrument', { data });
}

export async function endorseInstrument(data: InstrumentEndorseInput): Promise<InstrumentEndorseResult> {
  return invoke<InstrumentEndorseResult>('endorse_instrument', { data });
}

export async function discountInstrument(data: InstrumentDiscountInput): Promise<NegotiableInstrument> {
  return invoke<NegotiableInstrument>('discount_instrument', { data });
}

export async function startCollection(id: number, operateDate: string): Promise<NegotiableInstrument> {
  return invoke<NegotiableInstrument>('start_collection', { id, operateDate });
}

export async function confirmCollection(
  data: InstrumentCollectConfirmInput,
): Promise<NegotiableInstrument> {
  return invoke<NegotiableInstrument>('confirm_collection', { data });
}

export async function settleIssuedInstrument(data: InstrumentSettleInput): Promise<NegotiableInstrument> {
  return invoke<NegotiableInstrument>('settle_issued_instrument', { data });
}

export async function voidInstrument(id: number, reason: string): Promise<NegotiableInstrument> {
  return invoke<NegotiableInstrument>('void_instrument', { id, reason });
}

export async function reverseInstrumentFlow(data: InstrumentReverseInput): Promise<NegotiableInstrument> {
  return invoke<NegotiableInstrument>('reverse_instrument_flow', { data });
}

// ==================== 现金盘点单（第八阶段 Task 6，spec 6） ====================
// 账户限现金类、面额合计=实存、差异≠0 原因必填、confirmed 不可改、作废差异凭证走
// 红字冲正等校验以后端为准；invoke 参数 key 用 camelCase（Tauri 2 自动映射 snake_case）。

export async function getCountSheets(query: CashCountQuery = {}): Promise<CashCountSheet[]> {
  return invoke<CashCountSheet[]>('get_count_sheets', { query });
}

export async function getCountSheetDetail(id: number): Promise<CashCountSheetDetail> {
  return invoke<CashCountSheetDetail>('get_count_sheet_detail', { id });
}

export async function createCountSheet(data: CashCountCreateInput): Promise<CashCountSheet> {
  return invoke<CashCountSheet>('create_count_sheet', { data });
}

export async function updateCountSheet(
  id: number,
  data: CashCountUpdateInput,
): Promise<CashCountSheet> {
  return invoke<CashCountSheet>('update_count_sheet', { id, data });
}

export async function confirmCountSheet(id: number): Promise<CashCountSheet> {
  return invoke<CashCountSheet>('confirm_count_sheet', { id });
}

export async function voidCountSheet(id: number, reason?: string): Promise<CashCountSheet> {
  return invoke<CashCountSheet>('void_count_sheet', { id, reason: reason ?? null });
}
