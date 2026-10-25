// 银行对账页面与对账组件三方共用常量/工具。
// 从 src/pages/BankTransactions.tsx 抽取（纯搬家，行为不变）。
import type { BankTransaction } from '@/types';

/** 金额容差（元）：金额差小于该值视为已清零/两侧平衡 */
export const AMOUNT_TOLERANCE = 0.005;

/** 金额展示格式化：千分位 + 两位小数 */
export const fmtMoney = (value?: number | null) =>
  (value ?? 0).toLocaleString('zh-CN', { minimumFractionDigits: 2, maximumFractionDigits: 2 });

/** 流水方向金额：银行流水收入/支出单边有值，取较大一侧作为该流水金额 */
export const txSideAmount = (tx: Pick<BankTransaction, 'income_amount' | 'expense_amount'>) =>
  tx.income_amount > tx.expense_amount ? tx.income_amount : tx.expense_amount;
