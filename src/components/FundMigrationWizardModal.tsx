import { useCallback, useEffect, useMemo, useState } from 'react';
import { Alert, Button, Modal, Popconfirm, Segmented, Select, Space, Table, Typography, message } from 'antd';
import type { ColumnsType } from 'antd/es/table';
import { MergeOutlined } from '@ant-design/icons';
import SensitiveText from '@/components/SensitiveText';
import {
  applyFundAssignment,
  getFundAccounts,
  getFundMigrationStatus,
  previewFundAssignment,
} from '@/api';
import type {
  FundAccount,
  FundAssignmentEntityType,
  FundAssignmentInput,
  FundAssignmentPreview,
  FundMigrationStatus,
  PaymentBatch,
} from '@/types';
import { errText, fmtAmount } from '@/components/fundAccountsShared';

// 付款批次类型中文标签（与历史归集向导配套）。
const BATCH_TYPE_LABEL: Record<string, string> = {
  salary: '工资',
  reimbursement: '报销',
  general: '通用',
};

export interface FundMigrationWizardModalProps {
  open: boolean;
  onClose: () => void;
}

/**
 * 历史归集向导：展示待归集统计（流水/批次/分录）→ 按类别选择目标账户 → 自动预览 →
 * 用户确认执行 → 结果反馈（成功 N 条 / 跳过 M 条分录）。
 * 口径（spec 9）：不按金额/账号猜测归属；无法唯一确定的独立分录保持未归集；
 * 已月结月份写入会被后端拦截；void 凭证分录与 void 批次不在待归集之列。
 */
const FundMigrationWizardModal: React.FC<FundMigrationWizardModalProps> = ({ open, onClose }) => {
  const [status, setStatus] = useState<FundMigrationStatus | null>(null);
  const [accounts, setAccounts] = useState<FundAccount[]>([]);
  const [category, setCategory] = useState<FundAssignmentEntityType>('bank_transaction');
  const [month, setMonth] = useState<string | undefined>(undefined);
  const [accountId, setAccountId] = useState<number | undefined>(undefined);
  const [batchId, setBatchId] = useState<number | undefined>(undefined);
  const [preview, setPreview] = useState<FundAssignmentPreview | null>(null);
  const [applying, setApplying] = useState(false);

  const reloadStatus = useCallback(async () => {
    try {
      setStatus(await getFundMigrationStatus());
    } catch (e: unknown) {
      message.error('获取归集状态失败: ' + errText(e));
    }
  }, []);

  useEffect(() => {
    if (!open) return;
    setCategory('bank_transaction');
    setMonth(undefined);
    setAccountId(undefined);
    setBatchId(undefined);
    setPreview(null);
    reloadStatus();
    getFundAccounts({ is_active: true })
      .then(setAccounts)
      .catch((e: unknown) => message.error('获取资金账户失败: ' + errText(e)));
  }, [open, reloadStatus]);

  // 候选账户：流水限 bank/third_party（与流水导入口径一致）；付款批次不限账户类型
  const accountOptions = useMemo(
    () =>
      accounts
        .filter((a) => (category === 'bank_transaction' ? a.account_type !== 'cash' : true))
        .map((a) => ({ value: a.id, label: `${a.name}（${a.account_code}）` })),
    [accounts, category],
  );

  // 单账户唯一候选自动预填（"单账户唯一映射可预览"，写入仍需用户确认，不静默写入）
  useEffect(() => {
    if (accountId === undefined && accountOptions.length === 1) {
      setAccountId(accountOptions[0].value);
    }
  }, [accountOptions, accountId]);

  // 选择齐备后自动预览（只读，不写库）
  useEffect(() => {
    if (!open || accountId === undefined) {
      setPreview(null);
      return;
    }
    if (category === 'payment_batch' && batchId === undefined) {
      setPreview(null);
      return;
    }
    let cancelled = false;
    previewFundAssignment({
      entity_type: category,
      account_id: accountId,
      belong_month: category === 'bank_transaction' ? month ?? null : null,
      batch_id: category === 'payment_batch' ? batchId ?? null : null,
    })
      .then((p) => {
        if (!cancelled) setPreview(p);
      })
      .catch((e: unknown) => {
        if (!cancelled) {
          setPreview(null);
          message.error('预览失败: ' + errText(e));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [open, category, month, accountId, batchId]);

  const handleApply = async () => {
    if (accountId === undefined) return;
    setApplying(true);
    try {
      const data: FundAssignmentInput = {
        entity_type: category,
        account_id: accountId,
        belong_month: category === 'bank_transaction' ? month ?? null : null,
        batch_id: category === 'payment_batch' ? batchId ?? null : null,
      };
      const result = await applyFundAssignment(data);
      const skippedNote =
        result.skipped_voucher_lines > 0
          ? `，跳过分录 ${result.skipped_voucher_lines} 条（保持未归集）`
          : '';
      message.success(
        `归集完成：成功 ${result.updated_count} 条，联动资金分录 ${result.linked_voucher_lines_updated} 条${skippedNote}`,
      );
      setBatchId(undefined);
      setPreview(null);
      await reloadStatus();
    } catch (e: unknown) {
      message.error('归集失败: ' + errText(e));
    } finally {
      setApplying(false);
    }
  };

  const monthOptions = (status?.bank_months ?? []).map((m) => ({
    value: m.belong_month,
    label: `${m.belong_month}（流水 ${m.bank_transactions} 条 / 分录 ${m.voucher_lines} 条）`,
  }));

  const canApply =
    accountId !== undefined && (category === 'bank_transaction' || batchId !== undefined);

  const previewText = !preview
    ? null
    : preview.item_count === 0
      ? '该范围内暂无待归集数据。'
      : `预览：将归集 ${preview.item_count} 条${
          category === 'bank_transaction' ? '流水' : '批次'
        }，联动补齐 ${preview.affected_voucher_lines} 条资金分录` +
        (preview.skipped_voucher_lines > 0
          ? `；${preview.skipped_voucher_lines} 条分录因凭证已作废或科目不一致保持未归集。`
          : '。');

  const batchColumns: ColumnsType<PaymentBatch> = [
    { title: '批次号', dataIndex: 'batch_no', width: 150 },
    { title: '归属月', dataIndex: 'belong_month', width: 90 },
    {
      title: '类型',
      dataIndex: 'batch_type',
      width: 80,
      render: (v: string) => BATCH_TYPE_LABEL[v] ?? v,
    },
    { title: '状态', dataIndex: 'status', width: 80 },
    {
      title: '金额',
      dataIndex: 'total_amount',
      width: 110,
      align: 'right',
      render: (v: number) => <SensitiveText type="amount" value={fmtAmount(v)} />,
    },
  ];

  return (
    <Modal
      title="历史资金归集向导"
      open={open}
      onCancel={onClose}
      width={760}
      footer={null}
      destroyOnHidden
    >
      <Alert
        type="info"
        showIcon
        style={{ marginBottom: 12 }}
        message={
          status
            ? `待归集：银行流水 ${status.unassigned_bank_transactions} 条（${
                status.bank_months.length
              } 个月）、付款批次 ${status.unassigned_payment_batches} 个、资金分录 ${
                status.unassigned_voucher_lines
              } 条${
                status.unlinked_voucher_lines > 0
                  ? `（其中 ${status.unlinked_voucher_lines} 条无法按批次/流水自动联动，保持未归集，不猜测归属）`
                  : ''
              }`
            : '正在加载待归集统计…'
        }
        description="历史数据无法确定归属时不做猜测：请按月指定流水账户、按批次指定付款账户；系统仅联动生效凭证的资金分录。已正式月结的月份会被拦截。"
      />
      <Space direction="vertical" size={12} style={{ display: 'flex', marginBottom: 12 }}>
        <Segmented
          value={category}
          onChange={(v) => setCategory(v as FundAssignmentEntityType)}
          options={[
            { label: '银行流水', value: 'bank_transaction' },
            { label: '付款批次', value: 'payment_batch' },
          ]}
        />
        {category === 'bank_transaction' ? (
          <Space wrap>
            <Select
              style={{ width: 340 }}
              placeholder="归属月（默认全部待归集月份）"
              allowClear
              value={month}
              onChange={(v) => setMonth(v)}
              options={monthOptions}
            />
            <Select
              style={{ width: 260 }}
              placeholder="目标账户"
              value={accountId}
              onChange={(v) => setAccountId(v)}
              options={accountOptions}
            />
          </Space>
        ) : (
          <Space wrap>
            <Select
              style={{ width: 260 }}
              placeholder="目标账户"
              value={accountId}
              onChange={(v) => setAccountId(v)}
              options={accountOptions}
            />
            <Typography.Text type="secondary">
              在下方列表选择一个待归集批次（单账户唯一候选已自动预填）
            </Typography.Text>
          </Space>
        )}
      </Space>

      {category === 'payment_batch' && (
        <Table
          rowKey="id"
          size="small"
          columns={batchColumns}
          dataSource={status?.pending_batches ?? []}
          loading={!status}
          pagination={false}
          rowSelection={{
            type: 'radio',
            selectedRowKeys: batchId !== undefined ? [batchId] : [],
            onChange: (keys) => setBatchId(keys[0] as number),
          }}
          onRow={(record) => ({ onClick: () => setBatchId(record.id) })}
          locale={{ emptyText: '没有待归集的历史付款批次' }}
          style={{ marginBottom: 12 }}
        />
      )}

      {previewText && (
        <Alert
          type={preview && preview.item_count > 0 ? 'warning' : 'success'}
          showIcon
          style={{ marginBottom: 12 }}
          message={previewText}
        />
      )}

      <Space style={{ justifyContent: 'flex-end', display: 'flex' }}>
        <Button onClick={onClose}>关闭</Button>
        <Popconfirm
          title="确认执行归集？"
          description="写入前会再次校验相关月份未被月结锁定；操作将记入操作日志。"
          disabled={!canApply || applying}
          onConfirm={handleApply}
        >
          <Button type="primary" icon={<MergeOutlined />} disabled={!canApply} loading={applying}>
            执行归集
          </Button>
        </Popconfirm>
      </Space>
    </Modal>
  );
};

export default FundMigrationWizardModal;
