// 资金单据页面与其抽取组件共用的常量、类型与轻量工具
// 自 src/pages/FundDocuments.tsx 抽取（纯搬家，行为不变），供页面与
// FundDocumentFormModal / FundDocumentDetailDrawer 两个组件共用。
// 注：含返回 JSX 的 statusTag，故为 .tsx 文件（.ts 不支持 JSX）。
import type { ReactNode } from 'react';
import { Tag } from 'antd';
import type { Dayjs } from 'dayjs';
import { FUND_DOCUMENT_STATUS_LABEL, FUND_DOCUMENT_TYPE_LABEL } from '@/types';
import type {
  BusinessPartner,
  Employee,
  FundAccount,
  OperatorProfile,
} from '@/types';

export const errText = (e: unknown): string => (e instanceof Error ? e.message : String(e));

export const fmtAmount = (value: number): string =>
  (Number(value) || 0).toLocaleString('zh-CN', { minimumFractionDigits: 2, maximumFractionDigits: 2 });

// 状态颜色（与后端 cashier.rs fund_status_label 中文一致）
export const STATUS_COLOR: Record<string, string> = {
  draft: 'default',
  submitted: 'processing',
  approved: 'success',
  rejected: 'warning',
  batched: 'gold',
  settled: 'geekblue',
  void: '#999999',
  reversed: 'purple',
};

// 状态标签（单据列表页表格列与本抽屉共用同一实现；此前因 .ts 不支持 JSX
// 暂居 FundDocumentDetailDrawer.tsx，为满足 react-refresh/only-export-components 迁回）
export const statusTag = (status: string): ReactNode => (
  <Tag color={STATUS_COLOR[status] ?? 'default'}>
    {FUND_DOCUMENT_STATUS_LABEL[status] ?? status}
  </Tag>
);

// 本页可手工创建的单据类型（员工借款/借款核销/冲正分别在借款页与冲正入口产生）
export const CREATABLE_TYPE_OPTIONS = [
  { value: 'receipt', label: '收款单' },
  { value: 'payment', label: '付款单' },
  { value: 'transfer', label: '内部转账单' },
];

export const STATUS_OPTIONS = Object.entries(FUND_DOCUMENT_STATUS_LABEL).map(([value, label]) => ({
  value,
  label,
}));

export const TYPE_OPTIONS = Object.entries(FUND_DOCUMENT_TYPE_LABEL).map(([value, label]) => ({
  value,
  label,
}));

// 付款/借款单审批后须经付款批次（spec 5.1）；其余类型可直接结算
export const BATCHABLE_TYPES = ['payment', 'advance'];
// 附件可变更状态（与后端 FUND_DOCUMENT_ATTACHMENT_EDITABLE_STATUSES 一致）
export const ATTACHMENT_EDITABLE_STATUSES = ['draft', 'rejected', 'void'];

export interface SharedOptions {
  partners: BusinessPartner[];
  employees: Employee[];
  accounts: FundAccount[];
  glOptions: { value: string; label: string }[];
  operators: OperatorProfile[];
}

export interface FundDocFormValues {
  document_type: string;
  belong_month: Dayjs;
  document_date: Dayjs;
  amount?: number;
  summary: string;
  department?: string;
  expense_type?: string;
  counterparty_kind: 'partner' | 'employee';
  partner_id?: number;
  employee_id?: number;
  source_account_id?: number;
  target_account_id?: number;
  counter_account_code?: string;
  remark?: string;
}

export type CommentAction = 'approve' | 'reject' | 'void';

export const COMMENT_META: Record<CommentAction, { title: string; placeholder: string; okText: string }> = {
  approve: {
    title: '审批通过',
    placeholder: '请输入审批意见（必填）',
    okText: '通过',
  },
  reject: {
    title: '驳回',
    placeholder: '请填写驳回意见（必填），提交人可按意见修改后重新提交',
    okText: '驳回',
  },
  void: {
    title: '作废单据',
    placeholder: '请填写作废原因（必填）',
    okText: '作废',
  },
};
