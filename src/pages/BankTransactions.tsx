import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Alert,
  Button,
  Card,
  Checkbox,
  Col,
  DatePicker,
  Input,
  InputNumber,
  Modal,
  Popconfirm,
  Row,
  Select,
  Space,
  Spin,
  Statistic,
  Table,
  Tabs,
  Tag,
  Tooltip,
  Typography,
  message,
} from 'antd';
import type { ColumnsType } from 'antd/es/table';
import {
  CheckCircleOutlined,
  DisconnectOutlined,
  FileAddOutlined,
  FileDoneOutlined,
  ImportOutlined,
  ReloadOutlined,
  SearchOutlined,
  StopOutlined,
} from '@ant-design/icons';
import { open, save } from '@tauri-apps/plugin-dialog';
import {
  batchConfirmBankAutoMatches,
  cancelBankAllocation,
  cancelBankTransactionMatch,
  confirmBankAllocations,
  confirmBankReconciliationPeriod,
  createBankManualVoucher,
  exportBankReconciliationPeriod,
  generateBankReconciliationPeriod,
  getFundAccounts,
  getGlAccounts,
  ignoreBankTransaction,
  importBankTransactionsFile,
  listBankAllocations,
  listBankReconciliationPeriods,
  migrateLegacyBankMatches,
  previewBankAllocationCandidates,
  previewBankTransactionImport,
  queryBankTransactions,
} from '@/api';
import SensitiveText from '@/components/SensitiveText';
import { SensitiveStatistic } from '@/components/SensitiveStatistic';
import { useBusinessMonth } from '@/contexts/BusinessMonthContext';
import { useSecurity } from '@/contexts/SecurityContext';
import type {
  BankAllocationBatchResult,
  BankAllocationCandidate,
  BankAllocationInput,
  BankAutoMatchPreviewItem,
  BankImportPreview,
  BankImportPreviewRow,
  BankReconciliationAllocation,
  BankReconciliationPeriod,
  BankTransaction,
  BankTransactionStatus,
  FundAccount,
  GlAccount,
  LegacyBankMatchReport,
} from '@/types';

const statusMeta: Record<BankTransactionStatus, { text: string; color: string }> = {
  unmatched: { text: '待匹配', color: 'gold' },
  matched: { text: '已匹配', color: 'green' },
  ignored: { text: '已忽略', color: 'default' },
};

const previewStatusMeta: Record<string, { text: string; color: string }> = {
  ok: { text: '可导入', color: 'green' },
  duplicate: { text: '重复', color: 'blue' },
  warning: { text: '余额存疑', color: 'orange' },
  error: { text: '异常', color: 'red' },
};

const AMOUNT_TOLERANCE = 0.005;

const fmtMoney = (value?: number | null) =>
  (value ?? 0).toLocaleString('zh-CN', { minimumFractionDigits: 2, maximumFractionDigits: 2 });

const txSideAmount = (tx: Pick<BankTransaction, 'income_amount' | 'expense_amount'>) =>
  tx.income_amount > tx.expense_amount ? tx.income_amount : tx.expense_amount;

/** 核销对：一条流水 × 一条候选分录 + 拟核销金额（对账工作台行） */
interface PairDraft {
  key: string;
  transaction_id: number;
  transaction_date: string;
  tx_summary: string;
  tx_remaining: number;
  candidate: BankAllocationCandidate;
  amount: number;
  include: boolean;
}

// ==================== Tab 2：对账工作台（双栏：左流水 / 右候选分录） ====================

const ReconciliationWorkbench: React.FC<{
  month: string;
  accounts: FundAccount[];
  accountId?: number;
  onAccountChange: (id?: number) => void;
}> = ({ month, accounts, accountId, onAccountChange }) => {
  const [txs, setTxs] = useState<BankTransaction[]>([]);
  const [allocations, setAllocations] = useState<BankReconciliationAllocation[]>([]);
  const [loading, setLoading] = useState(false);
  const [selectedTxIds, setSelectedTxIds] = useState<React.Key[]>([]);
  const [pairs, setPairs] = useState<PairDraft[]>([]);
  const [confirming, setConfirming] = useState(false);
  const [autoMinScore, setAutoMinScore] = useState<number>(60);
  const [autoRunning, setAutoRunning] = useState(false);
  const [migrationOpen, setMigrationOpen] = useState(false);
  const [migrationReport, setMigrationReport] = useState<LegacyBankMatchReport | null>(null);
  const [migrationLoading, setMigrationLoading] = useState(false);

  const fetchAll = useCallback(async () => {
    setLoading(true);
    try {
      const [txData, allocData] = await Promise.all([
        queryBankTransactions({
          belong_month: month,
          fund_account_id: accountId,
        }),
        listBankAllocations({ belong_month: month, status: 'active' }),
      ]);
      setTxs(txData);
      setAllocations(allocData);
    } catch (e: unknown) {
      message.error('查询对账数据失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setLoading(false);
    }
  }, [accountId, month]);

  useEffect(() => {
    void fetchAll();
    setSelectedTxIds([]);
    setPairs([]);
  }, [fetchAll]);

  // 勾选流水 → 拉取候选并生成核销对草稿（最优候选默认勾选、金额取两侧剩余较小值）
  const handleSelectionChange = async (keys: React.Key[]) => {
    setSelectedTxIds(keys);
    if (keys.length === 0) {
      setPairs([]);
      return;
    }
    setLoading(true);
    try {
      const previews = await Promise.all(
        keys.map((k) => previewBankAllocationCandidates(Number(k))),
      );
      const next: PairDraft[] = [];
      previews.forEach((item: BankAutoMatchPreviewItem) => {
        const tx = txs.find((t) => t.id === item.transaction_id);
        const txRemaining = tx?.remaining_amount ?? item.remaining_amount;
        item.candidates.forEach((candidate, candIdx) => {
          const best = candIdx === 0;
          next.push({
            key: `${item.transaction_id}-${candidate.voucher_line_id}`,
            transaction_id: item.transaction_id,
            transaction_date: item.transaction_date,
            tx_summary: item.summary || item.counterparty_name || `流水ID=${item.transaction_id}`,
            tx_remaining: txRemaining,
            candidate,
            amount: best
              ? Number(Math.min(txRemaining, candidate.remaining_amount).toFixed(2))
              : 0,
            include: best,
          });
        });
      });
      setPairs(next);
    } catch (e: unknown) {
      setPairs([]);
      message.error('获取核销候选失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setLoading(false);
    }
  };

  const includedPairs = pairs.filter((p) => p.include && p.amount > AMOUNT_TOLERANCE);
  const includedTotal = includedPairs.reduce((sum, p) => sum + p.amount, 0);

  const handleConfirmPairs = async () => {
    if (includedPairs.length === 0) {
      message.warning('请先勾选核销对并填写核销金额');
      return;
    }
    const items: BankAllocationInput[] = includedPairs.map((p) => ({
      transaction_id: p.transaction_id,
      voucher_line_id: p.candidate.voucher_line_id,
      allocated_amount: p.amount,
      score: p.candidate.score,
      remark: `对账工作台核销（评分 ${p.candidate.score}）`,
    }));
    setConfirming(true);
    try {
      const result: BankAllocationBatchResult = await confirmBankAllocations(items, 'manual');
      if (result.errors.length > 0) {
        message.warning(
          `核销完成：成功 ${result.confirmed} 条，跳过 ${result.skipped} 条；失败：${result.errors.join('；')}`,
        );
      } else {
        message.success(`核销完成：成功 ${result.confirmed} 条`);
      }
      setSelectedTxIds([]);
      setPairs([]);
      await fetchAll();
    } catch (e: unknown) {
      message.error('核销失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setConfirming(false);
    }
  };

  const handleAutoMatch = async () => {
    setAutoRunning(true);
    try {
      const result = await batchConfirmBankAutoMatches(month, autoMinScore);
      if (result.errors.length > 0) {
        message.warning(
          `自动匹配完成：确认 ${result.confirmed} 条，跳过 ${result.skipped} 条（置信线 ${autoMinScore}）`,
        );
      } else {
        message.success(
          `自动匹配完成：确认 ${result.confirmed} 条，跳过 ${result.skipped} 条（置信线 ${autoMinScore}）`,
        );
      }
      await fetchAll();
    } catch (e: unknown) {
      message.error('自动匹配失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setAutoRunning(false);
    }
  };

  const handleRunMigration = async () => {
    setMigrationLoading(true);
    try {
      const report = await migrateLegacyBankMatches();
      setMigrationReport(report);
      setMigrationOpen(true);
    } catch (e: unknown) {
      message.error('迁移失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setMigrationLoading(false);
    }
  };

  const handleCancelAllocation = async (id: number) => {
    try {
      await cancelBankAllocation(id);
      message.success('核销已取消');
      await fetchAll();
    } catch (e: unknown) {
      message.error('取消核销失败: ' + (e instanceof Error ? e.message : String(e)));
    }
  };

  const txColumns: ColumnsType<BankTransaction> = [
    { title: '日期', dataIndex: 'transaction_date', key: 'transaction_date', width: 100 },
    {
      title: '摘要/对方',
      key: 'summary',
      width: 170,
      ellipsis: true,
      render: (_: unknown, tx: BankTransaction) =>
        tx.summary || tx.counterparty_name || `流水ID=${tx.id}`,
    },
    {
      title: '收入',
      dataIndex: 'income_amount',
      key: 'income_amount',
      width: 100,
      align: 'right',
      render: (v: number) => (v > 0 ? <SensitiveText type="amount" value={v} /> : '-'),
    },
    {
      title: '支出',
      dataIndex: 'expense_amount',
      key: 'expense_amount',
      width: 100,
      align: 'right',
      render: (v: number) => (v > 0 ? <SensitiveText type="amount" value={v} /> : '-'),
    },
    {
      title: '未核销',
      dataIndex: 'remaining_amount',
      key: 'remaining_amount',
      width: 100,
      align: 'right',
      render: (_: unknown, tx: BankTransaction) => {
        const remaining = tx.remaining_amount ?? txSideAmount(tx);
        if (tx.status === 'ignored') return <Tag>已忽略</Tag>;
        return remaining > AMOUNT_TOLERANCE ? (
          <span style={{ color: '#d46b08' }}>
            <SensitiveText type="amount" value={remaining} />
          </span>
        ) : (
          <Tag color="green">已核销</Tag>
        );
      },
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
          <Button icon={<ReloadOutlined />} loading={loading} onClick={() => void fetchAll()}>
            刷新
          </Button>
        </Space>
        <Space wrap>
          <InputNumber
            min={1}
            max={100}
            value={autoMinScore}
            onChange={(v) => setAutoMinScore(Number(v ?? 60))}
            addonBefore="置信线"
            style={{ width: 150 }}
          />
          <Button loading={autoRunning} onClick={handleAutoMatch}>
            自动匹配（高置信全额）
          </Button>
          <Button loading={migrationLoading} onClick={handleRunMigration}>
            旧匹配迁移报告
          </Button>
        </Space>
      </div>

      <Row gutter={[16, 16]}>
        <Col xs={24} lg={11}>
          <Card title="银行流水（勾选待核销流水）" size="small">
            <Table
              rowKey="id"
              size="small"
              columns={txColumns}
              dataSource={txs}
              pagination={{ pageSize: 10 }}
              scroll={{ y: 420 }}
              rowSelection={{
                selectedRowKeys: selectedTxIds,
                onChange: (keys) => void handleSelectionChange(keys),
                getCheckboxProps: (tx: BankTransaction) => ({
                  disabled:
                    tx.status === 'ignored' ||
                    !tx.fund_account_id ||
                    (tx.remaining_amount ?? txSideAmount(tx)) <= AMOUNT_TOLERANCE,
                }),
              }}
            />
          </Card>
        </Col>
        <Col xs={24} lg={13}>
          <Card
            title="账面分录候选（评分与未核销余额）"
            size="small"
            extra={
              <span>
                已勾选 <b>{includedPairs.length}</b> 对，核销合计{' '}
                <SensitiveText type="amount" value={includedTotal} />
              </span>
            }
          >
            {pairs.length === 0 ? (
              <div style={{ color: '#8c8c8c', padding: '48px 0', textAlign: 'center' }}>
                在左侧勾选流水后，这里按评分展示可核销的账面分录
              </div>
            ) : (
              <div style={{ maxHeight: 460, overflowY: 'auto' }}>
                {pairs.map((p) => (
                  <div
                    key={p.key}
                    style={{
                      display: 'flex',
                      alignItems: 'center',
                      gap: 8,
                      padding: '6px 4px',
                      borderBottom: '1px solid #f0f0f0',
                      flexWrap: 'wrap',
                    }}
                  >
                    <Checkbox
                      checked={p.include}
                      onChange={(e) =>
                        setPairs((prev) =>
                          prev.map((item) =>
                            item.key === p.key ? { ...item, include: e.target.checked } : item,
                          ),
                        )
                      }
                    />
                    <div style={{ flex: '1 1 260px', minWidth: 0 }}>
                      <div style={{ fontSize: 13 }}>
                        {p.transaction_date} {p.tx_summary}
                      </div>
                      <div style={{ fontSize: 12, color: '#8c8c8c' }}>
                        {p.candidate.voucher_no}（{p.candidate.voucher_date}）{' '}
                        {p.candidate.line_summary || p.candidate.account_code} · 分录剩余{' '}
                        <SensitiveText type="amount" value={p.candidate.remaining_amount} />
                      </div>
                    </div>
                    <Tag
                      color={p.candidate.score >= 80 ? 'green' : p.candidate.score >= 60 ? 'gold' : 'default'}
                    >
                      评分 {p.candidate.score}
                    </Tag>
                    <Tooltip title={p.candidate.score_reasons.join('；')}>
                      <span style={{ color: '#8c8c8c', fontSize: 12 }}>因子</span>
                    </Tooltip>
                    <InputNumber
                      size="small"
                      min={0}
                      max={Math.min(p.tx_remaining, p.candidate.remaining_amount)}
                      value={p.amount}
                      onChange={(v) =>
                        setPairs((prev) =>
                          prev.map((item) =>
                            item.key === p.key ? { ...item, amount: Number(v ?? 0) } : item,
                          ),
                        )
                      }
                      style={{ width: 120 }}
                    />
                  </div>
                ))}
              </div>
            )}
            <div style={{ marginTop: 12, textAlign: 'right' }}>
              <Button
                type="primary"
                icon={<CheckCircleOutlined />}
                disabled={includedPairs.length === 0}
                loading={confirming}
                onClick={handleConfirmPairs}
              >
                确认核销
              </Button>
            </div>
          </Card>
        </Col>
      </Row>

      <Card title="本月核销明细" size="small" className="mt-16">
        <Table
          rowKey="id"
          size="small"
          dataSource={allocations}
          pagination={{ pageSize: 8 }}
          columns={[
            { title: '流水ID', dataIndex: 'transaction_id', key: 'transaction_id', width: 80 },
            { title: '流水日期', dataIndex: 'voucher_date', key: 'voucher_date', width: 110, render: (_: unknown, r: BankReconciliationAllocation) => r.voucher_date },
            { title: '凭证号', dataIndex: 'voucher_no', key: 'voucher_no', width: 130 },
            {
              title: '核销金额',
              dataIndex: 'allocated_amount',
              key: 'allocated_amount',
              width: 120,
              align: 'right',
              render: (v: number) => <SensitiveText type="amount" value={v} />,
            },
            {
              title: '方式',
              dataIndex: 'match_method',
              key: 'match_method',
              width: 90,
              render: (m: string) =>
                m === 'auto' ? '自动' : m === 'migrated' ? '迁移' : '人工',
            },
            { title: '操作人', dataIndex: 'operator_name', key: 'operator_name', width: 100 },
            {
              title: '操作',
              key: 'actions',
              width: 90,
              render: (_: unknown, r: BankReconciliationAllocation) => (
                <Popconfirm
                  title="确认取消该核销？"
                  onConfirm={() => void handleCancelAllocation(r.id)}
                >
                  <Button size="small" icon={<DisconnectOutlined />}>
                    取消
                  </Button>
                </Popconfirm>
              ),
            },
          ]}
        />
      </Card>

      <Modal
        title="旧银行匹配迁移报告"
        open={migrationOpen}
        footer={[
          <Button key="close" onClick={() => setMigrationOpen(false)}>
            关闭
          </Button>,
        ]}
        onCancel={() => setMigrationOpen(false)}
      >
        {migrationReport && (
          <Space direction="vertical" style={{ width: '100%' }} size={8}>
            <div>
              旧匹配共 {migrationReport.total} 行（active {migrationReport.active_total}），
              已迁入核销 {migrationReport.migrated} 条，幂等跳过 {migrationReport.already_migrated} 条。
            </div>
            {migrationReport.unconverted.length > 0 ? (
              <>
                <Typography.Text type="warning">
                  以下 {migrationReport.unconverted.length} 条无法唯一定位付款凭证资金行，未迁移（不静默丢失）：
                </Typography.Text>
                <Table
                  size="small"
                  rowKey="match_id"
                  dataSource={migrationReport.unconverted}
                  pagination={{ pageSize: 6 }}
                  columns={[
                    { title: '旧匹配ID', dataIndex: 'match_id', key: 'match_id', width: 90 },
                    { title: '流水ID', dataIndex: 'transaction_id', key: 'transaction_id', width: 80 },
                    { title: '批次ID', dataIndex: 'payment_batch_id', key: 'payment_batch_id', width: 80 },
                    { title: '原因', dataIndex: 'reason', key: 'reason' },
                  ]}
                />
              </>
            ) : (
              <Typography.Text type="secondary">全部旧匹配均已转换或取消，无遗留项。</Typography.Text>
            )}
          </Space>
        )}
      </Modal>
    </div>
  );
};

// ==================== Tab 3：余额调节表 ====================

const ReconciliationPeriodsPanel: React.FC<{
  month: string;
  accounts: FundAccount[];
  accountId?: number;
  onAccountChange: (id?: number) => void;
  onGoWorkbench: () => void;
}> = ({ month, accounts, accountId, onAccountChange, onGoWorkbench }) => {
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

// ==================== 主页面 ====================

const BankTransactions: React.FC = () => {
  const { month, setMonth } = useBusinessMonth();
  const [activeTab, setActiveTab] = useState('list');
  const [statusFilter, setStatusFilter] = useState<BankTransactionStatus | undefined>(undefined);
  const [accountFilter, setAccountFilter] = useState<number | undefined>(undefined);
  const [keyword, setKeyword] = useState('');
  const [transactions, setTransactions] = useState<BankTransaction[]>([]);
  const [loading, setLoading] = useState(false);
  const [action, setAction] = useState<string | null>(null);
  const [ignoreTx, setIgnoreTx] = useState<BankTransaction | null>(null);
  const [ignoreReason, setIgnoreReason] = useState('');
  const [voucherTx, setVoucherTx] = useState<BankTransaction | null>(null);
  const [voucherAccounts, setVoucherAccounts] = useState<GlAccount[]>([]);
  const [voucherAccountCode, setVoucherAccountCode] = useState<string | undefined>(undefined);
  const [fundAccounts, setFundAccounts] = useState<FundAccount[]>([]);
  const [fundAccountId, setFundAccountId] = useState<number | undefined>(undefined);
  const [voucherSummary, setVoucherSummary] = useState('');
  // 导入预览（Task 11）：选账户 → 选文件解析预览 → 确认入库
  const [importOpen, setImportOpen] = useState(false);
  const [importAccountId, setImportAccountId] = useState<number | undefined>(undefined);
  const [importFile, setImportFile] = useState<string | null>(null);
  const [importPreview, setImportPreview] = useState<BankImportPreview | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);

  const fetchData = useCallback(async () => {
    setLoading(true);
    try {
      const belongMonth = month.format('YYYY-MM');
      const txData = await queryBankTransactions({
        belong_month: belongMonth,
        status: statusFilter,
        keyword: keyword.trim() || undefined,
        fund_account_id: accountFilter,
      });
      setTransactions(txData);
    } catch (e: unknown) {
      message.error('查询银行流水失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setLoading(false);
    }
  }, [accountFilter, keyword, month, statusFilter]);

  useEffect(() => {
    fetchData();
  }, [fetchData]);

  // 银行/第三方支付账户：导入目标与凭证资金行共用（现金账户不可导入银行流水）
  useEffect(() => {
    getFundAccounts({ is_active: true })
      .then((accounts) =>
        setFundAccounts(accounts.filter((a) => ['bank', 'third_party'].includes(a.account_type))),
      )
      .catch((e: unknown) => message.error('获取资金账户失败: ' + (e instanceof Error ? e.message : String(e))));
  }, []);

  const summary = useMemo(() => ({
    total: transactions.length,
    unmatched: transactions.filter((item) => item.status === 'unmatched').length,
    matched: transactions.filter((item) => item.status === 'matched').length,
    expenseAmount: transactions.reduce((sum, item) => sum + item.expense_amount, 0),
  }), [transactions]);

  // 导入流水三步（spec 4.8）：必选 bank/third_party 账户 → 文件解析预览（不落库）→ 确认导入
  const openImportModal = () => {
    setImportAccountId(undefined);
    setImportFile(null);
    setImportPreview(null);
    setImportOpen(true);
  };

  const runImportPreview = useCallback(async (filePath: string, accountId: number) => {
    setPreviewLoading(true);
    try {
      const preview = await previewBankTransactionImport(filePath, accountId);
      setImportPreview(preview);
    } catch (e: unknown) {
      setImportPreview(null);
      message.error('解析流水文件失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setPreviewLoading(false);
    }
  }, []);

  const handleSelectImportFile = async () => {
    if (!importAccountId) {
      message.warning('请先选择资金账户');
      return;
    }
    const selected = await open({
      filters: [{ name: '银行流水', extensions: ['xlsx', 'xls', 'csv'] }],
      multiple: false,
    });
    if (!selected) return;
    const filePath = String(selected);
    setImportFile(filePath);
    await runImportPreview(filePath, importAccountId);
  };

  const handleImportAccountChange = (accountId: number) => {
    setImportAccountId(accountId);
    // 账户维度参与去重判定，切账户后旧预览失效
    setImportPreview(null);
    if (importFile) {
      void runImportPreview(importFile, accountId);
    }
  };

  const handleConfirmImport = async () => {
    if (!importAccountId || !importFile || !importPreview) {
      message.warning('请选择账户并解析文件');
      return;
    }
    if (importPreview.error_rows > 0) {
      message.error('存在异常行（如已月结月份），请先处理后再导入');
      return;
    }
    setAction('import');
    try {
      const result = await importBankTransactionsFile(importFile, importAccountId);
      if (result.errors.length > 0) {
        message.warning(`导入完成：成功 ${result.imported} 条，跳过 ${result.failed} 条（重复或异常）`);
      } else {
        message.success(`导入完成：成功 ${result.imported} 条`);
      }
      setImportOpen(false);
      await fetchData();
    } catch (e: unknown) {
      message.error('导入银行流水失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setAction(null);
    }
  };

  const openIgnoreModal = (tx: BankTransaction) => {
    setIgnoreReason('');
    setIgnoreTx(tx);
  };

  const handleIgnore = async () => {
    if (!ignoreTx || !ignoreReason.trim()) {
      message.warning('请输入忽略原因');
      return;
    }
    setAction(`ignore-${ignoreTx.id}`);
    try {
      await ignoreBankTransaction({ transaction_id: ignoreTx.id, reason: ignoreReason });
      message.success('流水已忽略');
      setIgnoreTx(null);
      await fetchData();
    } catch (e: unknown) {
      message.error('忽略失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setAction(null);
    }
  };

  // 取消旧式批次匹配（旧 bank_transaction_matches 只读保留一个版本周期；
  // 新引擎核销走「对账工作台」，取消核销在该页完成）
  const handleCancelMatch = async (tx: BankTransaction) => {
    setAction(`cancel-${tx.id}`);
    try {
      await cancelBankTransactionMatch(tx.id);
      message.success('匹配已取消');
      await fetchData();
    } catch (e: unknown) {
      message.error('取消匹配失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setAction(null);
    }
  };

  // 生成凭证弹窗：仅未匹配且未忽略的流水（后端同口径校验）。
  // 支出流水选借方科目（贷方为所选资金账户科目），收入流水选贷方科目（借方为资金账户科目）。
  // 资金账户必选（spec 4.7）；已账户化流水锁定为其归属账户（spec 4.8/6.3 同账户硬条件）。
  const isExpenseTx = (tx: BankTransaction | null): boolean => (tx?.expense_amount ?? 0) > 0;

  const openVoucherModal = async (tx: BankTransaction) => {
    setVoucherAccountCode(undefined);
    setFundAccountId(tx.fund_account_id ?? undefined);
    setVoucherSummary(tx.summary || '');
    setVoucherTx(tx);
    if (voucherAccounts.length === 0) {
      try {
        setVoucherAccounts((await getGlAccounts()).filter((a) => a.is_active === 1));
      } catch (e: unknown) {
        message.error('获取科目列表失败: ' + (e instanceof Error ? e.message : String(e)));
      }
    }
  };

  const handleCreateVoucher = async () => {
    if (!voucherTx || !voucherAccountCode) {
      message.warning('请选择科目');
      return;
    }
    if (!fundAccountId) {
      message.warning('请选择资金账户');
      return;
    }
    setAction(`voucher-${voucherTx.id}`);
    try {
      const voucher = await createBankManualVoucher(
        voucherTx.id,
        voucherAccountCode,
        fundAccountId,
        voucherSummary.trim() || undefined,
      );
      message.success(`凭证已生成：${voucher.voucher_no}`);
      setVoucherTx(null);
      await fetchData();
    } catch (e: unknown) {
      message.error('生成凭证失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setAction(null);
    }
  };

  const columns = [
    { title: '交易日期', dataIndex: 'transaction_date', key: 'transaction_date', width: 110, fixed: 'left' as const },
    {
      title: '状态',
      dataIndex: 'status',
      key: 'status',
      width: 90,
      render: (status: BankTransactionStatus) => <Tag color={statusMeta[status].color}>{statusMeta[status].text}</Tag>,
    },
    {
      title: '资金账户',
      dataIndex: 'fund_account_name',
      key: 'fund_account',
      width: 140,
      ellipsis: true,
      render: (_: unknown, tx: BankTransaction) =>
        tx.fund_account_name ? (
          tx.fund_account_name
        ) : (
          <Tooltip title="历史流水未归集账户，不参与自动对账">
            <Tag color="orange">待归集</Tag>
          </Tooltip>
        ),
    },
    { title: '摘要', dataIndex: 'summary', key: 'summary', width: 180, ellipsis: true },
    { title: '对方户名', dataIndex: 'counterparty_name', key: 'counterparty_name', width: 140, ellipsis: true },
    {
      title: '对方账号',
      dataIndex: 'counterparty_account',
      key: 'counterparty_account',
      width: 200,
      ellipsis: true,
      render: (value: string) => <SensitiveText type="bank_card" value={value} />,
    },
    {
      title: '收入',
      dataIndex: 'income_amount',
      key: 'income_amount',
      width: 140,
      align: 'right' as const,
      render: (value: number) => value > 0 ? <SensitiveText type="amount" value={value} /> : '-',
    },
    {
      title: '支出',
      dataIndex: 'expense_amount',
      key: 'expense_amount',
      width: 140,
      align: 'right' as const,
      render: (value: number) => value > 0 ? <SensitiveText type="amount" value={value} /> : '-',
    },
    {
      title: '已核销/未核销',
      key: 'allocation',
      width: 180,
      render: (_: unknown, tx: BankTransaction) => {
        const remaining = tx.remaining_amount ?? txSideAmount(tx);
        return (
          <Space size={4}>
            <SensitiveText type="amount" value={tx.allocated_amount ?? 0} />
            <span style={{ color: '#8c8c8c' }}>/</span>
            {remaining > AMOUNT_TOLERANCE ? (
              <SensitiveText type="amount" value={remaining} />
            ) : (
              <Tag color="green">清</Tag>
            )}
          </Space>
        );
      },
    },
    {
      title: '匹配批次',
      dataIndex: 'matched_batch_no',
      key: 'matched_batch_no',
      width: 200,
      render: (_: unknown, tx: BankTransaction) =>
        tx.matched_batch_no ? (
          <Space size={4}>
            <span>{tx.matched_batch_no}</span>
            <span style={{ color: '#8c8c8c' }}>/</span>
            <SensitiveText type="amount" value={tx.matched_amount ?? 0} />
          </Space>
        ) : (tx.ignore_reason || '-'),
    },
    {
      title: '操作',
      key: 'actions',
      width: 240,
      fixed: 'right' as const,
      render: (_: unknown, tx: BankTransaction) => (
        <Space size={6} wrap>
          <Button
            size="small"
            icon={<FileDoneOutlined />}
            disabled={tx.status !== 'unmatched' || !!tx.ignore_reason}
            loading={action === `voucher-${tx.id}`}
            onClick={() => openVoucherModal(tx)}
          >
            生成凭证
          </Button>
          <Button
            size="small"
            icon={<StopOutlined />}
            disabled={tx.status !== 'unmatched'}
            loading={action === `ignore-${tx.id}`}
            onClick={() => openIgnoreModal(tx)}
          >
            忽略
          </Button>
          <Popconfirm
            title="确认取消该流水的旧式批次匹配?"
            okText="取消匹配"
            cancelText="关闭"
            onConfirm={() => handleCancelMatch(tx)}
          >
            <Button
              size="small"
              icon={<DisconnectOutlined />}
              disabled={tx.status !== 'matched'}
              loading={action === `cancel-${tx.id}`}
            >
              取消
            </Button>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <div>
      <div className="page-header">
        <span className="page-title">银行对账</span>
        <div className="page-header-actions">
          <DatePicker
            picker="month"
            value={month}
            allowClear={false}
            onChange={(value) => value && setMonth(value)}
            style={{ width: 150 }}
          />
        </div>
      </div>

      <Tabs
        activeKey={activeTab}
        onChange={setActiveTab}
        items={[
          {
            key: 'list',
            label: '银行流水',
            children: (
              <>
                <div className="mb-16" style={{ display: 'flex', justifyContent: 'flex-end', flexWrap: 'wrap', gap: 8 }}>
                  <Select
                    placeholder="状态"
                    allowClear
                    value={statusFilter}
                    onChange={setStatusFilter}
                    style={{ width: 120 }}
                    options={[
                      { value: 'unmatched', label: '待匹配' },
                      { value: 'matched', label: '已匹配' },
                      { value: 'ignored', label: '已忽略' },
                    ]}
                  />
                  <Select
                    placeholder="资金账户"
                    allowClear
                    showSearch
                    optionFilterProp="label"
                    value={accountFilter}
                    onChange={(value) => setAccountFilter(value)}
                    style={{ width: 180 }}
                    options={fundAccounts.map((a) => ({
                      value: a.id,
                      label: `${a.name}（${a.account_code}）`,
                    }))}
                  />
                  <Input
                    allowClear
                    prefix={<SearchOutlined />}
                    placeholder="摘要/户名/账号/批次"
                    value={keyword}
                    onChange={(event) => setKeyword(event.target.value)}
                    style={{ width: 220 }}
                  />
                  <Button icon={<ReloadOutlined />} loading={loading} onClick={fetchData}>
                    刷新
                  </Button>
                  <Button icon={<ImportOutlined />} loading={action === 'import'} onClick={openImportModal}>
                    导入流水
                  </Button>
                </div>

                <Spin spinning={loading}>
                  <Row gutter={[16, 16]} className="mb-16">
                    <Col xs={24} sm={12} lg={6}>
                      <Card className="stat-card"><Statistic title="流水数" value={summary.total} /></Card>
                    </Col>
                    <Col xs={24} sm={12} lg={6}>
                      <Card className="stat-card"><Statistic title="待匹配" value={summary.unmatched} /></Card>
                    </Col>
                    <Col xs={24} sm={12} lg={6}>
                      <Card className="stat-card"><Statistic title="已匹配" value={summary.matched} /></Card>
                    </Col>
                    <Col xs={24} sm={12} lg={6}>
                      <Card className="stat-card"><SensitiveStatistic title="支出金额" value={summary.expenseAmount} /></Card>
                    </Col>
                  </Row>

                  <Card>
                    <Table
                      rowKey="id"
                      columns={columns}
                      dataSource={transactions}
                      pagination={{ pageSize: 12, showSizeChanger: true }}
                      scroll={{ x: 1660 }}
                    />
                  </Card>
                </Spin>
              </>
            ),
          },
          {
            key: 'workbench',
            label: '对账工作台',
            children: (
              <ReconciliationWorkbench
                month={month.format('YYYY-MM')}
                accounts={fundAccounts}
                accountId={accountFilter}
                onAccountChange={setAccountFilter}
              />
            ),
          },
          {
            key: 'periods',
            label: '余额调节表',
            children: (
              <ReconciliationPeriodsPanel
                month={month.format('YYYY-MM')}
                accounts={fundAccounts}
                accountId={accountFilter}
                onAccountChange={setAccountFilter}
                onGoWorkbench={() => setActiveTab('workbench')}
              />
            ),
          },
        ]}
      />

      <Modal
        title="忽略银行流水"
        open={!!ignoreTx}
        okText="忽略"
        cancelText="取消"
        confirmLoading={!!ignoreTx && action === `ignore-${ignoreTx.id}`}
        onOk={handleIgnore}
        onCancel={() => setIgnoreTx(null)}
      >
        <Input.TextArea
          rows={3}
          placeholder="例如：非工资报销付款、内部调账"
          value={ignoreReason}
          onChange={(event) => setIgnoreReason(event.target.value)}
        />
      </Modal>

      <Modal
        title="流水生成凭证"
        open={!!voucherTx}
        okText="生成凭证"
        cancelText="取消"
        confirmLoading={!!voucherTx && action === `voucher-${voucherTx.id}`}
        onOk={handleCreateVoucher}
        onCancel={() => setVoucherTx(null)}
      >
        {voucherTx && (
          <Space direction="vertical" style={{ width: '100%' }} size={12}>
            <div>
              <div>
                {voucherTx.transaction_date} /{' '}
                {isExpenseTx(voucherTx) ? '支出' : '收入'}{' '}
                <SensitiveText
                  type="amount"
                  value={fmtMoney(isExpenseTx(voucherTx) ? voucherTx.expense_amount : voucherTx.income_amount)}
                />
              </div>
              <div>{voucherTx.summary || voucherTx.counterparty_name || '-'}</div>
            </div>
            <div style={{ color: '#8c8c8c' }}>
              {isExpenseTx(voucherTx)
                ? '支出流水：借所选科目，贷所选资金账户科目（如手续费选 6603 财务费用）'
                : '收入流水：借所选资金账户科目，贷所选科目（如利息收入选 6603 财务费用）'}
            </div>
            <Select
              showSearch
              optionFilterProp="label"
              placeholder="选择资金账户（资金行辅助核算）"
              value={fundAccountId}
              onChange={setFundAccountId}
              disabled={!!voucherTx?.fund_account_id}
              style={{ width: '100%' }}
              options={fundAccounts.map((a) => ({
                value: a.id,
                label: `${a.name}（${a.account_code}）`,
              }))}
            />
            {voucherTx?.fund_account_id && (
              <div style={{ color: '#8c8c8c' }}>已账户化流水必须使用其归属账户入账</div>
            )}
            <Select
              showSearch
              optionFilterProp="label"
              placeholder={isExpenseTx(voucherTx) ? '选择借方科目' : '选择贷方科目'}
              value={voucherAccountCode}
              onChange={setVoucherAccountCode}
              style={{ width: '100%' }}
              options={voucherAccounts.map((a) => ({
                value: a.code,
                label: `${a.code} ${a.name}`,
              }))}
            />
            <Input
              placeholder="摘要（可选）"
              value={voucherSummary}
              onChange={(event) => setVoucherSummary(event.target.value)}
            />
          </Space>
        )}
      </Modal>

      <Modal
        title="导入银行流水"
        open={importOpen}
        width={880}
        okText="确认导入"
        cancelText="取消"
        okButtonProps={{
          disabled: !importPreview || importPreview.error_rows > 0,
          icon: <FileAddOutlined />,
        }}
        confirmLoading={action === 'import'}
        onOk={handleConfirmImport}
        onCancel={() => setImportOpen(false)}
      >
        <Space direction="vertical" style={{ width: '100%' }} size={12}>
          <Space wrap>
            <Select
              showSearch
              optionFilterProp="label"
              placeholder="选择资金账户（必选）"
              value={importAccountId}
              onChange={handleImportAccountChange}
              style={{ width: 260 }}
              options={fundAccounts.map((a) => ({
                value: a.id,
                label: `${a.name}（${a.account_code}）`,
              }))}
            />
            <Button
              icon={<FileAddOutlined />}
              disabled={!importAccountId}
              loading={previewLoading}
              onClick={handleSelectImportFile}
            >
              {importFile ? '重新选择文件' : '选择流水文件'}
            </Button>
            {importFile && (
              <span style={{ color: '#8c8c8c' }} title={importFile}>
                {importFile.split(/[\\/]/).pop()}
              </span>
            )}
          </Space>
          {importPreview && (
            <>
              <Alert
                type={importPreview.error_rows > 0 ? 'error' : 'info'}
                showIcon
                message={`共 ${importPreview.total_rows} 行：可导入 ${importPreview.ok_rows}、重复 ${importPreview.duplicate_rows}（导入时跳过）、余额存疑 ${importPreview.warning_rows}、异常 ${importPreview.error_rows}${importPreview.error_rows > 0 ? '（处理后才能导入）' : ''}`}
                description={`收入合计 ¥${fmtMoney(importPreview.income_total)}，支出合计 ¥${fmtMoney(importPreview.expense_total)}，归属账户：${importPreview.fund_account_name}。确认后才会写入数据库。`}
              />
              <Table
                size="small"
                rowKey="row_no"
                dataSource={importPreview.rows}
                pagination={importPreview.rows.length > 10 ? { pageSize: 10 } : false}
                scroll={{ y: 320 }}
                columns={[
                  { title: '行号', dataIndex: 'row_no', key: 'row_no', width: 56 },
                  { title: '交易日期', dataIndex: 'transaction_date', key: 'transaction_date', width: 100 },
                  { title: '摘要', dataIndex: 'summary', key: 'summary', width: 140, ellipsis: true },
                  { title: '对方户名', dataIndex: 'counterparty_name', key: 'counterparty_name', width: 110, ellipsis: true },
                  {
                    title: '方向',
                    dataIndex: 'direction',
                    key: 'direction',
                    width: 64,
                    render: (d: string) => (d === 'income' ? '收入' : d === 'expense' ? '支出' : '异常'),
                  },
                  {
                    title: '收入',
                    dataIndex: 'income_amount',
                    key: 'income_amount',
                    width: 100,
                    align: 'right' as const,
                    render: (v: number) => (v > 0 ? <SensitiveText type="amount" value={v} /> : '-'),
                  },
                  {
                    title: '支出',
                    dataIndex: 'expense_amount',
                    key: 'expense_amount',
                    width: 100,
                    align: 'right' as const,
                    render: (v: number) => (v > 0 ? <SensitiveText type="amount" value={v} /> : '-'),
                  },
                  {
                    title: '余额',
                    dataIndex: 'balance',
                    key: 'balance',
                    width: 110,
                    align: 'right' as const,
                    render: (v?: number) => <SensitiveText type="amount" value={v ?? 0} />,
                  },
                  {
                    title: '检查结果',
                    key: 'row_status',
                    width: 240,
                    render: (_: unknown, row: BankImportPreviewRow) => {
                      const meta = previewStatusMeta[row.row_status] ?? previewStatusMeta.error;
                      return (
                        <Space size={6}>
                          <Tag color={meta.color}>{meta.text}</Tag>
                          {row.message && (
                            <Tooltip title={row.message}>
                              <span style={{ color: '#8c8c8c' }}>{row.message}</span>
                            </Tooltip>
                          )}
                        </Space>
                      );
                    },
                  },
                ]}
              />
            </>
          )}
        </Space>
      </Modal>
    </div>
  );
};

export default BankTransactions;
