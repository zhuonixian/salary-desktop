import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Button,
  Card,
  Col,
  DatePicker,
  Form,
  Input,
  InputNumber,
  Modal,
  Row,
  Select,
  Space,
  Spin,
  Switch,
  Table,
  Tag,
  Tooltip,
  message,
} from 'antd';
import type { ColumnsType } from 'antd/es/table';
import {
  DownloadOutlined,
  ReloadOutlined,
  RollbackOutlined,
} from '@ant-design/icons';
import dayjs, { type Dayjs } from 'dayjs';
import { save } from '@tauri-apps/plugin-dialog';
import {
  cancelArAllocation,
  createArAllocation,
  exportArAgingReport,
  exportOutputTaxLedger,
  getArAllocations,
  getArLedger,
  getFundDocuments,
} from '@/api';
import SensitiveStatistic from '@/components/SensitiveStatistic';
import SensitiveText from '@/components/SensitiveText';
import { useSecurity } from '@/contexts/SecurityContext';
import {
  AR_AGING_BUCKETS,
  AR_STATUS_COLOR,
  type ArLedgerRow,
  type ArLedgerQuery,
  type ArReceiptAllocation,
  type FundDocument,
} from '@/types';

const fmtMoney = (value?: number | null) =>
  (value ?? 0).toLocaleString('zh-CN', { minimumFractionDigits: 2, maximumFractionDigits: 2 });

const bucketColor = (bucket: string): string =>
  AR_AGING_BUCKETS.find((b) => b.key === bucket)?.color ?? 'default';

const bucketLabel = (bucket: string): string =>
  AR_AGING_BUCKETS.find((b) => b.key === bucket)?.label ?? bucket;

const errText = (e: unknown): string => (e instanceof Error ? e.message : String(e));

interface AllocateFormValues {
  receipt_id: number;
  amount: number;
  remark?: string;
}

/** 应收台账（第十二阶段 Task 2，spec 4.2/4.3）：销项发票核销进度、账龄分桶与回款核销 */
const ARReceivables: React.FC = () => {
  const { isSensitiveRevealed } = useSecurity();

  const [rows, setRows] = useState<ArLedgerRow[]>([]);
  const [loading, setLoading] = useState(false);
  const [exporting, setExporting] = useState<'aging' | 'output-tax' | null>(null);

  const [keyword, setKeyword] = useState('');
  const [onlyOutstanding, setOnlyOutstanding] = useState(false);
  const [asOfDate, setAsOfDate] = useState<Dayjs>(dayjs());

  // 回款核销 Modal：选择已结算收款单 → 核销金额默认取 min(收款单可用额, 发票未核销余额)
  const [allocTarget, setAllocTarget] = useState<ArLedgerRow | null>(null);
  const [receipts, setReceipts] = useState<FundDocument[]>([]);
  const [receiptAvailable, setReceiptAvailable] = useState<number | null>(null);
  const [allocForm] = Form.useForm<AllocateFormValues>();
  const watchReceiptId = Form.useWatch('receipt_id', allocForm);
  const [saving, setSaving] = useState(false);

  // 核销记录（行展开小表，含 cancelled 留痕）
  const [expandedKeys, setExpandedKeys] = useState<number[]>([]);
  const [allocationMap, setAllocationMap] = useState<Record<number, ArReceiptAllocation[]>>({});
  const [allocationLoading, setAllocationLoading] = useState(false);

  // 取消核销（原因必填）
  const [cancelTarget, setCancelTarget] = useState<ArReceiptAllocation | null>(null);
  const [cancelReason, setCancelReason] = useState('');

  // 导出销项台账（起止月份，默认当月）
  const [taxLedgerOpen, setTaxLedgerOpen] = useState(false);
  const [taxFromMonth, setTaxFromMonth] = useState<Dayjs>(dayjs());
  const [taxToMonth, setTaxToMonth] = useState<Dayjs>(dayjs());

  const currentQuery: ArLedgerQuery = useMemo(
    () => ({
      keyword: keyword.trim() || undefined,
      only_outstanding: onlyOutstanding || undefined,
      as_of_date: asOfDate.format('YYYY-MM-DD'),
    }),
    [keyword, onlyOutstanding, asOfDate],
  );

  const fetchData = useCallback(async () => {
    setLoading(true);
    try {
      setRows(await getArLedger(currentQuery));
    } catch (e: unknown) {
      message.error('查询应收台账失败: ' + errText(e));
    } finally {
      setLoading(false);
    }
  }, [currentQuery]);

  useEffect(() => {
    void fetchData();
  }, [fetchData]);

  const loadAllocations = useCallback(async (invoiceId: number) => {
    setAllocationLoading(true);
    try {
      const list = await getArAllocations({ invoiceId });
      setAllocationMap((prev) => ({ ...prev, [invoiceId]: list }));
    } catch (e: unknown) {
      message.error('查询核销记录失败: ' + errText(e));
    } finally {
      setAllocationLoading(false);
    }
  }, []);

  const loadReceipts = useCallback(async () => {
    try {
      setReceipts(await getFundDocuments({ document_type: 'receipt', status: 'settled' }));
    } catch {
      setReceipts([]);
    }
  }, []);

  const openAllocateModal = (row: ArLedgerRow) => {
    setAllocTarget(row);
    setReceiptAvailable(null);
    allocForm.resetFields();
    void loadReceipts();
  };

  // 选中收款单后：查该单 active 核销合计得可用额，并预填默认核销金额
  useEffect(() => {
    if (!watchReceiptId || !allocTarget) return;
    let cancelled = false;
    void (async () => {
      try {
        const allocs = await getArAllocations({ receiptId: watchReceiptId });
        if (cancelled) return;
        const used = allocs
          .filter((a) => a.status === 'active')
          .reduce((s, a) => s + a.allocated_amount, 0);
        const receipt = receipts.find((r) => r.id === watchReceiptId);
        const available = Math.max(0, (receipt?.amount ?? 0) - used);
        setReceiptAvailable(available);
        allocForm.setFieldsValue({
          amount: Math.round(Math.min(available, allocTarget.outstanding_amount) * 100) / 100,
        });
      } catch (e: unknown) {
        message.error('查询收款单可用额度失败: ' + errText(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [watchReceiptId, allocTarget, receipts, allocForm]);

  const submitAllocate = async (values: AllocateFormValues) => {
    if (!allocTarget) return;
    setSaving(true);
    try {
      await createArAllocation({
        receipt_id: Number(values.receipt_id),
        invoice_id: allocTarget.invoice_id,
        amount: Number(values.amount),
        remark: values.remark?.trim() || null,
      });
      message.success('回款核销已登记');
      setAllocTarget(null);
      allocForm.resetFields();
      void fetchData();
      if (expandedKeys.includes(allocTarget.invoice_id)) {
        void loadAllocations(allocTarget.invoice_id);
      }
    } catch (e: unknown) {
      message.error('登记核销失败: ' + errText(e));
    } finally {
      setSaving(false);
    }
  };

  const confirmCancelAllocation = async () => {
    if (!cancelTarget) return;
    const reason = cancelReason.trim();
    if (!reason) {
      message.warning('取消核销必须填写原因');
      return;
    }
    try {
      await cancelArAllocation({ id: cancelTarget.id, reason });
      message.success('已取消核销，发票未核销余额已恢复');
      setCancelTarget(null);
      setCancelReason('');
      void fetchData();
      if (expandedKeys.includes(cancelTarget.invoice_id)) {
        void loadAllocations(cancelTarget.invoice_id);
      }
    } catch (e: unknown) {
      message.error('取消核销失败: ' + errText(e));
    }
  };

  const handleExportAging = async () => {
    const target = await save({
      defaultPath: `应收账龄表_${asOfDate.format('YYYY-MM-DD')}.xlsx`,
      filters: [{ name: 'Excel', extensions: ['xlsx'] }],
    });
    if (!target) return;
    setExporting('aging');
    try {
      await exportArAgingReport(currentQuery, String(target));
      message.success('应收账龄表已导出');
    } catch (e: unknown) {
      message.error('导出失败: ' + errText(e));
    } finally {
      setExporting(null);
    }
  };

  const handleExportOutputTax = async () => {
    const from = taxFromMonth.format('YYYY-MM');
    const to = taxToMonth.format('YYYY-MM');
    if (from > to) {
      message.warning('起始月份不能晚于结束月份');
      return;
    }
    const target = await save({
      defaultPath: `${from}_${to}_销项台账.xlsx`,
      filters: [{ name: 'Excel', extensions: ['xlsx'] }],
    });
    if (!target) return;
    setExporting('output-tax');
    try {
      await exportOutputTaxLedger(from, to, String(target));
      message.success('销项台账已导出');
      setTaxLedgerOpen(false);
    } catch (e: unknown) {
      message.error('导出失败: ' + errText(e));
    } finally {
      setExporting(null);
    }
  };

  // 汇总（前端聚合）：未核销总额 + 五个账龄分桶（仅统计未核清部分）
  const outstandingTotal = rows.reduce((s, r) => s + r.outstanding_amount, 0);
  const bucketSums = useMemo(() => {
    const map: Record<string, number> = {};
    for (const b of AR_AGING_BUCKETS) map[b.key] = 0;
    for (const r of rows) {
      if (r.outstanding_amount <= 0.005) continue;
      if (map[r.aging_bucket] === undefined) map[r.aging_bucket] = 0;
      map[r.aging_bucket] += r.outstanding_amount;
    }
    return map;
  }, [rows]);

  const allocationColumns: ColumnsType<ArReceiptAllocation> = [
    {
      title: '收款单号',
      key: 'receipt_no',
      width: 190,
      render: (_, a) => a.receipt_document_no ?? `收款单ID=${a.receipt_id}`,
    },
    {
      title: '收款结算日',
      dataIndex: 'receipt_settled_at',
      key: 'receipt_settled_at',
      width: 110,
      render: (v: string | null) => (v ?? '').slice(0, 10) || '-',
    },
    {
      title: '核销金额',
      dataIndex: 'allocated_amount',
      key: 'allocated_amount',
      width: 120,
      align: 'right',
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '收款摘要',
      dataIndex: 'receipt_summary',
      key: 'receipt_summary',
      ellipsis: true,
      render: (v: string | null) => v || '-',
    },
    {
      title: '备注',
      dataIndex: 'remark',
      key: 'remark',
      ellipsis: true,
      render: (v: string | null) => v || '-',
    },
    {
      title: '状态',
      dataIndex: 'status',
      key: 'status',
      width: 90,
      render: (v: string) =>
        v === 'active' ? <Tag color="success">生效</Tag> : <Tag>已取消</Tag>,
    },
    {
      title: '取消原因',
      dataIndex: 'cancel_reason',
      key: 'cancel_reason',
      ellipsis: true,
      render: (v: string | null) => v || '-',
    },
    {
      title: '操作',
      key: 'op',
      width: 110,
      render: (_, a) =>
        a.status === 'active' ? (
          <Tooltip title="取消后留痕（cancelled），发票未核销余额恢复">
            <Button
              size="small"
              type="link"
              danger
              icon={<RollbackOutlined />}
              onClick={() => {
                setCancelTarget(a);
                setCancelReason('');
              }}
            >
              取消核销
            </Button>
          </Tooltip>
        ) : (
          <span style={{ color: '#8c8c8c' }}>—</span>
        ),
    },
  ];

  const columns: ColumnsType<ArLedgerRow> = [
    {
      title: '票号',
      key: 'invoice_no',
      width: 200,
      render: (_, r) => (
        <div>
          <div style={{ fontSize: 12, color: '#999' }}>{r.invoice_code || '-'}</div>
          <div style={{ fontWeight: 500 }}>{r.invoice_number || '-'}</div>
        </div>
      ),
    },
    {
      title: '客户',
      key: 'partner',
      width: 180,
      ellipsis: true,
      render: (_, r) => r.partner_name || r.buyer_name || '-',
    },
    {
      title: '开票日',
      dataIndex: 'issue_date',
      key: 'issue_date',
      width: 110,
      render: (v: string | null) => v || '-',
    },
    {
      title: '归属月',
      dataIndex: 'belong_month',
      key: 'belong_month',
      width: 90,
      render: (v: string | null) => v || '-',
    },
    {
      title: '不含税金额',
      dataIndex: 'amount',
      key: 'amount',
      width: 120,
      align: 'right',
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '税额',
      dataIndex: 'tax_amount',
      key: 'tax_amount',
      width: 110,
      align: 'right',
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '价税合计',
      dataIndex: 'total_amount',
      key: 'total_amount',
      width: 120,
      align: 'right',
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '已核销',
      dataIndex: 'settled_amount',
      key: 'settled_amount',
      width: 120,
      align: 'right',
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '未核销余额',
      dataIndex: 'outstanding_amount',
      key: 'outstanding_amount',
      width: 130,
      align: 'right',
      render: (v: number) => (
        <span style={v > 0 ? { color: '#cf1322' } : undefined}>
          <SensitiveText type="amount" value={v} />
        </span>
      ),
    },
    { title: '账龄天数', dataIndex: 'aging_days', key: 'aging_days', width: 90 },
    {
      title: '账龄',
      dataIndex: 'aging_bucket',
      key: 'aging_bucket',
      width: 100,
      render: (v: string) => <Tag color={bucketColor(v)}>{bucketLabel(v)}</Tag>,
    },
    {
      title: '状态',
      dataIndex: 'status',
      key: 'status',
      width: 90,
      render: (v: string) => <Tag color={AR_STATUS_COLOR[v] ?? 'default'}>{v}</Tag>,
    },
    {
      title: '操作',
      key: 'op',
      width: 110,
      render: (_, r) =>
        r.outstanding_amount > 0.005 ? (
          <Button size="small" type="link" onClick={() => openAllocateModal(r)}>
            回款核销
          </Button>
        ) : (
          <span style={{ color: '#8c8c8c' }}>已核清</span>
        ),
    },
  ];

  return (
    <div>
      <div className="page-header">
        <span className="page-title">应收台账</span>
        <div className="page-header-actions">
          <Input.Search
            placeholder="客户/票号"
            allowClear
            style={{ width: 180 }}
            onSearch={setKeyword}
          />
          <Tooltip title="仅显示未核销余额大于 0 的销项发票">
            <Space size={4}>
              <span style={{ color: '#666' }}>仅未核清</span>
              <Switch size="small" checked={onlyOutstanding} onChange={setOnlyOutstanding} />
            </Space>
          </Tooltip>
          <Tooltip title="账龄计算基准日（默认今天）">
            <span style={{ color: '#666' }}>基准日</span>
          </Tooltip>
          <DatePicker
            value={asOfDate}
            onChange={(d) => d && setAsOfDate(d)}
            allowClear={false}
            style={{ width: 140 }}
          />
          <Button icon={<ReloadOutlined />} loading={loading} onClick={() => void fetchData()}>
            刷新
          </Button>
          <Tooltip title={isSensitiveRevealed ? '' : '敏感导出需先在页面中解锁敏感数据'}>
            <Button
              icon={<DownloadOutlined />}
              disabled={!isSensitiveRevealed}
              loading={exporting === 'aging'}
              onClick={() => void handleExportAging()}
            >
              导出应收账龄表
            </Button>
          </Tooltip>
          <Button
            type="primary"
            ghost
            icon={<DownloadOutlined />}
            onClick={() => setTaxLedgerOpen(true)}
          >
            导出销项台账
          </Button>
        </div>
      </div>

      <Spin spinning={loading}>
        <Row gutter={[16, 16]} className="mb-16">
          <Col xs={24} sm={12} lg={4}>
            <Card className="stat-card">
              <SensitiveStatistic title="未核销总额" value={outstandingTotal} />
            </Card>
          </Col>
          {AR_AGING_BUCKETS.map((b) => (
            <Col xs={24} sm={12} lg={4} key={b.key}>
              <Card className="stat-card">
                <SensitiveStatistic title={`${b.label} 未核销`} value={bucketSums[b.key] ?? 0} />
              </Card>
            </Col>
          ))}
        </Row>

        <Card
          title="销项发票应收台账"
          extra={
            <span style={{ color: '#8c8c8c' }}>账龄按开票日至基准日分桶；展开行查看核销记录</span>
          }
        >
          <Table<ArLedgerRow>
            rowKey="invoice_id"
            columns={columns}
            dataSource={rows}
            pagination={{ pageSize: 15, showSizeChanger: true }}
            scroll={{ x: 1660 }}
            expandable={{
              expandedRowKeys: expandedKeys,
              onExpand: (expanded, record) => {
                setExpandedKeys((prev) =>
                  expanded
                    ? [...prev, record.invoice_id]
                    : prev.filter((k) => k !== record.invoice_id),
                );
                if (expanded) void loadAllocations(record.invoice_id);
              },
              expandedRowRender: (record) => (
                <Spin spinning={allocationLoading}>
                  <Table<ArReceiptAllocation>
                    rowKey="id"
                    size="small"
                    columns={allocationColumns}
                    dataSource={allocationMap[record.invoice_id] ?? []}
                    pagination={false}
                    locale={{ emptyText: '暂无核销记录' }}
                  />
                </Spin>
              ),
            }}
          />
        </Card>
      </Spin>

      {/* 回款核销 Modal */}
      <Modal
        title={`回款核销 - ${allocTarget ? allocTarget.invoice_number || `发票ID=${allocTarget.invoice_id}` : ''}`}
        open={allocTarget !== null}
        onCancel={() => setAllocTarget(null)}
        onOk={() => allocForm.submit()}
        confirmLoading={saving}
        okText="登记核销"
        cancelText="取消"
        destroyOnHidden
      >
        <Form<AllocateFormValues>
          form={allocForm}
          layout="vertical"
          onFinish={(values) => void submitAllocate(values)}
        >
          <Form.Item label="发票未核销余额">
            <SensitiveText type="amount" value={allocTarget?.outstanding_amount ?? 0} />
            <span style={{ color: '#8c8c8c', marginLeft: 12 }}>
              价税合计 {fmtMoney(allocTarget?.total_amount ?? 0)} · 已核销{' '}
              {fmtMoney(allocTarget?.settled_amount ?? 0)}
            </span>
          </Form.Item>
          <Form.Item
            name="receipt_id"
            label="选择已结算收款单"
            rules={[{ required: true, message: '请选择收款单（仅已结算）' }]}
          >
            <Select
              placeholder="选择收款单（仅已结算）"
              showSearch
              optionFilterProp="label"
              options={receipts.map((r) => ({
                value: r.id,
                label: `${r.document_no}｜${r.document_date}｜${fmtMoney(r.amount)}｜${r.summary}`,
              }))}
            />
          </Form.Item>
          {receiptAvailable !== null && (
            <div style={{ marginBottom: 12, color: '#5f6b7a' }}>
              该收款单可用额度：{fmtMoney(receiptAvailable)}（扣除已在其他发票核销的 active 占用）
            </div>
          )}
          <Form.Item
            name="amount"
            label="核销金额"
            rules={[{ required: true, message: '请输入核销金额' }]}
            extra="默认取收款单可用额度与发票未核销余额的较小值；累计不得超过发票价税合计"
          >
            <InputNumber min={0.01} step={0.01} style={{ width: '100%' }} placeholder="0.00" />
          </Form.Item>
          <Form.Item name="remark" label="备注">
            <Input placeholder="选填" maxLength={200} />
          </Form.Item>
        </Form>
      </Modal>

      {/* 取消核销（原因必填） */}
      <Modal
        title="取消应收核销"
        open={cancelTarget !== null}
        onCancel={() => {
          setCancelTarget(null);
          setCancelReason('');
        }}
        onOk={() => void confirmCancelAllocation()}
        okText="确认取消"
        okButtonProps={{ danger: true }}
        cancelText="返回"
      >
        <div style={{ marginBottom: 12 }}>
          取消后该笔核销留痕（状态置为已取消），发票未核销余额与收款单可用额度恢复；双方归属月份须处于开放状态。
        </div>
        <Input.TextArea
          rows={3}
          value={cancelReason}
          onChange={(e) => setCancelReason(e.target.value)}
          placeholder="取消核销原因（必填）"
        />
      </Modal>

      {/* 导出销项台账（起止月份） */}
      <Modal
        title="导出销项台账"
        open={taxLedgerOpen}
        onCancel={() => setTaxLedgerOpen(false)}
        onOk={() => void handleExportOutputTax()}
        okText="导出"
        confirmLoading={exporting === 'output-tax'}
        cancelText="取消"
      >
        <Space>
          <DatePicker
            picker="month"
            value={taxFromMonth}
            onChange={(d) => d && setTaxFromMonth(d)}
            allowClear={false}
            placeholder="起始月份"
          />
          <span style={{ color: '#666' }}>至</span>
          <DatePicker
            picker="month"
            value={taxToMonth}
            onChange={(d) => d && setTaxToMonth(d)}
            allowClear={false}
            placeholder="结束月份"
          />
        </Space>
        <div style={{ color: '#8c8c8c', marginTop: 12, fontSize: 12 }}>
          按归属月份区间导出销项发票台账（含作废口径以后端为准）；默认当月。
        </div>
      </Modal>
    </div>
  );
};

export default ARReceivables;
