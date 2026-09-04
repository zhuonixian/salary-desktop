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
  BankAutoMatchResult,
  BankImportPreview,
  BankTransaction,
  BankTransactionIgnoreInput,
  BankTransactionMatch,
  BankTransactionMatchInput,
  BankTransactionQuery,
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
  ReimbursementStatus,
  PaymentStatus,
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
  FundDocumentDetail,
  FundDocumentReverseInput,
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
    default:
      if (command.startsWith('get_') || command.startsWith('query_')) return [];
      if (command.startsWith('export_') || command.startsWith('delete_') || command.startsWith('update_')) return true;
      return true;
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
