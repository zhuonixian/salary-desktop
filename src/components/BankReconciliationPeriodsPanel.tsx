// 余额调节表面板（银行对账 Tab 3）：按账户+月份生成银行余额调节表，查看差额、确认与敏感导出。
// 从 src/pages/BankTransactions.tsx 抽取（纯搬家，行为与 UI 不变）。
import { useCallback, useEffect, useState } from 'react';
import {
  Alert,
  Button,
  Card,
  InputNumber,
  Popconfirm,
  Select,
  Space,
  Table,
  Tag,
  Tooltip,
  message,
} from 'antd';
import type { ColumnsType } from 'antd/es/table';
import { ReloadOutlined } from '@ant-design/icons';
import { save } from '@tauri-apps/plugin-dialog';
import {
  confirmBankReconciliationPeriod,
  exportBankReconciliationPeriod,
  generateBankReconciliationPeriod,
  listBankReconciliationPeriods,
} from '@/api';
import SensitiveText from '@/components/SensitiveText';
import { AMOUNT_TOLERANCE } from '@/components/bankTransactionsShared';
import { useSecurity } from '@/contexts/SecurityContext';
import type { BankReconciliationPeriod, FundAccount } from '@/types';

// ==================== Tab 3：余额调节表 ====================

export interface BankReconciliationPeriodsPanelProps {
  month: string;
  accounts: FundAccount[];
  accountId?: number;
  onAccountChange: (id?: number) => void;
  onGoWorkbench: () => void;
}

const BankReconciliationPeriodsPanel: React.FC<BankReconciliationPeriodsPanelProps> = ({
  month,
  accounts,
  accountId,
  onAccountChange,
  onGoWorkbench,
}) => {
  const { isSensitiveRevealed } = useSecurity();
  const [periods, setPeriods] = useState<BankReconciliationPeriod[]>([]);
  const [loading, setLoading] = useState(false);
  const [openingOverride, setOpeningOverride] = useState<number | null>(null);
  const [closingOverride, setClosingOverride] = useState<number | null>(null);
  const [generating, setGenerating] = useState(false);
  const [confirmingId, setConfirmingId] = useState<number | null>(null);
  const [exportingId, setExportingId] = useState<number | null>(null);

  const fetchPeriods = useCallback(async () => {
    setLoading(true);
    try {
      setPeriods(await listBankReconciliationPeriods(accountId));
    } catch (e: unknown) {
      message.error('查询余额调节表失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setLoading(false);
    }
  }, [accountId]);

  useEffect(() => {
    void fetchPeriods();
  }, [fetchPeriods]);

  const latest = periods[0];
  const latestUnbalanced = !!latest && Math.abs(latest.difference) > AMOUNT_TOLERANCE;

  const handleGenerate = async () => {
    if (!accountId) {
      message.warning('请先选择资金账户');
      return;
    }
    setGenerating(true);
    try {
      const period = await generateBankReconciliationPeriod(
        accountId,
        month,
        openingOverride ?? undefined,
        closingOverride ?? undefined,
      );
      message.success(
        `已生成 ${period.belong_month} 调节表（对账单余额来源：${
          { derived: '流水推算', manual: '人工录入', carried: '结转上期', empty: '无来源' }[
            period.statement_source
          ] ?? period.statement_source
        }）`,
      );
      await fetchPeriods();
    } catch (e: unknown) {
      message.error('生成调节表失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setGenerating(false);
    }
  };

  const handleConfirm = async (id: number) => {
    setConfirmingId(id);
    try {
      await confirmBankReconciliationPeriod(id);
      message.success('余额调节表已确认');
      await fetchPeriods();
    } catch (e: unknown) {
      message.error('确认失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setConfirmingId(null);
    }
  };

  const handleExport = async (period: BankReconciliationPeriod) => {
    if (!isSensitiveRevealed) {
      message.warning('敏感导出需先解锁敏感数据');
      return;
    }
    const target = await save({
      defaultPath: `银行余额调节表_${period.fund_account_name ?? ''}_${period.belong_month.replace('-', '')}.xlsx`,
      filters: [{ name: '余额调节表', extensions: ['xlsx'] }],
    });
    if (!target) return;
    setExportingId(period.id);
    try {
      await exportBankReconciliationPeriod(period.id, String(target));
      message.success('余额调节表已导出');
    } catch (e: unknown) {
      message.error('导出失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setExportingId(null);
    }
  };

  const columns: ColumnsType<BankReconciliationPeriod> = [
    { title: '月份', dataIndex: 'belong_month', key: 'belong_month', width: 90 },
    {
      title: '对账单期初',
      dataIndex: 'statement_opening_balance',
      key: 'opening',
      width: 110,
      align: 'right',
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '对账单期末',
      dataIndex: 'statement_closing_balance',
      key: 'closing',
      width: 110,
      align: 'right',
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '账面期末',
      dataIndex: 'book_closing_balance',
      key: 'book',
      width: 110,
      align: 'right',
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '调节后（账面侧）',
      dataIndex: 'adjusted_book_balance',
      key: 'adjusted_book',
      width: 130,
      align: 'right',
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '调节后（银行侧）',
      dataIndex: 'adjusted_bank_balance',
      key: 'adjusted_bank',
      width: 130,
      align: 'right',
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '差额',
      dataIndex: 'difference',
      key: 'difference',
      width: 110,
      align: 'right',
      render: (v: number) =>
        Math.abs(v) > AMOUNT_TOLERANCE ? (
          <span style={{ color: '#cf1322', fontWeight: 600 }}>
            <SensitiveText type="amount" value={v} />
          </span>
        ) : (
          <SensitiveText type="amount" value={v} />
        ),
    },
    {
      title: '状态',
      dataIndex: 'status',
      key: 'status',
      width: 110,
      render: (s: string, r: BankReconciliationPeriod) => (
        <Space size={4}>
          <Tag color={s === 'confirmed' ? 'green' : 'gold'}>
            {s === 'confirmed' ? '已确认' : '草稿'}
          </Tag>
          {s === 'confirmed' && (
            <Tooltip title={`${r.confirmed_by ?? ''} ${r.confirmed_at ?? ''}`}>
              <span style={{ color: '#8c8c8c', fontSize: 12 }}>详情</span>
            </Tooltip>
          )}
        </Space>
      ),
    },
    {
      title: '操作',
      key: 'actions',
      width: 210,
      render: (_: unknown, r: BankReconciliationPeriod) => (
        <Space size={4} wrap>
          <Popconfirm
            title="确认该余额调节表？"
            disabled={r.status === 'confirmed' || Math.abs(r.difference) > AMOUNT_TOLERANCE}
            onConfirm={() => void handleConfirm(r.id)}
          >
            <Button
              size="small"
              type="link"
              disabled={r.status === 'confirmed' || Math.abs(r.difference) > AMOUNT_TOLERANCE}
              loading={confirmingId === r.id}
            >
              确认
            </Button>
          </Popconfirm>
          <Tooltip title={isSensitiveRevealed ? '' : '敏感导出需先在页面中解锁敏感数据'}>
            <Button
              size="small"
              type="link"
              disabled={!isSensitiveRevealed}
              loading={exportingId === r.id}
              onClick={() => void handleExport(r)}
            >
              导出
            </Button>
          </Tooltip>
        </Space>
      ),
    },
  ];

  return (
    <div>
      <div className="mb-16" style={{ display: 'flex', justifyContent: 'space-between', gap: 12, flexWrap: 'wrap' }}>
        <Space wrap>
          <Select
            placeholder="选择资金账户"
            showSearch
            optionFilterProp="label"
            value={accountId}
            onChange={(v) => onAccountChange(v)}
            style={{ width: 220 }}
            options={accounts.map((a) => ({
              value: a.id,
              label: `${a.name}（${a.account_code}）`,
            }))}
          />
          <Button icon={<ReloadOutlined />} loading={loading} onClick={() => void fetchPeriods()}>
            刷新
          </Button>
        </Space>
        <Space wrap>
          <Tooltip title="留空=从当月流水余额列自动推算；导入流水无余额列时可手工录入">
            <InputNumber
              placeholder="对账单期初(可空)"
              min={0}
              value={openingOverride}
              onChange={(v) => setOpeningOverride(v)}
              style={{ width: 160 }}
            />
          </Tooltip>
          <InputNumber
            placeholder="对账单期末(可空)"
            min={0}
            value={closingOverride}
            onChange={(v) => setClosingOverride(v)}
            style={{ width: 160 }}
          />
          <Button type="primary" loading={generating} onClick={handleGenerate}>
            生成{month}调节表
          </Button>
        </Space>
      </div>

      {latestUnbalanced && (
        <Alert
          type="error"
          showIcon
          className="mb-16"
          message={
            <Space wrap>
              <span>
                调节后两侧不平衡，差额{' '}
                <SensitiveText type="amount" value={latest?.difference ?? 0} />
                ，请先处理未核销项或修正对账单余额
              </span>
              <Button size="small" type="primary" danger onClick={onGoWorkbench}>
                去处理未核销项
              </Button>
            </Space>
          }
        />
      )}

      <Card size="small">
        <Table
          rowKey="id"
          columns={columns}
          dataSource={periods}
          pagination={{ pageSize: 10 }}
          scroll={{ x: 1030 }}
        />
      </Card>
    </div>
  );
};

export default BankReconciliationPeriodsPanel;
