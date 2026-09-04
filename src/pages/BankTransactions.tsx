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
  Tag,
  Tooltip,
  message,
} from 'antd';
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
import { open } from '@tauri-apps/plugin-dialog';
import {
  autoMatchBankTransactions,
  cancelBankTransactionMatch,
  confirmBankTransactionMatch,
  createBankManualVoucher,
  getFundAccounts,
  getGlAccounts,
  ignoreBankTransaction,
  importBankTransactionsFile,
  previewBankTransactionImport,
  queryBankTransactions,
  queryPaymentBatches,
} from '@/api';
import { SensitiveText } from '@/components/SensitiveText';
import { SensitiveStatistic } from '@/components/SensitiveStatistic';
import { useBusinessMonth } from '@/contexts/BusinessMonthContext';
import type {
  BankImportPreview,
  BankImportPreviewRow,
  BankTransaction,
  BankTransactionStatus,
  FundAccount,
  GlAccount,
  PaymentBatch,
  PaymentBatchType,
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

const typeText: Record<PaymentBatchType, string> = {
  salary: '工资',
  reimbursement: '报销',
  general: '通用',
};

const fmtMoney = (value?: number | null) =>
  (value ?? 0).toLocaleString('zh-CN', { minimumFractionDigits: 2, maximumFractionDigits: 2 });

const BankTransactions: React.FC = () => {
  const { month, setMonth } = useBusinessMonth();
  const [statusFilter, setStatusFilter] = useState<BankTransactionStatus | undefined>(undefined);
  const [accountFilter, setAccountFilter] = useState<number | undefined>(undefined);
  const [keyword, setKeyword] = useState('');
  const [transactions, setTransactions] = useState<BankTransaction[]>([]);
  const [paidBatches, setPaidBatches] = useState<PaymentBatch[]>([]);
  const [loading, setLoading] = useState(false);
  const [action, setAction] = useState<string | null>(null);
  const [matchTx, setMatchTx] = useState<BankTransaction | null>(null);
  const [selectedBatchId, setSelectedBatchId] = useState<number | undefined>(undefined);
  const [matchRemark, setMatchRemark] = useState('');
  const [ignoreTx, setIgnoreTx] = useState<BankTransaction | null>(null);
  const [ignoreReason, setIgnoreReason] = useState('');
  const [voucherTx, setVoucherTx] = useState<BankTransaction | null>(null);
  const [voucherAccounts, setVoucherAccounts] = useState<GlAccount[]>([]);
  const [voucherAccountCode, setVoucherAccountCode] = useState<string | undefined>(undefined);
  const [fundAccountOptions, setFundAccountOptions] = useState<FundAccount[]>([]);
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
      const [txData, batchData] = await Promise.all([
        queryBankTransactions({
          belong_month: belongMonth,
          status: statusFilter,
          keyword: keyword.trim() || undefined,
          fund_account_id: accountFilter,
        }),
        queryPaymentBatches({ belong_month: belongMonth, status: 'paid' }),
      ]);
      setTransactions(txData);
      setPaidBatches(batchData);
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
        setFundAccountOptions(accounts.filter((a) => ['bank', 'third_party'].includes(a.account_type))),
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

  const handleAutoMatch = async () => {
    setAction('auto-match');
    try {
      const result = await autoMatchBankTransactions(month.format('YYYY-MM'));
      message.success(`自动匹配完成：成功 ${result.matched} 条，跳过 ${result.skipped} 条`);
      await fetchData();
    } catch (e: unknown) {
      message.error('自动匹配失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setAction(null);
    }
  };

  const matchCandidates = useMemo(
    () => matchTx
      ? paidBatches.filter((batch) => Math.abs(batch.total_amount - matchTx.expense_amount) <= 0.01)
      : [],
    [matchTx, paidBatches],
  );

  const openMatchModal = (tx: BankTransaction) => {
    const candidates = paidBatches.filter((batch) => Math.abs(batch.total_amount - tx.expense_amount) <= 0.01);
    if (candidates.length === 0) {
      message.warning('没有金额一致的已付款批次');
      return;
    }
    setSelectedBatchId(candidates[0]?.id);
    setMatchRemark('');
    setMatchTx(tx);
  };

  const openIgnoreModal = (tx: BankTransaction) => {
    setIgnoreReason('');
    setIgnoreTx(tx);
  };

  const handleConfirmMatch = async () => {
    if (!matchTx || !selectedBatchId) {
      message.warning('请选择付款批次');
      return;
    }
    setAction(`match-${matchTx.id}`);
    try {
      await confirmBankTransactionMatch({
        transaction_id: matchTx.id,
        payment_batch_id: selectedBatchId,
        remark: matchRemark || undefined,
      });
      message.success('匹配已确认');
      setMatchTx(null);
      await fetchData();
    } catch (e: unknown) {
      message.error('确认匹配失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setAction(null);
    }
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
          <Tooltip title="历史流水未归集账户，不参与自动匹配">
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
      title: '匹配批次',
      dataIndex: 'matched_batch_no',
      key: 'matched_batch_no',
      width: 220,
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
      width: 320,
      fixed: 'right' as const,
      render: (_: unknown, tx: BankTransaction) => (
        <Space size={6} wrap>
          <Button
            size="small"
            icon={<CheckCircleOutlined />}
            disabled={tx.status !== 'unmatched' || tx.expense_amount <= 0}
            loading={action === `match-${tx.id}`}
            onClick={() => openMatchModal(tx)}
          >
            匹配
          </Button>
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
            title="确认取消该流水匹配?"
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
        <span className="page-title">银行流水</span>
        <div className="page-header-actions">
          <DatePicker
            picker="month"
            value={month}
            allowClear={false}
            onChange={(value) => value && setMonth(value)}
            style={{ width: 150 }}
          />
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
            options={fundAccountOptions.map((a) => ({
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
          <Button type="primary" loading={action === 'auto-match'} onClick={handleAutoMatch}>
            自动匹配
          </Button>
        </div>
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
            scroll={{ x: 1680 }}
          />
        </Card>
      </Spin>

      <Modal
        title="匹配付款批次"
        open={!!matchTx}
        okText="确认匹配"
        cancelText="取消"
        confirmLoading={!!matchTx && action === `match-${matchTx.id}`}
        onOk={handleConfirmMatch}
        onCancel={() => setMatchTx(null)}
      >
        {matchTx && (
          <Space direction="vertical" style={{ width: '100%' }} size={12}>
            <div>
              <div>
                {matchTx.transaction_date} / 支出{' '}
                <SensitiveText type="amount" value={matchTx.expense_amount} />
              </div>
              <div>{matchTx.summary || matchTx.counterparty_name || '-'}</div>
            </div>
            <Select
              showSearch
              optionFilterProp="label"
              value={selectedBatchId}
              onChange={setSelectedBatchId}
              style={{ width: '100%' }}
              options={matchCandidates.map((batch) => ({
                value: batch.id,
                label: `${batch.batch_no} / ${typeText[batch.batch_type]} / ¥${fmtMoney(batch.total_amount)}`,
              }))}
            />
            <Input.TextArea
              rows={2}
              placeholder="可填写匹配说明"
              value={matchRemark}
              onChange={(event) => setMatchRemark(event.target.value)}
            />
          </Space>
        )}
      </Modal>

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
              options={fundAccountOptions.map((a) => ({
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
          icon: <ImportOutlined />,
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
              options={fundAccountOptions.map((a) => ({
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
