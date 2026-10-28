import { useEffect, useState } from 'react';
import { Alert, Button, Modal } from 'antd';

import { getGlAccounts } from '@/api';
import type { GlAccount, Voucher, VoucherLine } from '@/types';
import { SensitiveText } from '@/components/SensitiveText';
import { useSecurity } from '@/contexts/SecurityContext';

/** 凭证日期转中文展示：2026-08-05 → 2026年08月05日 */
const fmtCnDate = (date: string): string => {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(date ?? '');
  return m ? `${m[1]}年${m[2]}月${m[3]}日` : date;
};

/** 分录按 line_order 升序（与凭证页展开行同口径） */
const sortedLines = (lines: VoucherLine[]): VoucherLine[] =>
  [...lines].sort((a, b) => a.line_order - b.line_order);

/** 中式凭证版式最少行数：不足补空行，保持凭证纸张观感 */
const MIN_PRINT_ROWS = 5;

/** 金额千分位 + 两位小数（与凭证页手工凭证合计 Vouchers.fmtMoney 同口径：
 *  消除 reduce 浮点尾巴（如 3588.67+466.47=4055.1400000000003），整数也补齐 84220 → 84,220.00） */
const fmtMoney = (value: number): string =>
  (Number(value) || 0).toLocaleString('zh-CN', { minimumFractionDigits: 2, maximumFractionDigits: 2 });

export interface VoucherPrintModalProps {
  open: boolean;
  /** 待打印凭证（支持多张连续打印，每张独立分页） */
  vouchers: Voucher[];
  onClose: () => void;
}

/**
 * 记账凭证打印预览 Modal（中式记账凭证版式）：
 * - 凭证种类与字号（如 记-202608-001）+ 凭证日期；
 * - 分录表格（摘要 / 科目 / 借方金额 / 贷方金额）+ 合计行（借贷相等才标注平衡）；
 * - 附件张数与制单署名区（纸质凭证留空手签）；
 * - 金额展示沿用页面现有 SensitiveText 脱敏口径；
 * - 打印门禁参照工资条打印：未解锁敏感数据时禁用打印按钮并提示。
 */
const VoucherPrintModal: React.FC<VoucherPrintModalProps> = ({ open, vouchers, onClose }) => {
  const { isSensitiveRevealed } = useSecurity();
  // 科目编码 → 名称映射：桌面模式展示「编码 名称」，预览 mock 无科目数据时回退为仅编码
  const [accountNames, setAccountNames] = useState<Record<string, string>>({});

  useEffect(() => {
    if (!open) return;
    let alive = true;
    void (async () => {
      try {
        const accounts: GlAccount[] = await getGlAccounts();
        if (!alive) return;
        const map: Record<string, string> = {};
        accounts.forEach((a) => {
          map[a.code] = a.name;
        });
        setAccountNames(map);
      } catch {
        // 获取科目失败不影响打印：科目列回退为仅显示编码
      }
    })();
    return () => {
      alive = false;
    };
  }, [open]);

  /** 渲染单张凭证（.voucher-print-sheet 为打印分页单位） */
  const renderSheet = (v: Voucher) => {
    const lines = sortedLines(v.lines);
    // 借贷合计与平衡判定（与凭证页手工录入同口径：差额四舍五入到分后 < 0.005 视为平衡）
    const debitTotal = lines.reduce((s, l) => s + (Number(l.debit_amount) || 0), 0);
    const creditTotal = lines.reduce((s, l) => s + (Number(l.credit_amount) || 0), 0);
    const balanced =
      Math.abs(Math.round((debitTotal - creditTotal) * 100) / 100) < 0.005;
    // 不足最少行数时补空行（null 行仅占位）
    const rows: (VoucherLine | null)[] = [...lines];
    while (rows.length < MIN_PRINT_ROWS) rows.push(null);

    return (
      <div key={v.id} className="voucher-print-sheet">
        <span className="voucher-print-title">
          记账凭证
          {v.status === 'void' && <span className="voucher-print-void">（已作废）</span>}
        </span>
        <div className="voucher-print-meta">
          <span>
            凭证字号：<b>{v.voucher_no}</b>
          </span>
          <span>凭证日期：{fmtCnDate(v.voucher_date)}</span>
          <span>第 {v.belong_month} 期</span>
        </div>
        <table className="voucher-print-table">
          <thead>
            <tr>
              <th className="voucher-print-summary">摘要</th>
              <th className="voucher-print-account">科目</th>
              <th className="voucher-print-num">借方金额</th>
              <th className="voucher-print-num">贷方金额</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((line, idx) =>
              line ? (
                <tr key={line.id}>
                  <td className="voucher-print-summary">{line.summary ?? ''}</td>
                  <td className="voucher-print-account">
                    {line.account_code}
                    {accountNames[line.account_code] ? ` ${accountNames[line.account_code]}` : ''}
                  </td>
                  <td className="voucher-print-num">
                    {line.debit_amount > 0 ? (
                      <SensitiveText type="amount" value={line.debit_amount} revealable={false} />
                    ) : (
                      ''
                    )}
                  </td>
                  <td className="voucher-print-num">
                    {line.credit_amount > 0 ? (
                      <SensitiveText type="amount" value={line.credit_amount} revealable={false} />
                    ) : (
                      ''
                    )}
                  </td>
                </tr>
              ) : (
                // 补位空行：保持中式凭证固定行数的纸张观感
                <tr key={`pad-${idx}`} className="voucher-print-pad-row">
                  <td className="voucher-print-summary"> </td>
                  <td className="voucher-print-account" />
                  <td className="voucher-print-num" />
                  <td className="voucher-print-num" />
                </tr>
              ),
            )}
            <tr className="voucher-print-total-row">
              <td colSpan={2}>
                合计
                {/* 借贷合计必须相等才展示平衡标注，不相等时红色警示 */}
                {balanced ? (
                  <span className="voucher-print-balanced">（借贷平衡）</span>
                ) : (
                  <span className="voucher-print-unbalanced">（借贷不平衡，请先核对）</span>
                )}
              </td>
              <td className="voucher-print-num">
                <SensitiveText type="amount" value={fmtMoney(debitTotal)} revealable={false} />
              </td>
              <td className="voucher-print-num">
                <SensitiveText type="amount" value={fmtMoney(creditTotal)} revealable={false} />
              </td>
            </tr>
          </tbody>
        </table>
        <div className="voucher-print-foot">
          <span>附件：______ 张</span>
          <span>制单：__________</span>
          <span>审核：__________</span>
          <span>记账：__________</span>
          <span>出纳：__________</span>
        </div>
      </div>
    );
  };

  return (
    <Modal
      open={open}
      onCancel={onClose}
      title={`记账凭证打印预览（共 ${vouchers.length} 张）`}
      width={860}
      footer={[
        // 打印门禁与工资条打印一致：未解锁敏感数据时禁用，避免打印出脱敏「¥ ****」
        <Button
          key="print"
          type="primary"
          disabled={!isSensitiveRevealed}
          onClick={() => window.print()}
        >
          打印 / 另存 PDF
        </Button>,
      ]}
    >
      {!isSensitiveRevealed && (
        <Alert
          type="warning"
          showIcon
          message="金额已脱敏。请先解锁敏感数据（点击任意金额的眼睛图标解锁）查看明文并打印"
          style={{ marginBottom: 16 }}
        />
      )}
      {/* 打印区：@media print 时仅此区域可见，多张凭证每张独立分页 */}
      <div className="voucher-print-area">{vouchers.map(renderSheet)}</div>
    </Modal>
  );
};

export default VoucherPrintModal;
