// 对账工作台（银行对账 Tab 2）：双栏核销面板 —— 左栏勾选银行流水，右栏按评分勾选账面分录候选并确认核销。
// 从 src/pages/BankTransactions.tsx 抽取（纯搬家，行为与 UI 不变）。
import { useCallback, useEffect, useState } from 'react';
import {
  Button,
  Card,
  Checkbox,
  Col,
  InputNumber,
  Modal,
  Popconfirm,
  Row,
  Select,
  Space,
  Table,
  Tag,
  Tooltip,
  Typography,
  message,
} from 'antd';
import type { ColumnsType } from 'antd/es/table';
import { CheckCircleOutlined, DisconnectOutlined, ReloadOutlined } from '@ant-design/icons';
import {
  batchConfirmBankAutoMatches,
  cancelBankAllocation,
  confirmBankAllocations,
  listBankAllocations,
  migrateLegacyBankMatches,
  previewBankAllocationCandidates,
  queryBankTransactions,
} from '@/api';
import SensitiveText from '@/components/SensitiveText';
import { AMOUNT_TOLERANCE, txSideAmount } from '@/components/bankTransactionsShared';
import type {
  BankAllocationBatchResult,
  BankAllocationCandidate,
  BankAllocationInput,
  BankAutoMatchPreviewItem,
  BankReconciliationAllocation,
  BankTransaction,
  FundAccount,
  LegacyBankMatchReport,
} from '@/types';

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

export interface BankReconciliationWorkbenchProps {
  month: string;
  accounts: FundAccount[];
  accountId?: number;
  onAccountChange: (id?: number) => void;
}

const BankReconciliationWorkbench: React.FC<BankReconciliationWorkbenchProps> = ({
  month,
  accounts,
  accountId,
  onAccountChange,
}) => {
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
          <Button data-tour="bank-auto-match" loading={autoRunning} onClick={handleAutoMatch}>
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

export default BankReconciliationWorkbench;
