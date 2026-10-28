import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Button,
  Card,
  Col,
  DatePicker,
  Input,
  InputNumber,
  Modal,
  Popconfirm,
  Row,
  Segmented,
  Select,
  Space,
  Spin,
  Statistic,
  Table,
  Tag,
  Tooltip,
  Typography,
  message,
} from 'antd';
import type { ColumnsType } from 'antd/es/table';
import { DeleteOutlined, EditOutlined, PlusOutlined, PrinterOutlined, QuestionCircleOutlined, ReloadOutlined } from '@ant-design/icons';
import dayjs, { type Dayjs } from 'dayjs';
import {
  createManualVoucher,
  getFundAccounts,
  getGlAccounts,
  getVouchers,
  voidManualVoucher,
} from '@/api';
import type {
  FundAccount,
  GlAccount,
  ManualVoucherKind,
  Voucher,
  VoucherLine,
} from '@/types';
import { MANUAL_VOUCHER_KIND_OPTIONS, VOUCHER_SOURCE_LABEL } from '@/types';
import { SensitiveText } from '@/components/SensitiveText';
import { SensitiveStatistic } from '@/components/SensitiveStatistic';
import VoucherPrintModal from '@/components/VoucherPrintModal';
import PageTour from '@/components/PageTour';
import { tourTarget } from '@/components/tourTarget';
import { useBusinessMonth } from '@/contexts/BusinessMonthContext';

const { Text } = Typography;

const fmtMoney = (value: number): string =>
  (Number(value) || 0).toLocaleString('zh-CN', { minimumFractionDigits: 2, maximumFractionDigits: 2 });

const errText = (e: unknown): string => (e instanceof Error ? e.message : String(e));

/** 资金科目编码（库存现金/银行存款/其他货币资金，与后端门禁一致） */
const isFundGlCode = (code?: string): boolean =>
  code === '1001' || code === '1002' || code === '1012';

const sourceTagColor: Record<string, string> = {
  salary_accrual: 'blue',
  salary_payment: 'geekblue',
  reimbursement_accrual: 'cyan',
  reimbursement_payment: 'green',
  invoice_expense: 'purple',
  bank_manual: 'orange',
  manual_voucher: 'volcano',
  asset_purchase: 'gold',
  asset_depreciation: 'lime',
  asset_disposal: 'magenta',
};

const sourceText = (sourceType: string) =>
  VOUCHER_SOURCE_LABEL[sourceType] ?? sourceType;

const voucherStatusMeta: Record<Voucher['status'], { text: string; color: string }> = {
  active: { text: '有效', color: 'green' },
  void: { text: '已作废', color: 'red' },
};

/** 手工凭证分录编辑行（Modal 内可增删） */
interface ManualLineRow {
  key: string;
  account_code?: string;
  debit_amount: number | null;
  credit_amount: number | null;
  summary?: string;
  fund_account_id?: number | null;
}

const Vouchers: React.FC = () => {
  const { month, monthStr, setMonth } = useBusinessMonth();
  // 页面操作指引（第十五阶段）：递增触发 PageTour 复看
  const [tourTick, setTourTick] = useState(0);
  const [sourceFilter, setSourceFilter] = useState<string | undefined>(undefined);
  const [statusFilter, setStatusFilter] = useState<string | undefined>(undefined);
  const [vouchers, setVouchers] = useState<Voucher[]>([]);
  const [loading, setLoading] = useState(false);
  const [voidingId, setVoidingId] = useState<number | null>(null);

  // 凭证打印预览 Modal（中式记账凭证版式）
  const [printOpen, setPrintOpen] = useState(false);
  const [printVouchers, setPrintVouchers] = useState<Voucher[]>([]);

  // 手工凭证 Modal
  const [manualOpen, setManualOpen] = useState(false);
  const [voucherKind, setVoucherKind] = useState<ManualVoucherKind>('transfer_voucher');
  const [voucherDate, setVoucherDate] = useState<Dayjs>(dayjs());
  const [remark, setRemark] = useState('');
  const [lines, setLines] = useState<ManualLineRow[]>([]);
  const [savingManual, setSavingManual] = useState(false);
  const [glAccounts, setGlAccounts] = useState<GlAccount[]>([]);
  const [fundAccounts, setFundAccounts] = useState<FundAccount[]>([]);
  const lineSeqRef = useRef(0);

  const fetchData = useCallback(async () => {
    setLoading(true);
    try {
      setVouchers(await getVouchers({
        month: monthStr,
        source_type: sourceFilter,
        status: statusFilter,
      }));
    } catch (e: unknown) {
      message.error('查询记账凭证失败: ' + errText(e));
    } finally {
      setLoading(false);
    }
  }, [monthStr, sourceFilter, statusFilter]);

  useEffect(() => {
    fetchData();
  }, [fetchData]);

  const accountOptions = useMemo(
    () =>
      glAccounts
        .filter((a) => a.is_active === 1)
        .map((a) => ({ value: a.code, label: `${a.code} ${a.name}` })),
    [glAccounts],
  );

  /** 资金账户候选：优先按科目挂接精确匹配，无挂接再按现金/银行类型兜底 */
  const fundOptionsFor = useCallback(
    (code?: string): { value: number; label: string }[] => {
      if (!code) return [];
      const byGl = fundAccounts.filter((f) => f.gl_account_code === code);
      const pool = byGl.length > 0
        ? byGl
        : fundAccounts.filter((f) => f.account_type === (code === '1001' ? 'cash' : 'bank'));
      return pool.map((f) => ({ value: f.id, label: `${f.name}（${f.account_code}）` }));
    },
    [fundAccounts],
  );

  const openManualModal = () => {
    setVoucherKind('transfer_voucher');
    setVoucherDate(dayjs());
    setRemark('');
    lineSeqRef.current = 0;
    setLines([makeLine(), makeLine()]);
    setManualOpen(true);
    void (async () => {
      try {
        const [accounts, funds] = await Promise.all([getGlAccounts(), getFundAccounts({})]);
        setGlAccounts(accounts);
        setFundAccounts(funds.filter((f) => f.is_active));
      } catch (e: unknown) {
        message.error('获取科目/资金账户失败: ' + errText(e));
      }
    })();
  };

  const makeLine = (): ManualLineRow => {
    lineSeqRef.current += 1;
    return { key: `L${lineSeqRef.current}`, debit_amount: null, credit_amount: null, fund_account_id: null };
  };

  const updateLine = (key: string, patch: Partial<ManualLineRow>) => {
    setLines((prev) => prev.map((l) => (l.key === key ? { ...l, ...patch } : l)));
  };

  const removeLine = (key: string) => {
    setLines((prev) => prev.filter((l) => l.key !== key));
  };

  // 借贷实时合计与保存门禁（与后端校验同口径的预检，最终以后端为准）
  const debitTotal = useMemo(
    () => lines.reduce((s, l) => s + (Number(l.debit_amount) || 0), 0),
    [lines],
  );
  const creditTotal = useMemo(
    () => lines.reduce((s, l) => s + (Number(l.credit_amount) || 0), 0),
    [lines],
  );
  const diff = Math.round((debitTotal - creditTotal) * 100) / 100;
  const balanced = Math.abs(diff) < 0.005;
  const validLines = useMemo(
    () =>
      lines.filter(
        (l) => l.account_code && ((Number(l.debit_amount) || 0) > 0 || (Number(l.credit_amount) || 0) > 0),
      ),
    [lines],
  );
  const fundLineMissing = validLines.some((l) => isFundGlCode(l.account_code) && !l.fund_account_id);
  const bothSidesLine = validLines.some(
    (l) => (Number(l.debit_amount) || 0) > 0 && (Number(l.credit_amount) || 0) > 0,
  );
  const kindNeedFund =
    voucherKind !== 'transfer_voucher' && !validLines.some((l) => isFundGlCode(l.account_code));
  const canSave =
    validLines.length >= 2 && balanced && !fundLineMissing && !bothSidesLine && !kindNeedFund;

  const saveProblems: string[] = [];
  if (validLines.length < 2) saveProblems.push('至少需要两行有效分录（科目 + 借或贷金额）');
  if (!balanced) saveProblems.push(`借贷不平衡：差额 ${fmtMoney(diff)}`);
  if (bothSidesLine) saveProblems.push('同一分录不能同时有借方和贷方金额');
  if (fundLineMissing) saveProblems.push('资金科目（1001/1002/1012）分录必须选择资金账户');
  if (kindNeedFund) saveProblems.push('收款/付款凭证必须包含资金科目（1001/1002/1012）分录');

  const handleSaveManual = async () => {
    if (!canSave) {
      message.warning(saveProblems[0]);
      return;
    }
    setSavingManual(true);
    try {
      const created = await createManualVoucher({
        belong_month: voucherDate.format('YYYY-MM'),
        voucher_date: voucherDate.format('YYYY-MM-DD'),
        voucher_kind: voucherKind,
        lines: validLines.map((l) => ({
          account_code: String(l.account_code),
          debit_amount: Number(l.debit_amount) || 0,
          credit_amount: Number(l.credit_amount) || 0,
          summary: l.summary?.trim() || null,
          fund_account_id: isFundGlCode(l.account_code) ? Number(l.fund_account_id) : null,
        })),
        remark: remark.trim() || null,
      });
      message.success(`凭证 ${created.voucher_no} 已保存（${fmtMoney(created.total_amount)} 元）`);
      setManualOpen(false);
      // 归属月不在当前浏览月时给出提示，避免“保存成功却看不到”
      const belongMonth = voucherDate.format('YYYY-MM');
      if (belongMonth !== monthStr) {
        message.info(`该凭证归属 ${belongMonth}，已切换月份便于查看`);
        setMonth(voucherDate);
      } else {
        void fetchData();
      }
    } catch (e: unknown) {
      message.error(errText(e) || '保存凭证失败');
    } finally {
      setSavingManual(false);
    }
  };

  /** 打开凭证打印预览（支持传入多张连续打印，每张独立分页） */
  const openPrintModal = (items: Voucher[]) => {
    setPrintVouchers(items);
    setPrintOpen(true);
  };

  const handleVoid = async (record: Voucher) => {
    setVoidingId(record.id);
    try {
      await voidManualVoucher(record.id);
      message.success(`凭证 ${record.voucher_no} 已作废`);
      void fetchData();
    } catch (e: unknown) {
      message.error('作废失败: ' + errText(e));
    } finally {
      setVoidingId(null);
    }
  };

  const summary = useMemo(() => ({
    total: vouchers.length,
    activeCount: vouchers.filter((item) => item.status === 'active').length,
    voidCount: vouchers.filter((item) => item.status === 'void').length,
    activeAmount: vouchers
      .filter((item) => item.status === 'active')
      .reduce((sum, item) => sum + item.total_amount, 0),
  }), [vouchers]);

  const columns: ColumnsType<Voucher> = [
    { title: '凭证号', dataIndex: 'voucher_no', key: 'voucher_no', width: 170, fixed: 'left' as const },
    { title: '凭证日期', dataIndex: 'voucher_date', key: 'voucher_date', width: 110 },
    { title: '月份', dataIndex: 'belong_month', key: 'belong_month', width: 90 },
    {
      title: '来源',
      dataIndex: 'source_type',
      key: 'source_type',
      width: 100,
      render: (sourceType: string) => (
        <Tag color={sourceTagColor[sourceType] ?? 'default'}>{sourceText(sourceType)}</Tag>
      ),
    },
    {
      title: '金额',
      dataIndex: 'total_amount',
      key: 'total_amount',
      width: 130,
      align: 'right' as const,
      render: (value: number) => <SensitiveText type="amount" value={value} />,
    },
    {
      title: '状态',
      dataIndex: 'status',
      key: 'status',
      width: 90,
      render: (status: Voucher['status']) => (
        <Tag color={voucherStatusMeta[status].color}>{voucherStatusMeta[status].text}</Tag>
      ),
    },
    { title: '摘要/备注', dataIndex: 'remark', key: 'remark', ellipsis: true },
    {
      title: '操作',
      key: 'op',
      width: 150,
      fixed: 'right' as const,
      render: (_, record: Voucher) => (
        <Space size={0}>
          {/* 打印：打开中式凭证打印预览（金额门禁在 Modal 内控制） */}
          <Button
            size="small"
            type="link"
            icon={<PrinterOutlined />}
            onClick={() => openPrintModal([record])}
          >
            打印
          </Button>
          {record.source_type === 'manual_voucher' && record.status === 'active' ? (
            <Popconfirm
              title="确认作废该凭证？"
              description="作废后留痕不可恢复；归属月份须处于开放状态"
              okText="作废"
              cancelText="取消"
              okButtonProps={{ danger: true }}
              onConfirm={() => void handleVoid(record)}
            >
              <Button size="small" type="link" danger loading={voidingId === record.id}>
                作废
              </Button>
            </Popconfirm>
          ) : null}
        </Space>
      ),
    },
  ];

  const lineColumns = [
    { title: '序号', dataIndex: 'line_order', key: 'line_order', width: 60 },
    { title: '科目编码', dataIndex: 'account_code', key: 'account_code', width: 110 },
    {
      title: '借方金额',
      dataIndex: 'debit_amount',
      key: 'debit_amount',
      width: 130,
      align: 'right' as const,
      render: (value: number) => (value > 0 ? <SensitiveText type="amount" value={value} /> : '-'),
    },
    {
      title: '贷方金额',
      dataIndex: 'credit_amount',
      key: 'credit_amount',
      width: 130,
      align: 'right' as const,
      render: (value: number) => (value > 0 ? <SensitiveText type="amount" value={value} /> : '-'),
    },
    { title: '摘要', dataIndex: 'summary', key: 'summary', ellipsis: true },
  ];

  // 手工凭证分录编辑列（可增删行）
  const manualLineColumns: ColumnsType<ManualLineRow> = [
    {
      title: '科目',
      dataIndex: 'account_code',
      key: 'account_code',
      width: 230,
      render: (_, row) => (
        <Select
          size="small"
          style={{ width: '100%' }}
          placeholder="选择科目"
          showSearch
          optionFilterProp="label"
          value={row.account_code}
          options={accountOptions}
          onChange={(code) => updateLine(row.key, { account_code: code, fund_account_id: null })}
        />
      ),
    },
    {
      title: '借方金额',
      dataIndex: 'debit_amount',
      key: 'debit_amount',
      width: 130,
      align: 'right',
      render: (_, row) => (
        <InputNumber
          size="small"
          min={0}
          precision={2}
          step={0.01}
          style={{ width: '100%' }}
          placeholder="0.00"
          value={row.debit_amount}
          onChange={(v) => updateLine(row.key, { debit_amount: v })}
        />
      ),
    },
    {
      title: '贷方金额',
      dataIndex: 'credit_amount',
      key: 'credit_amount',
      width: 130,
      align: 'right',
      render: (_, row) => (
        <InputNumber
          size="small"
          min={0}
          precision={2}
          step={0.01}
          style={{ width: '100%' }}
          placeholder="0.00"
          value={row.credit_amount}
          onChange={(v) => updateLine(row.key, { credit_amount: v })}
        />
      ),
    },
    {
      title: '摘要',
      dataIndex: 'summary',
      key: 'summary',
      render: (_, row) => (
        <Input
          size="small"
          maxLength={100}
          placeholder="选填"
          value={row.summary}
          onChange={(e) => updateLine(row.key, { summary: e.target.value })}
        />
      ),
    },
    {
      title: '资金账户',
      dataIndex: 'fund_account_id',
      key: 'fund_account_id',
      width: 200,
      render: (_, row) =>
        isFundGlCode(row.account_code) ? (
          <Select
            size="small"
            style={{ width: '100%' }}
            placeholder="资金科目必选"
            showSearch
            optionFilterProp="label"
            value={row.fund_account_id ?? undefined}
            options={fundOptionsFor(row.account_code)}
            onChange={(id) => updateLine(row.key, { fund_account_id: id ?? null })}
          />
        ) : (
          <span style={{ color: '#8c8c8c' }}>—</span>
        ),
    },
    {
      title: '',
      key: 'op',
      width: 50,
      render: (_, row) => (
        <Tooltip title="删除该行">
          <Button
            size="small"
            type="text"
            danger
            icon={<DeleteOutlined />}
            onClick={() => removeLine(row.key)}
          />
        </Tooltip>
      ),
    },
  ];

  return (
    <div>
      <div className="page-header">
        <span className="page-title">记账凭证</span>
        <div className="page-header-actions">
          <Button
            icon={<QuestionCircleOutlined />}
            onClick={() => setTourTick((t) => t + 1)}
          >
            指引
          </Button>
          <DatePicker
            data-tour="voucher-filter"
            picker="month"
            value={month}
            allowClear={false}
            onChange={(value) => value && setMonth(value)}
            style={{ width: 160 }}
          />
          <Select
            placeholder="凭证来源"
            allowClear
            value={sourceFilter}
            onChange={setSourceFilter}
            style={{ width: 140 }}
            options={Object.entries(VOUCHER_SOURCE_LABEL).map(([value, label]) => ({ value, label }))}
          />
          <Select
            placeholder="凭证状态"
            allowClear
            value={statusFilter}
            onChange={setStatusFilter}
            style={{ width: 130 }}
            options={[
              { value: 'active', label: '有效' },
              { value: 'void', label: '已作废' },
            ]}
          />
          <Button icon={<ReloadOutlined />} loading={loading} onClick={fetchData}>
            刷新
          </Button>
          <Button type="primary" data-tour="voucher-manual" icon={<EditOutlined />} onClick={openManualModal}>
            手工录入凭证
          </Button>
        </div>
      </div>

      <PageTour
        storageKey="vouchers"
        reopenTick={tourTick}
        steps={[
          {
            title: '记账凭证',
            description:
              '本工具的凭证由业务自动生成：工资锁定、报销审批、收付款单、票据流转、折旧计提等动作会自动出凭证并自动红冲。这里是全部凭证的查询与追溯入口。',
          },
          {
            title: '按月与来源筛选',
            description:
              '选月份 + 凭证来源（工资计提 / 报销 / 资金单据 / 票据 / 折旧 / 手工凭证等 11 类）+ 状态，快速定位目标凭证。',
            target: tourTarget('[data-tour="voucher-filter"]'),
          },
          {
            title: '手工录入凭证',
            description:
              '用于补记无模板业务（摊销、暂估、往来调账等）。收 / 付款凭证必须包含资金科目行（1001/1002/1012）并关联资金账户，保存时实时校验借贷平衡。',
            target: tourTarget('[data-tour="voucher-manual"]'),
          },
          {
            title: '作废与月结保护',
            description:
              '凭证作废生成红字冲销并留痕；自动凭证随业务反向联动（如作废发票自动红冲）。已月结月份的凭证受月结保护，须先反月结。',
          },
        ]}
      />

      <Spin spinning={loading}>
        <Row gutter={[16, 16]} className="mb-16">
          <Col xs={24} sm={12} lg={6}>
            <Card className="stat-card"><Statistic title="凭证数" value={summary.total} /></Card>
          </Col>
          <Col xs={24} sm={12} lg={6}>
            <Card className="stat-card"><Statistic title="有效凭证" value={summary.activeCount} /></Card>
          </Col>
          <Col xs={24} sm={12} lg={6}>
            <Card className="stat-card"><Statistic title="已作废" value={summary.voidCount} /></Card>
          </Col>
          <Col xs={24} sm={12} lg={6}>
            <Card className="stat-card">
              <SensitiveStatistic title="有效凭证金额" value={summary.activeAmount} />
            </Card>
          </Col>
        </Row>

        <Card>
          <Table<Voucher>
            rowKey="id"
            columns={columns}
            dataSource={vouchers}
            pagination={{ pageSize: 10 }}
            scroll={{ x: 1050 }}
            expandable={{
              expandedRowRender: (record: Voucher) => (
                <Table<VoucherLine>
                  rowKey="id"
                  columns={lineColumns}
                  dataSource={[...record.lines].sort((a, b) => a.line_order - b.line_order)}
                  pagination={false}
                  size="small"
                />
              ),
            }}
          />
        </Card>
      </Spin>

      {/* 手工录入凭证 Modal */}
      <Modal
        title="手工录入凭证"
        open={manualOpen}
        onCancel={() => setManualOpen(false)}
        onOk={() => void handleSaveManual()}
        confirmLoading={savingManual}
        okText="保存凭证"
        cancelText="取消"
        okButtonProps={{ disabled: !canSave }}
        width={960}
      >
        <Space wrap size={12} style={{ marginBottom: 12 }}>
          <Space size={4}>
            <span style={{ color: '#666' }}>凭证种类：</span>
            <Segmented
              value={voucherKind}
              onChange={(v) => setVoucherKind(v as ManualVoucherKind)}
              options={MANUAL_VOUCHER_KIND_OPTIONS}
            />
          </Space>
          <Space size={4}>
            <span style={{ color: '#666' }}>凭证日期：</span>
            <DatePicker
              value={voucherDate}
              onChange={(d) => d && setVoucherDate(d)}
              allowClear={false}
              style={{ width: 150 }}
            />
          </Space>
          <Input
            placeholder="备注（选填）"
            maxLength={200}
            style={{ width: 320 }}
            value={remark}
            onChange={(e) => setRemark(e.target.value)}
          />
        </Space>
        <div style={{ color: '#8c8c8c', fontSize: 12, marginBottom: 8 }}>
          归属月按凭证日期所在月（{voucherDate.format('YYYY-MM')}）；收款/付款凭证必须包含资金科目
          （1001 库存现金 / 1002 银行存款 / 1012 其他货币资金）分录并选择资金账户；借贷合计必须相等。
        </div>

        <Table<ManualLineRow>
          rowKey="key"
          size="small"
          columns={manualLineColumns}
          dataSource={lines}
          pagination={false}
          locale={{ emptyText: '暂无分录，请添加' }}
          footer={() => (
            <Button
              size="small"
              type="dashed"
              icon={<PlusOutlined />}
              onClick={() => setLines((prev) => [...prev, makeLine()])}
            >
              添加分录
            </Button>
          )}
        />

        <div style={{ marginTop: 12, display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
          <Space size={16}>
            <span>借方合计 <Text strong>{fmtMoney(debitTotal)}</Text></span>
            <span>贷方合计 <Text strong>{fmtMoney(creditTotal)}</Text></span>
            {balanced ? (
              <Text type="success" strong>借贷平衡（差额 0.00）</Text>
            ) : (
              <Text type="danger" strong>差额 {fmtMoney(diff)}，借贷不平衡</Text>
            )}
            <span style={{ color: '#8c8c8c' }}>有效分录 {validLines.length} 行（需 ≥2）</span>
          </Space>
          {!canSave && saveProblems.length > 0 && (
            <Text type="danger" style={{ fontSize: 12 }}>{saveProblems[0]}</Text>
          )}
        </div>
      </Modal>

      {/* 凭证打印预览 Modal：多张凭证连续打印时每张独立分页 */}
      <VoucherPrintModal
        open={printOpen}
        vouchers={printVouchers}
        onClose={() => setPrintOpen(false)}
      />
    </div>
  );
};

export default Vouchers;
