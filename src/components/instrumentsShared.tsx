// 票据台账共享常量、公共小件与表单值类型
// 自 src/pages/NotesInstruments.tsx 抽取（纯搬家），供页面与
// InstrumentRegisterModal / InstrumentDetailDrawer 两个组件共用。
import type { ReactNode } from 'react';
import { Tag } from 'antd';
import dayjs from 'dayjs';
import type { Dayjs } from 'dayjs';
import {
  INSTRUMENT_DIRECTION_LABEL,
  INSTRUMENT_STATUS_LABEL,
  INSTRUMENT_TYPE_LABEL,
} from '@/types';
import type {
  BusinessPartner,
  FundAccount,
  NegotiableInstrument,
  OperatorProfile,
} from '@/types';

export const errText = (e: unknown): string => (e instanceof Error ? e.message : String(e));

export const fmtAmount = (value: number): string =>
  (Number(value) || 0).toLocaleString('zh-CN', { minimumFractionDigits: 2, maximumFractionDigits: 2 });

// 状态颜色（与后端 notes.rs status_label 中文一致）
const STATUS_COLOR: Record<string, string> = {
  holding: 'processing',
  endorsed_out: 'purple',
  discounted: 'geekblue',
  collecting: 'gold',
  collected: 'success',
  issued_outstanding: 'warning',
  paid: 'success',
  void: '#999999',
};

export const STATUS_OPTIONS = Object.entries(INSTRUMENT_STATUS_LABEL).map(([value, label]) => ({
  value,
  label,
}));

export const TYPE_OPTIONS = Object.entries(INSTRUMENT_TYPE_LABEL).map(([value, label]) => ({
  value,
  label,
}));

// 方向标签（全部 Tab 表格列展示用）
export const directionTag = (direction: string): ReactNode => (
  <Tag color={direction === 'received' ? 'blue' : 'orange'}>
    {INSTRUMENT_DIRECTION_LABEL[direction] ?? direction}
  </Tag>
);

export const statusTag = (status: string): ReactNode => (
  <Tag color={STATUS_COLOR[status] ?? 'default'}>{INSTRUMENT_STATUS_LABEL[status] ?? status}</Tag>
);

// 可红字冲正的流转后状态（撤销最后一次流转；作废为纯终态不可冲正）
export const REVERSABLE_STATUSES = ['endorsed_out', 'discounted', 'collected', 'paid'];

// 冲正恢复态说明（与后端 reverse_instrument_flow 恢复口径一致）
export const reverseTargetLabel = (inst: NegotiableInstrument): string => {
  if (inst.instrument_type === 'check') return '已作废（登记即终态的纠错，同号可重新登记）';
  if (inst.status === 'paid') return '已开出未兑付（可重新兑付或作废）';
  return '持有（可改道背书/贴现/托收或作废）';
};

export interface SharedOptions {
  accounts: FundAccount[];
  partners: BusinessPartner[];
  glOptions: { value: string; label: string }[];
  operators: OperatorProfile[];
}

// ==================== 日期/账户公共小件 ====================

// 业务月份内的缺省操作日期（与收付款单页同模式，操作月须未月结由后端强校验）
export const monthDate = (month: Dayjs): Dayjs => month.date(Math.min(dayjs().date(), month.daysInMonth()));

export const accountSelectOptions = (accounts: FundAccount[]) =>
  accounts
    .filter((a) => a.is_active)
    .map((a) => ({ value: a.id, label: `${a.name}（${a.account_code}）` }));

export const accountName = (accounts: FundAccount[], id: number | null): string =>
  id === null ? '-' : accounts.find((a) => a.id === id)?.name ?? `账户ID=${id}`;

export interface RegisterFormValues {
  instrument_type: string;
  direction: string;
  instrument_no: string;
  face_amount?: number;
  issue_date: Dayjs;
  due_date: Dayjs;
  drawer?: string;
  acceptor?: string;
  payee?: string;
  partner_id?: number;
  fund_account_id?: number;
  counter_account_code?: string;
  remark?: string;
}

export interface EndorseFormValues {
  endorsee: string;
  endorse_date: Dayjs;
  purpose?: string;
  counter_account_code?: string;
}

export interface DiscountFormValues {
  discount_date: Dayjs;
  proceeds?: number;
  fund_account_id?: number;
}

export interface OperateFormValues {
  operate_date: Dayjs;
  fund_account_id?: number;
}

export interface ReverseFormValues {
  reverse_date: Dayjs;
  reason: string;
}
