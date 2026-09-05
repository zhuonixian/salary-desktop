import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Button,
  Card,
  Col,
  DatePicker,
  Descriptions,
  Form,
  Input,
  Modal,
  Row,
  Select,
  Space,
  Spin,
  Switch,
  Table,
  Tag,
  Timeline,
  Tooltip,
  message,
} from 'antd';
import type { ColumnsType } from 'antd/es/table';
import {
  DownloadOutlined,
  ReloadOutlined,
  RollbackOutlined,
  PlusOutlined,
} from '@ant-design/icons';
import dayjs, { type Dayjs } from 'dayjs';
import { save } from '@tauri-apps/plugin-dialog';
import {
  approveFundDocument,
  cancelAdvanceSettlementLink,
  createFundDocument,
  exportAdvanceLedger,
  getAdvanceLedger,
  getAdvanceSettlementLinks,
  getEmployees,
  getFundAccounts,
  getFundDocuments,
  getGlAccounts,
  submitFundDocument,
  voidFundDocument,
} from '@/api';
import SensitiveStatistic from '@/components/SensitiveStatistic';
import SensitiveText from '@/components/SensitiveText';
import { useBusinessMonth } from '@/contexts/BusinessMonthContext';
import { useSecurity } from '@/contexts/SecurityContext';
import {
  FUND_DOCUMENT_STATUS_LABEL,
  SETTLEMENT_MODE_LABEL,
  type AdvanceLedgerRow,
  type AdvanceSettlementLink,
  type Employee,
  type FundAccount,
  type FundDocument,
} from '@/types';

const fmtMoney = (value?: number | null) =>
  (value ?? 0).toLocaleString('zh-CN', { minimumFractionDigits: 2, maximumFractionDigits: 2 });

const AGING_COLOR: Record<string, string> = {
  '0-30天': 'green',
  '31-60天': 'gold',
  '61-90天': 'orange',
  '90天以上': 'red',
  已结清: 'default',
};

const STATUS_COLOR: Record<string, string> = {
  draft: 'default',
  submitted: 'processing',
  approved: 'cyan',
  settled: 'success',
  void: '#999999',
  reversed: 'purple',
};

/** 其他应收款科目（借款单对方科目，缺省 1221 其他应收款） */
const ADVANCE_DEFAULT_GL = '1221';

type CommentAction =
  | { type: 'approve'; docId: number; docNo: string }
  | { type: 'void'; docId: number; docNo: string }
  | {
      type: 'cancel-link';
      link: AdvanceSettlementLink;
      loanNo: string;
      settledSettlement: boolean;
    };

interface SettlementFormValues {
  settlement_mode: string;
  employee_id: number;
  advance_id: number;
  amount: number;
  belong_month: string;
  document_date: string;
  target_account_id?: number;
  counter_account_code?: string;
  summary: string;
}

interface AdvanceFormValues {
  employee_id: number;
  amount: number;
  belong_month: string;
  document_date: string;
  source_account_id: number;
  due_date: string;
  counter_account_code?: string;
  summary: string;
}

/** 借款备用金（第七阶段 Task 14，spec 4.11）：借款台账、核销时间线与核销单创建 */
const Advances: React.FC = () => {
  const { month } = useBusinessMonth();
  const { isSensitiveRevealed } = useSecurity();
  const monthStr = month.format('YYYY-MM');

  const [employees, setEmployees] = useState<Employee[]>([]);
  const [accounts, setAccounts] = useState<FundAccount[]>([]);
  const [glAccounts, setGlAccounts] = useState<{ code: string; name: string }[]>([]);
  const [settledLoans, setSettledLoans] = useState<FundDocument[]>([]);
  const [employeeId, setEmployeeId] = useState<number | undefined>(undefined);
  const [keyword, setKeyword] = useState('');
  const [onlyOutstanding, setOnlyOutstanding] = useState(false);
  const [ledger, setLedger] = useState<{ rows: AdvanceLedgerRow[]; total_outstanding: number } | null>(
    null,
  );
  const [loading, setLoading] = useState(false);
  const [exporting, setExporting] = useState(false);

  const [advanceModalOpen, setAdvanceModalOpen] = useState(false);
  const [advanceForm] = Form.useForm<AdvanceFormValues>();
  const [settlementModalOpen, setSettlementModalOpen] = useState(false);
  const [settlementForm] = Form.useForm<SettlementFormValues>();
  const watchMode = Form.useWatch('settlement_mode', settlementForm);
  const watchSettleEmployee = Form.useWatch('employee_id', settlementForm);
  const [saving, setSaving] = useState(false);

  const [commentAction, setCommentAction] = useState<CommentAction | null>(null);
  const [commentText, setCommentText] = useState('');
  const [reversalMonth, setReversalMonth] = useState<Dayjs | null>(null);
  const [reversalDate, setReversalDate] = useState<Dayjs | null>(null);

  const [timelineFor, setTimelineFor] = useState<number | null>(null);
  const [timeline, setTimeline] = useState<AdvanceSettlementLink[]>([]);
  const [timelineLoading, setTimelineLoading] = useState(false);

  useEffect(() => {
    getEmployees().then(setEmployees).catch(() => setEmployees([]));
    getFundAccounts({ is_active: true }).then(setAccounts).catch(() => setAccounts([]));
    getGlAccounts()
      .then((list) => setGlAccounts(list.filter((a) => a.is_active === 1)))
      .catch(() => setGlAccounts([]));
  }, []);

  const fetchData = useCallback(async () => {
    setLoading(true);
    try {
      const data = await getAdvanceLedger({
        employee_id: employeeId,
        keyword: keyword.trim() || undefined,
        only_outstanding: onlyOutstanding || undefined,
      });
      setLedger(data);
    } catch (e: unknown) {
      message.error('查询借款台账失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setLoading(false);
    }
  }, [employeeId, keyword, onlyOutstanding]);

  useEffect(() => {
    void fetchData();
  }, [fetchData]);

  const loadSettledLoans = useCallback(async () => {
    try {
      const docs = await getFundDocuments({ document_type: 'advance', status: 'settled' });
      setSettledLoans(docs);
    } catch {
      setSettledLoans([]);
    }
  }, []);

  const loadTimeline = useCallback(async (advanceId: number) => {
    setTimelineLoading(true);
    try {
      setTimeline(await getAdvanceSettlementLinks(advanceId));
    } catch (e: unknown) {
      message.error('查询核销时间线失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setTimelineLoading(false);
    }
  }, []);

  const openSettlementModal = () => {
    settlementForm.setFieldsValue({
      settlement_mode: 'cash_return',
      belong_month: monthStr,
      document_date: monthStr
        ? `${monthStr}-05`
        : dayjs().format('YYYY-MM-DD'),
      summary: '借款核销',
    });
    void loadSettledLoans();
    setSettlementModalOpen(true);
  };

  /** 台账中未结清的已发放借款，作为核销关联候选（与所选员工联动过滤） */
  const outstandingLoanOptions = useMemo(() => {
    const rows = (ledger?.rows ?? []).filter(
      (r) => r.advance_status === 'settled' && r.outstanding_amount > 0.005,
    );
    return rows
      .filter((r) => !watchSettleEmployee || r.employee_id === watchSettleEmployee)
      .map((r) => ({ value: r.advance_id, label: `${r.document_no}（余额 ${fmtMoney(r.outstanding_amount)}）` }));
  }, [ledger, watchSettleEmployee]);

  const submitAdvance = async (values: AdvanceFormValues) => {
    setSaving(true);
    try {
      const doc = await createFundDocument({
        document_type: 'advance',
        belong_month: values.belong_month,
        document_date: values.document_date,
        amount: Number(values.amount),
        summary: values.summary.trim(),
        employee_id: values.employee_id,
        source_account_id: values.source_account_id,
        due_date: values.due_date,
        counter_account_code: values.counter_account_code?.trim() || null,
      });
      message.success(`借款单 ${doc.document_no} 已创建；审批后经付款批次发放`);
      setAdvanceModalOpen(false);
      advanceForm.resetFields();
      void fetchData();
    } catch (e: unknown) {
      message.error('创建借款单失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setSaving(false);
    }
  };

  const submitSettlement = async (values: SettlementFormValues) => {
    setSaving(true);
    try {
      const loan = settledLoans.find((d) => d.id === values.advance_id);
      const doc = await createFundDocument({
        document_type: 'advance_settlement',
        belong_month: values.belong_month,
        document_date: values.document_date,
        amount: Number(values.amount),
        summary: values.summary.trim(),
        employee_id: values.employee_id,
        settlement_mode: values.settlement_mode,
        target_account_id:
          values.settlement_mode === 'cash_return' ? values.target_account_id ?? null : null,
        counter_account_code:
          values.settlement_mode === 'other' ? values.counter_account_code?.trim() || null : null,
        advance_allocations: [{ advance_id: values.advance_id, amount: Number(values.amount) }],
      });
      message.success(
        `核销单 ${doc.document_no} 已创建（${
          SETTLEMENT_MODE_LABEL[values.settlement_mode] ?? values.settlement_mode
        }）；需提交审批并结算后生效${loan ? '' : ''}`,
      );
      setSettlementModalOpen(false);
      settlementForm.resetFields();
      void fetchData();
      if (timelineFor) void loadTimeline(timelineFor);
    } catch (e: unknown) {
      message.error('创建核销单失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setSaving(false);
    }
  };

  const handleTimelineAction = async (
    docId: number,
    docNo: string,
    action: 'submit' | 'approve',
  ) => {
    try {
      if (action === 'submit') {
        await submitFundDocument(docId);
        message.success('核销单已提交');
      } else {
        setCommentAction({ type: 'approve', docId, docNo });
        setCommentText('同意');
      }
    } catch (e: unknown) {
      message.error(e instanceof Error ? e.message : String(e));
    }
  };

  const confirmComment = async () => {
    if (!commentAction) return;
    const text = commentText.trim();
    try {
      if (commentAction.type === 'approve') {
        if (!text) {
          message.warning('审批必须填写意见');
          return;
        }
        await approveFundDocument(commentAction.docId, text);
        message.success('审批通过；请结算核销单使其生效');
      } else if (commentAction.type === 'void') {
        if (!text) {
          message.warning('作废必须填写原因');
          return;
        }
        await voidFundDocument(commentAction.docId, text);
        message.success('已作废');
      } else if (commentAction.type === 'cancel-link') {
        if (!text) {
          message.warning('取消核销必须填写原因');
          return;
        }
        if (commentAction.settledSettlement && (!reversalMonth || !reversalDate)) {
          message.warning('该核销单已结算入账，取消核销需选择冲正归属月份与日期');
          return;
        }
        await cancelAdvanceSettlementLink({
          link_id: commentAction.link.id,
          reason: text,
          reversal_month: commentAction.settledSettlement ? reversalMonth?.format('YYYY-MM') : null,
          reversal_date: commentAction.settledSettlement ? reversalDate?.format('YYYY-MM-DD') : null,
        });
        message.success('已取消核销，借款未核销余额已恢复');
      }
      setCommentAction(null);
      void fetchData();
      if (timelineFor) void loadTimeline(timelineFor);
    } catch (e: unknown) {
      message.error(e instanceof Error ? e.message : String(e));
    }
  };

  const handleExport = async () => {
    const target = await save({
      defaultPath: `借款备用金台账_${dayjs().format('YYYYMMDD')}.xlsx`,
      filters: [{ name: '台账', extensions: ['xlsx'] }],
    });
    if (!target) return;
    setExporting(true);
    try {
      await exportAdvanceLedger(
        {
          employee_id: employeeId,
          keyword: keyword.trim() || undefined,
          only_outstanding: onlyOutstanding || undefined,
        },
        String(target),
      );
      message.success('借款台账已导出');
    } catch (e: unknown) {
      message.error('导出失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setExporting(false);
    }
  };

  /** 核销时间线单条记录的操作区（提交/审批/取消核销） */
  const renderLinkActions = (link: AdvanceSettlementLink) => {
    if (link.status !== 'active') {
      return <Tag>已取消</Tag>;
    }
    return (
      <Space size={4}>
        {link.settlement_status === 'draft' && (
          <Button
            size="small"
            type="link"
            onClick={() =>
              void handleTimelineAction(
                link.settlement_id,
                link.settlement_document_no ?? '',
                'submit',
              )
            }
          >
            提交
          </Button>
        )}
        {link.settlement_status === 'submitted' && (
          <Button
            size="small"
            type="link"
            onClick={() =>
              void handleTimelineAction(
                link.settlement_id,
                link.settlement_document_no ?? '',
                'approve',
              )
            }
          >
            审批
          </Button>
        )}
        <Tooltip title="取消核销：未结算核销单作废；已结算核销单冲正，借款余额恢复">
          <Button
            size="small"
            type="link"
            danger
            icon={<RollbackOutlined />}
            onClick={() =>
              setCommentAction({
                type: 'cancel-link',
                link,
                loanNo: '',
                settledSettlement: link.settlement_status === 'settled',
              })
            }
          >
            取消核销
          </Button>
        </Tooltip>
      </Space>
    );
  };

  const columns: ColumnsType<AdvanceLedgerRow> = [
    { title: '借款单号', dataIndex: 'document_no', key: 'document_no', width: 190 },
    {
      title: '员工',
      dataIndex: 'employee_name',
      key: 'employee_name',
      width: 100,
      render: (v: string | null) => v || '-',
    },
    {
      title: '部门',
      dataIndex: 'department',
      key: 'department',
      width: 110,
      render: (v: string | null) => v || '-',
    },
    { title: '借款日期', dataIndex: 'document_date', key: 'document_date', width: 110 },
    {
      title: '预计归还日',
      dataIndex: 'due_date',
      key: 'due_date',
      width: 110,
      render: (v: string | null) => v || '-',
    },
    {
      title: '借款金额',
      dataIndex: 'amount',
      key: 'amount',
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
    { title: '未清天数', dataIndex: 'days_outstanding', key: 'days_outstanding', width: 100 },
    {
      title: '逾期天数',
      dataIndex: 'overdue_days',
      key: 'overdue_days',
      width: 100,
      render: (v: number) =>
        v > 0 ? <span style={{ color: '#cf1322', fontWeight: 600 }}>{v}</span> : 0,
    },
    {
      title: '账龄',
      dataIndex: 'aging_bucket',
      key: 'aging_bucket',
      width: 100,
      render: (v: string) => <Tag color={AGING_COLOR[v] ?? 'default'}>{v}</Tag>,
    },
    {
      title: '状态',
      dataIndex: 'advance_status',
      key: 'advance_status',
      width: 90,
      render: (s: string) => (
        <Tag color={STATUS_COLOR[s] ?? 'default'}>{FUND_DOCUMENT_STATUS_LABEL[s] ?? s}</Tag>
      ),
    },
    {
      title: '操作',
      key: 'op',
      width: 150,
      render: (_, record) =>
        record.advance_status === 'draft' ? (
          <Space size={0}>
            <Button
              size="small"
              type="link"
              onClick={() =>
                setCommentAction({ type: 'void', docId: record.advance_id, docNo: record.document_no })
              }
            >
              作废
            </Button>
            <Button
              size="small"
              type="link"
              onClick={() => message.info('借款发放请经「付款批次」页面完成')}
            >
              发放指引
            </Button>
          </Space>
        ) : (
          <Button
            size="small"
            type="link"
            onClick={() => message.info('借款发放请经「付款批次」页面完成')}
          >
            发放指引
          </Button>
        ),
    },
  ];

  return (
    <div>
      <div className="page-header">
        <span className="page-title">借款备用金</span>
        <div className="page-header-actions">
          <Select
            placeholder="员工筛选"
            allowClear
            showSearch
            optionFilterProp="label"
            value={employeeId}
            onChange={setEmployeeId}
            style={{ width: 160 }}
            options={employees.map((e) => ({ value: e.id, label: e.name }))}
          />
          <Input.Search
            placeholder="单号/摘要/员工"
            allowClear
            style={{ width: 180 }}
            onSearch={setKeyword}
          />
          <Tooltip title="仅显示未核销余额大于 0 的借款">
            <Space size={4}>
              <span style={{ color: '#666' }}>仅未结清</span>
              <Switch size="small" checked={onlyOutstanding} onChange={setOnlyOutstanding} />
            </Space>
          </Tooltip>
          <Button icon={<ReloadOutlined />} loading={loading} onClick={() => void fetchData()}>
            刷新
          </Button>
          <Button
            type="primary"
            ghost
            icon={<PlusOutlined />}
            onClick={() => {
              advanceForm.setFieldsValue({
                belong_month: monthStr,
                document_date: `${monthStr}-05`,
                counter_account_code: ADVANCE_DEFAULT_GL,
              });
              setAdvanceModalOpen(true);
            }}
          >
            新增借款
          </Button>
          <Button type="primary" icon={<PlusOutlined />} onClick={openSettlementModal}>
            新增核销
          </Button>
          <Tooltip title={isSensitiveRevealed ? '' : '敏感导出需先在页面中解锁敏感数据'}>
            <Button
              icon={<DownloadOutlined />}
              disabled={!isSensitiveRevealed}
              loading={exporting}
              onClick={() => void handleExport()}
            >
              导出
            </Button>
          </Tooltip>
        </div>
      </div>

      <Spin spinning={loading}>
        <Row gutter={[16, 16]} className="mb-16">
          <Col xs={24} sm={12} lg={8}>
            <Card className="stat-card">
              <SensitiveStatistic title="借款总额" value={ledger?.rows.reduce((s, r) => s + r.amount, 0) ?? 0} />
            </Card>
          </Col>
          <Col xs={24} sm={12} lg={8}>
            <Card className="stat-card">
              <SensitiveStatistic
                title="累计已核销"
                value={ledger?.rows.reduce((s, r) => s + r.settled_amount, 0) ?? 0}
              />
            </Card>
          </Col>
          <Col xs={24} sm={12} lg={8}>
            <Card className="stat-card">
              <SensitiveStatistic title="未核销余额" value={ledger?.total_outstanding ?? 0} />
            </Card>
          </Col>
        </Row>

        <Card title="员工借款台账" extra={<span style={{ color: '#8c8c8c' }}>账龄按借款日至今未清天数分桶</span>}>
          <Table<AdvanceLedgerRow>
            rowKey="advance_id"
            columns={columns}
            dataSource={ledger?.rows ?? []}
            pagination={{ pageSize: 15, showSizeChanger: true }}
            scroll={{ x: 1530 }}
            expandable={{
              expandedRowRender: (record) => (
                <div style={{ padding: '4px 8px' }}>
                  <Descriptions size="small" column={3} className="mb-8">
                    <Descriptions.Item label="摘要">{record.summary}</Descriptions.Item>
                    <Descriptions.Item label="归属月份">{record.belong_month}</Descriptions.Item>
                    <Descriptions.Item label="员工">{record.employee_name ?? '-'}</Descriptions.Item>
                  </Descriptions>
                  <Button
                    size="small"
                    onClick={() => {
                      setTimelineFor(record.advance_id);
                      void loadTimeline(record.advance_id);
                    }}
                  >
                    {timelineFor === record.advance_id ? '刷新时间线' : '加载核销时间线'}
                  </Button>
                  {timelineFor === record.advance_id && (
                    <Spin spinning={timelineLoading}>
                      <Timeline
                        className="mt-8"
                        items={timeline.map((l) => ({
                          color: l.status === 'active' ? 'green' : 'gray',
                          children: (
                            <div>
                              <Space wrap size={8}>
                                <span style={{ fontWeight: 600 }}>
                                  {l.settlement_document_no ?? `核销单ID=${l.settlement_id}`}
                                </span>
                                <span>
                                  {l.settlement_mode
                                    ? SETTLEMENT_MODE_LABEL[l.settlement_mode] ?? l.settlement_mode
                                    : '-'}
                                </span>
                                <SensitiveText type="amount" value={l.allocated_amount} />
                                <Tag color={STATUS_COLOR[l.settlement_status ?? ''] ?? 'default'}>
                                  {l.settlement_status
                                    ? FUND_DOCUMENT_STATUS_LABEL[l.settlement_status] ??
                                      l.settlement_status
                                    : '-'}
                                </Tag>
                                {l.settlement_date && <span>核销日期 {l.settlement_date}</span>}
                                {renderLinkActions(l)}
                              </Space>
                              {l.cancel_reason && (
                                <div style={{ color: '#8c8c8c' }}>取消原因：{l.cancel_reason}</div>
                              )}
                            </div>
                          ),
                        }))}
                      />
                    </Spin>
                  )}
                </div>
              ),
            }}
          />
        </Card>
      </Spin>

      {/* 新增借款单 */}
      <Modal
        title="新增借款单"
        open={advanceModalOpen}
        onCancel={() => setAdvanceModalOpen(false)}
        onOk={() => advanceForm.submit()}
        confirmLoading={saving}
        destroyOnHidden
      >
        <Form<AdvanceFormValues>
          form={advanceForm}
          layout="vertical"
          onFinish={submitAdvance}
        >
          <Form.Item
            name="employee_id"
            label="员工"
            rules={[{ required: true, message: '请选择员工' }]}
          >
            <Select
              placeholder="选择员工"
              showSearch
              optionFilterProp="label"
              options={employees.map((e) => ({
                value: e.id,
                label: `${e.name}（${e.department ?? '-'}）`,
              }))}
            />
          </Form.Item>
          <Form.Item
            name="amount"
            label="借款金额"
            rules={[{ required: true, message: '请输入借款金额' }]}
          >
            <Input type="number" min={0.01} step="0.01" placeholder="0.00" />
          </Form.Item>
          <Form.Item name="belong_month" label="归属月份" rules={[{ required: true }]}>
            <Input placeholder="YYYY-MM" />
          </Form.Item>
          <Form.Item
            name="document_date"
            label="借款日期"
            rules={[{ required: true, message: '请选择借款日期' }]}
          >
            <Input placeholder="YYYY-MM-DD" />
          </Form.Item>
          <Form.Item
            name="due_date"
            label="预计归还日"
            rules={[{ required: true, message: '预计归还日必填（账龄与逾期统计依据）' }]}
          >
            <Input placeholder="YYYY-MM-DD" />
          </Form.Item>
          <Form.Item
            name="source_account_id"
            label="资金账户"
            rules={[{ required: true, message: '请选择资金流出账户' }]}
          >
            <Select
              placeholder="选择资金账户"
              options={accounts.map((a) => ({
                value: a.id,
                label: `${a.name}（${a.account_code}）`,
              }))}
            />
          </Form.Item>
          <Form.Item
            name="counter_account_code"
            label="其他应收款科目"
            extra="缺省 1221 其他应收款"
          >
            <Select
              allowClear
              showSearch
              optionFilterProp="label"
              placeholder="1221 其他应收款"
              options={glAccounts.map((a) => ({ value: a.code, label: `${a.code} ${a.name}` }))}
            />
          </Form.Item>
          <Form.Item name="summary" label="摘要" rules={[{ required: true, message: '请填写摘要' }]}>
            <Input placeholder="如：员工出差借款" />
          </Form.Item>
        </Form>
      </Modal>

      {/* 新增核销单 */}
      <Modal
        title="新增借款核销单"
        open={settlementModalOpen}
        onCancel={() => setSettlementModalOpen(false)}
        onOk={() => settlementForm.submit()}
        confirmLoading={saving}
        destroyOnHidden
      >
        <Form<SettlementFormValues>
          form={settlementForm}
          layout="vertical"
          onFinish={submitSettlement}
        >
          <Form.Item
            name="settlement_mode"
            label="核销方式"
            rules={[{ required: true, message: '请选择核销方式' }]}
            extra="现金归还走资金回流；报销抵扣/工资扣回为无资金流动的科目对转"
          >
            <Select
              options={Object.entries(SETTLEMENT_MODE_LABEL).map(([value, label]) => ({ value, label }))}
              onChange={() => settlementForm.setFieldsValue({ advance_id: undefined, amount: undefined })}
            />
          </Form.Item>
          <Form.Item
            name="employee_id"
            label="员工"
            rules={[{ required: true, message: '请选择员工' }]}
          >
            <Select
              placeholder="选择员工"
              showSearch
              optionFilterProp="label"
              options={employees.map((e) => ({ value: e.id, label: e.name }))}
              onChange={() => settlementForm.setFieldsValue({ advance_id: undefined })}
            />
          </Form.Item>
          <Form.Item
            name="advance_id"
            label="关联借款单"
            rules={[{ required: true, message: '请选择要核销的借款单（仅列出已发放借款）' }]}
          >
            <Select
              placeholder="选择借款单（仅已发放）"
              options={outstandingLoanOptions}
              onChange={(advanceId: number) => {
                const row = (ledger?.rows ?? []).find((r) => r.advance_id === advanceId);
                if (row) settlementForm.setFieldsValue({ amount: row.outstanding_amount });
              }}
            />
          </Form.Item>
          <Form.Item
            name="amount"
            label="本次核销金额"
            rules={[{ required: true, message: '请输入核销金额' }]}
            extra="多次核销累计不得超过借款金额"
          >
            <Input type="number" min={0.01} step="0.01" placeholder="0.00" />
          </Form.Item>
          <Form.Item noStyle shouldUpdate={(p, c) => p.settlement_mode !== c.settlement_mode}>
            {() =>
              watchMode === 'cash_return' ? (
                <Form.Item
                  name="target_account_id"
                  label="资金回流账户"
                  rules={[{ required: true, message: '现金归还必须选择资金回流账户' }]}
                >
                  <Select
                    placeholder="选择资金账户"
                    options={accounts.map((a) => ({
                      value: a.id,
                      label: `${a.name}（${a.account_code}）`,
                    }))}
                  />
                </Form.Item>
              ) : watchMode === 'other' ? (
                <Form.Item
                  name="counter_account_code"
                  label="借方科目"
                  rules={[{ required: true, message: '其他核销必须指定借方科目' }]}
                  extra="贷方固定为关联借款的其他应收款科目"
                >
                  <Select
                    showSearch
                    optionFilterProp="label"
                    placeholder="选择借方科目"
                    options={glAccounts.map((a) => ({ value: a.code, label: `${a.code} ${a.name}` }))}
                  />
                </Form.Item>
              ) : null
            }
          </Form.Item>
          <Form.Item name="belong_month" label="归属月份" rules={[{ required: true }]}>
            <Input placeholder="YYYY-MM" />
          </Form.Item>
          <Form.Item
            name="document_date"
            label="核销日期"
            rules={[{ required: true, message: '请选择核销日期' }]}
          >
            <Input placeholder="YYYY-MM-DD" />
          </Form.Item>
          <Form.Item name="summary" label="摘要" rules={[{ required: true, message: '请填写摘要' }]}>
            <Input />
          </Form.Item>
        </Form>
      </Modal>

      {/* 审批/作废/取消核销 意见弹窗 */}
      <Modal
        title={
          commentAction?.type === 'approve'
            ? '审批意见'
            : commentAction?.type === 'void'
              ? `作废借款单 ${commentAction.docNo}`
              : '取消核销'
        }
        open={commentAction !== null}
        onCancel={() => setCommentAction(null)}
        onOk={() => void confirmComment()}
        okText={commentAction?.type === 'approve' ? '通过' : '确认'}
      >
        {commentAction?.type === 'cancel-link' && (
          <div style={{ marginBottom: 12 }}>
            {commentAction.settledSettlement
              ? '该核销单已结算入账，取消核销将生成冲正凭证（红字冲销），请选择冲正归属月份与日期。'
              : '取消核销将作废该核销单（未结算，无凭证），借款未核销余额恢复。'}
          </div>
        )}
        {commentAction?.type === 'cancel-link' && commentAction.settledSettlement && (
          <Space className="mb-12">
            <DatePicker
              picker="month"
              placeholder="冲正归属月份"
              value={reversalMonth}
              onChange={setReversalMonth}
            />
            <DatePicker placeholder="冲正日期" value={reversalDate} onChange={setReversalDate} />
          </Space>
        )}
        <Input.TextArea
          value={commentText}
          onChange={(e) => setCommentText(e.target.value)}
          rows={3}
          placeholder={
            commentAction?.type === 'approve'
              ? '审批意见（必填）'
              : commentAction?.type === 'void'
                ? '作废原因（必填）'
                : '取消核销原因（必填）'
          }
        />
      </Modal>
    </div>
  );
};

export default Advances;
