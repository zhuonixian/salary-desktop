import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Alert,
  Button,
  Card,
  Col,
  DatePicker,
  Input,
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
  message,
} from 'antd';
import {
  DisconnectOutlined,
  FileAddOutlined,
  FileDoneOutlined,
  ImportOutlined,
  QuestionCircleOutlined,
  ReloadOutlined,
  SearchOutlined,
  StopOutlined,
} from '@ant-design/icons';
import { open } from '@tauri-apps/plugin-dialog';
import {
  cancelBankTransactionMatch,
  createBankManualVoucher,
  getFundAccounts,
  getGlAccounts,
  ignoreBankTransaction,
  importBankTransactionsFile,
  previewBankTransactionImport,
  queryBankTransactions,
} from '@/api';
import SensitiveText from '@/components/SensitiveText';
import { SensitiveStatistic } from '@/components/SensitiveStatistic';
import PageTour from '@/components/PageTour';
import { tourTarget } from '@/components/tourTarget';
import BankReconciliationWorkbench from '@/components/BankReconciliationWorkbench';
import BankReconciliationPeriodsPanel from '@/components/BankReconciliationPeriodsPanel';
import { AMOUNT_TOLERANCE, fmtMoney, txSideAmount } from '@/components/bankTransactionsShared';
import { useBusinessMonth } from '@/contexts/BusinessMonthContext';
import type {
  BankColumnMapping,
  BankImportPreview,
  BankImportPreviewRow,
  BankTransaction,
  BankTransactionStatus,
  FundAccount,
  GlAccount,
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

// 列识别提示（第十七阶段）：标准字段中文名
const BANK_FIELD_LABELS: Record<string, string> = {
  date: '交易日期',
  summary: '摘要',
  income: '收入列',
  expense: '支出列',
  single_amount: '单列金额',
  counterparty_name: '对方户名',
  counterparty_account: '对方账号',
  balance: '余额',
};

// 关键字段：日期缺失，或收入/支出/单列金额全缺 → 无法解析出行，必须手工映射
function bankCriticalMissing(hints: BankImportPreview['column_hints']) {
  const get = (f: string) => hints.find((h) => h.field === f);
  const dateMissing = !get('date')?.column;
  const amountMissing =
    !get('income')?.column && !get('expense')?.column && !get('single_amount')?.column;
  return { dateMissing, amountMissing, any: dateMissing || amountMissing };
}

// AMOUNT_TOLERANCE / fmtMoney / txSideAmount 三方共用常量已抽取至 @/components/bankTransactionsShared

/** 伪未达项：状态是待匹配，但新对账引擎仍有 active 核销额（常见于旧匹配取消后核销未释放） */
const hasAllocationResidual = (tx: Pick<BankTransaction, 'status' | 'allocated_amount'>): boolean =>
  tx.status === 'unmatched' && (tx.allocated_amount ?? 0) > AMOUNT_TOLERANCE;

/** 伪未达项两步恢复引导（stage7 Minor 11） */
const RESIDUAL_RECOVERY_HINT =
  '两步恢复：① 在「对账工作台」取消该流水的相关核销，释放残留核销额；② 回到本页重新匹配批次或生成凭证/忽略。';

// ==================== 主页面 ====================

const BankTransactions: React.FC = () => {
  const { month, setMonth } = useBusinessMonth();
  // 页面操作指引（第十五阶段）：递增触发 PageTour 复看
  const [tourTick, setTourTick] = useState(0);
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
  // 第十七阶段：列识别诊断 + 手工映射（关键字段未识别时可指定列重析）
  const [columnMapping, setColumnMapping] = useState<BankColumnMapping>({});
  const openImportModal = () => {
    setImportAccountId(undefined);
    setImportFile(null);
    setImportPreview(null);
    setColumnMapping({});
    setImportOpen(true);
  };

  const runImportPreview = useCallback(
    async (filePath: string, accountId: number, mapping?: BankColumnMapping) => {
      setPreviewLoading(true);
      try {
        const preview = await previewBankTransactionImport(filePath, accountId, mapping);
        setImportPreview(preview);
      } catch (e: unknown) {
        setImportPreview(null);
        message.error('解析流水文件失败: ' + (e instanceof Error ? e.message : String(e)));
      } finally {
        setPreviewLoading(false);
      }
    },
    [],
  );

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
    setColumnMapping({});
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
      const result = await importBankTransactionsFile(importFile, importAccountId, columnMapping);
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
  // 新引擎核销走「对账工作台」，取消核销在该页完成）。
  // 取消只作废旧匹配并把流水打回待匹配，不动新引擎核销：若流水仍有 active
  // 核销残留则给出两步恢复引导（Minor 11），避免当作普通待匹配流水重复匹配/入账。
  const handleCancelMatch = async (tx: BankTransaction) => {
    setAction(`cancel-${tx.id}`);
    try {
      await cancelBankTransactionMatch(tx.id);
      let residual = 0;
      try {
        const fresh = await queryBankTransactions({ belong_month: tx.belong_month });
        residual = fresh.find((r) => r.id === tx.id)?.allocated_amount ?? 0;
      } catch {
        // 查询失败不阻断取消流程，仅退回普通提示
      }
      if (residual > AMOUNT_TOLERANCE) {
        message.warning(
          `匹配已取消，但该流水在新对账引擎仍有核销残留 ${fmtMoney(residual)} 元（伪未达项）。${RESIDUAL_RECOVERY_HINT}`,
          8,
        );
      } else {
        message.success('匹配已取消');
      }
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
      width: 120,
      render: (status: BankTransactionStatus, tx: BankTransaction) => (
        <Space size={4} wrap>
          <Tag color={statusMeta[status].color}>{statusMeta[status].text}</Tag>
          {hasAllocationResidual(tx) && (
            <Tooltip
              title={`伪未达项：流水仍有 ${fmtMoney(tx.allocated_amount)} 元未释放核销额（常见于旧匹配取消后核销未释放）。${RESIDUAL_RECOVERY_HINT}`}
            >
              <Tag color="orange">核销残留</Tag>
            </Tooltip>
          )}
        </Space>
      ),
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
          <Button
            icon={<QuestionCircleOutlined />}
            onClick={() => setTourTick((t) => t + 1)}
          >
            指引
          </Button>
          <DatePicker
            data-tour="bank-month"
            picker="month"
            value={month}
            allowClear={false}
            onChange={(value) => value && setMonth(value)}
            style={{ width: 150 }}
          />
        </div>
      </div>

      <PageTour
        storageKey="bank-transactions"
        reopenTick={tourTick}
        steps={[
          {
            title: '银行对账',
            description:
              '本页把银行流水与账内资金记录逐笔勾稽。流程：① 导入流水 → ② 匹配核销 → ③ 生成余额调节表。首次使用建议按指引走一遍。',
          },
          {
            title: '第一步：选月份',
            description:
              '对账按月进行。先选业务月份，再在下方选择资金账户（银行 / 第三方支付账户）。',
            target: tourTarget('[data-tour="bank-month"]'),
          },
          {
            title: '三个工作区',
            description:
              '「银行流水」导入与查看状态；「对账工作台」左右双栏手工核销；「余额调节表」按期生成调节表并确认。',
            target: tourTarget('.ant-tabs-nav'),
          },
          {
            title: '自动匹配',
            description:
              '「自动匹配」先消化高置信全额匹配（置信线可调）；剩余流水进对账工作台逐笔双栏核销，支持一条流水对多条分录的部分核销。',
            target: tourTarget('[data-tour="bank-auto-match"]'),
          },
        ]}
      />

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
              <BankReconciliationWorkbench
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
              <BankReconciliationPeriodsPanel
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
          disabled:
            !importPreview || importPreview.error_rows > 0 || importPreview.total_rows === 0,
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
              {/* 列识别诊断（第十七阶段） */}
              <Space wrap size={6}>
                {importPreview.column_hints.map((h) => {
                  const label = BANK_FIELD_LABELS[h.field] ?? h.field;
                  const critical =
                    h.field === 'date' ||
                    ['income', 'expense', 'single_amount'].includes(h.field);
                  const color = h.column
                    ? h.source === 'manual'
                      ? 'blue'
                      : 'green'
                    : h.source === 'manual_missing'
                      ? 'orange'
                      : critical
                        ? 'red'
                        : 'default';
                  return (
                    <Tag key={h.field} color={color}>
                      {label}：
                      {h.column ?? (h.source === 'manual_missing' ? '映射列不存在' : '未识别')}
                    </Tag>
                  );
                })}
              </Space>
              {bankCriticalMissing(importPreview.column_hints).any && (
                <Alert
                  type="warning"
                  showIcon
                  message="关键字段未识别，请为缺失字段指定文件列后重新解析"
                  description={
                    <Space direction="vertical" size={8} style={{ width: '100%' }}>
                      <Space wrap>
                        {(['date', 'income', 'expense', 'single_amount'] as const)
                          .filter((f) => !importPreview.column_hints.find((h) => h.field === f)?.column)
                          .map((f) => (
                            <Select
                              key={f}
                              allowClear
                              placeholder={`映射「${BANK_FIELD_LABELS[f]}」到文件列`}
                              value={(columnMapping as Record<string, string | undefined>)[f]}
                              onChange={(v) => setColumnMapping((m) => ({ ...m, [f]: v }))}
                              style={{ width: 230 }}
                              options={importPreview.headers.map((h) => ({ value: h, label: h }))}
                            />
                          ))}
                        <Button
                          loading={previewLoading}
                          disabled={!importFile || !importAccountId}
                          onClick={() => void runImportPreview(importFile!, importAccountId!, columnMapping)}
                        >
                          重新解析
                        </Button>
                      </Space>
                      <span style={{ color: '#8c8c8c', fontSize: 12 }}>
                        单列金额：银行导出仅一列「交易金额」（正=收入 / 负=支出）时映射到该列；有收入 / 支出双列时无需填写。
                      </span>
                    </Space>
                  }
                />
              )}
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
