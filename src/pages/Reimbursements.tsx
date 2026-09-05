import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Button,
  Card,
  Col,
  DatePicker,
  Drawer,
  Form,
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
  Timeline,
  Tooltip,
  Typography,
  message,
} from 'antd';
import {
  CheckCircleOutlined,
  DeleteOutlined,
  EditOutlined,
  EyeOutlined,
  PlusOutlined,
  ReloadOutlined,
  RollbackOutlined,
  SendOutlined,
  StopOutlined,
} from '@ant-design/icons';
import dayjs from 'dayjs';
import {
  approveReimbursementClaim,
  deleteReimbursementClaim,
  getEmployees,
  getInvoiceExpenseTypes,
  getOperatorProfiles,
  getReimbursementInvoices,
  listApprovalEvents,
  queryInvoices,
  queryReimbursementClaims,
  rejectReimbursementClaim,
  saveReimbursementClaim,
  submitReimbursementClaim,
  unapproveReimbursementClaim,
  withdrawReimbursementClaim,
} from '@/api';
import type {
  ApprovalEvent,
  Employee,
  Invoice,
  InvoiceExpenseType,
  OperatorProfile,
  PaymentStatus,
  ReimbursementClaim,
  ReimbursementClaimInput,
  ReimbursementInvoice,
  ReimbursementStatus,
} from '@/types';
import { SensitiveText } from '@/components/SensitiveText';
import { SensitiveStatistic } from '@/components/SensitiveStatistic';
import { useBusinessMonth } from '@/contexts/BusinessMonthContext';
import { useOperator } from '@/contexts/OperatorContext';

const { TextArea } = Input;
const { Text, Title } = Typography;

const statusMap: Record<ReimbursementStatus, { text: string; color: string }> = {
  draft: { text: '草稿', color: 'default' },
  submitted: { text: '待审批', color: 'gold' },
  approved: { text: '已审批', color: 'blue' },
  rejected: { text: '已驳回', color: 'red' },
  void: { text: '已作废', color: 'default' },
};

const paymentMap: Record<PaymentStatus, { text: string; color: string }> = {
  unpaid: { text: '未付款', color: 'red' },
  paid: { text: '已付款', color: 'green' },
};

const approvalActionLabel: Record<string, string> = {
  submit: '提交',
  approve: '审批通过',
  reject: '驳回',
  withdraw: '撤回',
  unapprove: '反审批',
  void: '作废',
};

// 需要填写意见/原因的审批动作（对应后端 require_comment 校验）
type CommentAction = 'approve' | 'reject' | 'unapprove' | 'void';

const commentActionMeta: Record<CommentAction, { title: string; label: string; required: string }> = {
  approve: { title: '审批通过', label: '审批意见', required: '请填写审批意见' },
  reject: { title: '驳回', label: '驳回意见', required: '请填写驳回意见' },
  unapprove: { title: '反审批', label: '反审批原因', required: '请填写反审批原因（将作废原计提凭证）' },
  void: { title: '作废报销单', label: '作废原因', required: '请填写作废原因' },
};

const Reimbursements: React.FC = () => {
  const [claims, setClaims] = useState<ReimbursementClaim[]>([]);
  const [employees, setEmployees] = useState<Employee[]>([]);
  const [expenseTypes, setExpenseTypes] = useState<InvoiceExpenseType[]>([]);
  const [operators, setOperators] = useState<OperatorProfile[]>([]);
  const [loading, setLoading] = useState(false);
  const { month, setMonth } = useBusinessMonth();
  const { operator } = useOperator();
  const [employeeFilter, setEmployeeFilter] = useState<number | undefined>(undefined);
  const [statusFilter, setStatusFilter] = useState<ReimbursementStatus | undefined>(undefined);
  const [paymentFilter, setPaymentFilter] = useState<PaymentStatus | undefined>(undefined);
  const [keyword, setKeyword] = useState('');

  const [modalOpen, setModalOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const [invoiceLoading, setInvoiceLoading] = useState(false);
  const [availableInvoices, setAvailableInvoices] = useState<Invoice[]>([]);
  const [selectedInvoiceIds, setSelectedInvoiceIds] = useState<number[]>([]);
  const [form, setForm] = useState<ReimbursementClaimInput>({
    belong_month: dayjs().format('YYYY-MM'),
    title: '',
    invoice_ids: [],
  });

  const [detailClaim, setDetailClaim] = useState<ReimbursementClaim | null>(null);
  const [detailInvoices, setDetailInvoices] = useState<ReimbursementInvoice[]>([]);
  const [detailEvents, setDetailEvents] = useState<ApprovalEvent[]>([]);

  const [commentAction, setCommentAction] = useState<{ type: CommentAction; claim: ReimbursementClaim } | null>(null);
  const [commentSaving, setCommentSaving] = useState(false);
  const [commentForm] = Form.useForm<{ comment: string }>();

  const employeeOptions = employees.map((employee) => ({
    value: employee.id,
    label: `${employee.name} (${employee.employee_no})`,
  }));

  const operatorName = useCallback(
    (id: number | null) => operators.find((op) => op.id === id)?.name ?? (id ? `操作人#${id}` : '—'),
    [operators],
  );

  const fetchClaims = useCallback(async () => {
    setLoading(true);
    try {
      setClaims(await queryReimbursementClaims({
        belong_month: month.format('YYYY-MM'),
        employee_id: employeeFilter,
        status: statusFilter,
        payment_status: paymentFilter,
        keyword: keyword || undefined,
      }));
    } catch (e: unknown) {
      message.error('查询报销单失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setLoading(false);
    }
  }, [month, employeeFilter, statusFilter, paymentFilter, keyword]);

  const fetchStaticData = useCallback(async () => {
    try {
      const [employeeData, expenseData] = await Promise.all([getEmployees(), getInvoiceExpenseTypes()]);
      setEmployees(employeeData);
      setExpenseTypes(expenseData);
    } catch (e: unknown) {
      message.error('基础数据加载失败: ' + (e instanceof Error ? e.message : String(e)));
    }
    // 操作人仅用于审批轨迹署名展示，加载失败不影响主流程
    try {
      setOperators(await getOperatorProfiles());
    } catch {
      setOperators([]);
    }
  }, []);

  const fetchAvailableInvoices = useCallback(async () => {
    if (!modalOpen || !form.employee_id || !form.belong_month) {
      setAvailableInvoices([]);
      return;
    }
    setInvoiceLoading(true);
    try {
      setAvailableInvoices(await queryInvoices({
        belong_month: form.belong_month,
        employee_id: form.employee_id,
        status: 'normal',
      }));
    } catch (e: unknown) {
      message.error('加载可报销发票失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setInvoiceLoading(false);
    }
  }, [modalOpen, form.employee_id, form.belong_month]);

  useEffect(() => { fetchStaticData(); }, [fetchStaticData]);
  useEffect(() => { fetchClaims(); }, [fetchClaims]);
  useEffect(() => { fetchAvailableInvoices(); }, [fetchAvailableInvoices]);

  const totalAmount = claims.reduce((sum, item) => sum + item.total_amount, 0);
  const unpaidCount = claims.filter((item) => item.status === 'approved' && item.payment_status !== 'paid').length;
  const pendingCount = claims.filter((item) => item.status === 'draft' || item.status === 'submitted').length;

  const selectedTotal = useMemo(
    () => availableInvoices
      .filter((invoice) => selectedInvoiceIds.includes(invoice.id))
      .reduce((sum, invoice) => sum + (invoice.total_amount || 0), 0),
    [availableInvoices, selectedInvoiceIds],
  );

  const openCreate = () => {
    const defaultMonth = month.format('YYYY-MM');
    setForm({
      belong_month: defaultMonth,
      employee_id: employeeFilter,
      title: `${defaultMonth} 报销单`,
      invoice_ids: [],
    });
    setSelectedInvoiceIds([]);
    setAvailableInvoices([]);
    setModalOpen(true);
  };

  const openEdit = async (claim: ReimbursementClaim) => {
    try {
      const invoices = await getReimbursementInvoices(claim.id);
      setForm({
        id: claim.id,
        employee_id: claim.employee_id,
        belong_month: claim.belong_month,
        title: claim.title,
        invoice_ids: invoices.map((invoice) => invoice.invoice_id),
        remark: claim.remark,
      });
      setSelectedInvoiceIds(invoices.map((invoice) => invoice.invoice_id));
      setModalOpen(true);
    } catch (e: unknown) {
      message.error('打开报销单失败: ' + (e instanceof Error ? e.message : String(e)));
    }
  };

  const openDetail = async (claim: ReimbursementClaim) => {
    setDetailClaim(claim);
    setDetailEvents([]);
    try {
      const [invoices, events] = await Promise.all([
        getReimbursementInvoices(claim.id),
        listApprovalEvents('reimbursement_claim', claim.id),
      ]);
      setDetailInvoices(invoices);
      setDetailEvents(events);
    } catch (e: unknown) {
      message.error('获取报销明细失败: ' + (e instanceof Error ? e.message : String(e)));
    }
  };

  const handleSave = async () => {
    if (!form.employee_id) {
      message.warning('请选择报销人');
      return;
    }
    if (!form.title.trim()) {
      message.warning('请输入报销单标题');
      return;
    }
    if (selectedInvoiceIds.length === 0) {
      message.warning('请选择至少一张发票');
      return;
    }
    setSaving(true);
    try {
      await saveReimbursementClaim({ ...form, invoice_ids: selectedInvoiceIds });
      message.success(form.id ? '报销单已更新' : '报销单已创建（草稿，提交后进入审批）');
      setModalOpen(false);
      fetchClaims();
    } catch (e: unknown) {
      message.error('保存报销单失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setSaving(false);
    }
  };

  // ---------- 状态机操作（Task 15，spec 5.2） ----------

  const runTransition = async (
    fn: () => Promise<ReimbursementClaim>,
    success: string,
  ) => {
    try {
      await fn();
      message.success(success);
      fetchClaims();
    } catch (e: unknown) {
      message.error('操作失败: ' + (e instanceof Error ? e.message : String(e)));
    }
  };

  const handleCommentOk = async () => {
    if (!commentAction) return;
    let values: { comment: string };
    try {
      values = await commentForm.validateFields();
    } catch {
      return;
    }
    setCommentSaving(true);
    const { type, claim } = commentAction;
    const comment = values.comment.trim();
    try {
      if (type === 'approve') {
        await approveReimbursementClaim(claim.id, comment);
      } else if (type === 'reject') {
        await rejectReimbursementClaim(claim.id, comment);
      } else if (type === 'unapprove') {
        await unapproveReimbursementClaim(claim.id, comment);
      } else {
        await deleteReimbursementClaim(claim.id, comment);
      }
      message.success(`${commentActionMeta[type].title}成功`);
      setCommentAction(null);
      fetchClaims();
    } catch (e: unknown) {
      message.error('操作失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setCommentSaving(false);
    }
  };

  const openCommentModal = (type: CommentAction, claim: ReimbursementClaim) => {
    setCommentAction({ type, claim });
    commentForm.resetFields();
  };

  const claimColumns = [
    { title: '单号', dataIndex: 'claim_no', key: 'claim_no', width: 180, fixed: 'left' as const },
    { title: '标题', dataIndex: 'title', key: 'title', ellipsis: true },
    { title: '报销人', dataIndex: 'employee_name', key: 'employee_name', width: 100 },
    { title: '部门', dataIndex: 'department', key: 'department', width: 100 },
    { title: '月份', dataIndex: 'belong_month', key: 'belong_month', width: 100 },
    {
      title: '金额',
      dataIndex: 'total_amount',
      key: 'total_amount',
      width: 130,
      align: 'right' as const,
      render: (value: number) => <SensitiveText type="amount" value={value} />,
    },
    { title: '发票', dataIndex: 'invoice_count', key: 'invoice_count', width: 80, align: 'right' as const },
    {
      title: '审批',
      dataIndex: 'status',
      key: 'status',
      width: 90,
      render: (status: ReimbursementStatus) => <Tag color={statusMap[status]?.color}>{statusMap[status]?.text ?? status}</Tag>,
    },
    {
      title: '付款',
      dataIndex: 'payment_status',
      key: 'payment_status',
      width: 90,
      render: (status: PaymentStatus) => (
        <Tooltip title="付款只能通过付款批次完成（资金出纳 → 付款批次）">
          <Tag color={paymentMap[status]?.color}>{paymentMap[status]?.text ?? status}</Tag>
        </Tooltip>
      ),
    },
    { title: '付款日期', dataIndex: 'payment_date', key: 'payment_date', width: 110 },
    {
      title: '操作',
      key: 'actions',
      width: 280,
      fixed: 'right' as const,
      render: (_: unknown, claim: ReimbursementClaim) => (
        <Space size={4}>
          <Button size="small" icon={<EyeOutlined />} onClick={() => openDetail(claim)} />
          <Tooltip title={claim.status === 'draft' ? '编辑' : '仅草稿可编辑；submitted 后须先撤回/反审批'}>
            <Button
              size="small"
              icon={<EditOutlined />}
              disabled={claim.status !== 'draft'}
              onClick={() => openEdit(claim)}
            />
          </Tooltip>
          {claim.status === 'draft' && (
            <Button
              size="small"
              icon={<SendOutlined />}
              onClick={() => runTransition(
                () => submitReimbursementClaim(claim.id),
                '报销单已提交，等待审批',
              )}
            >
              提交
            </Button>
          )}
          {claim.status === 'submitted' && (
            <Button
              size="small"
              icon={<CheckCircleOutlined />}
              onClick={() => openCommentModal('approve', claim)}
            >
              审批
            </Button>
          )}
          {claim.status === 'submitted' && (
            <Button
              size="small"
              danger
              icon={<StopOutlined />}
              onClick={() => openCommentModal('reject', claim)}
            >
              驳回
            </Button>
          )}
          {(claim.status === 'submitted' || claim.status === 'rejected') && (
            <Popconfirm
              title="撤回后将回到草稿，可修改后重新提交，确认撤回？"
              okText="撤回"
              cancelText="取消"
              onConfirm={() => runTransition(
                () => withdrawReimbursementClaim(claim.id),
                '报销单已撤回（草稿）',
              )}
            >
              <Button size="small" icon={<RollbackOutlined />}>撤回</Button>
            </Popconfirm>
          )}
          {claim.status === 'approved' && (
            <Tooltip title="已审批后修改附件/发票须先反审批，将填写原因并作废原计提凭证">
              <Button size="small" onClick={() => openCommentModal('unapprove', claim)}>
                反审批
              </Button>
            </Tooltip>
          )}
          {claim.status !== 'void' && (
            <Button
              size="small"
              danger
              icon={<DeleteOutlined />}
              onClick={() => openCommentModal('void', claim)}
            />
          )}
        </Space>
      ),
    },
  ];

  const invoiceColumns = [
    { title: '发票号码', dataIndex: 'invoice_number', key: 'invoice_number', width: 180 },
    { title: '开票日期', dataIndex: 'issue_date', key: 'issue_date', width: 110 },
    { title: '销售方', dataIndex: 'seller_name', key: 'seller_name', ellipsis: true },
    {
      title: '费用类型',
      dataIndex: 'expense_type_code',
      key: 'expense_type_code',
      width: 110,
      render: (code?: string) => expenseTypes.find((item) => item.code === code)?.name ?? code ?? '-',
    },
    {
      title: '金额',
      dataIndex: 'total_amount',
      key: 'total_amount',
      width: 120,
      align: 'right' as const,
      render: (value: number) => <SensitiveText type="amount" value={value} />,
    },
  ];

  return (
    <div>
      <div className="page-header">
        <span className="page-title">报销管理</span>
        <div className="page-header-actions">
          {operator && <Text type="secondary">当前操作人：{operator.name}</Text>}
          <Button icon={<ReloadOutlined />} onClick={fetchClaims} loading={loading}>刷新</Button>
          <Button type="primary" icon={<PlusOutlined />} onClick={openCreate}>新增报销单</Button>
        </div>
      </div>

      <Row gutter={[16, 16]} className="mb-16">
        <Col xs={24} sm={8}>
          <Card className="stat-card"><Statistic title="报销单数" value={claims.length} /></Card>
        </Col>
        <Col xs={24} sm={8}>
          <Card className="stat-card"><SensitiveStatistic title="报销金额" value={totalAmount} /></Card>
        </Col>
        <Col xs={24} sm={8}>
          <Card className="stat-card">
            <Statistic
              title="待处理"
              value={pendingCount + unpaidCount}
              suffix={`审批 ${pendingCount} / 待付款 ${unpaidCount}`}
            />
          </Card>
        </Col>
      </Row>

      <Card style={{ marginBottom: 16 }}>
        <Space wrap>
          <DatePicker
            picker="month"
            allowClear={false}
            value={month}
            onChange={(value) => value && setMonth(value)}
            placeholder="归属月份"
          />
          <Select
            style={{ width: 180 }}
            allowClear
            showSearch
            placeholder="报销人"
            value={employeeFilter}
            onChange={setEmployeeFilter}
            options={employeeOptions}
          />
          <Select
            style={{ width: 140 }}
            allowClear
            placeholder="审批状态"
            value={statusFilter}
            onChange={setStatusFilter}
            options={[
              { value: 'draft', label: '草稿' },
              { value: 'submitted', label: '待审批' },
              { value: 'approved', label: '已审批' },
              { value: 'rejected', label: '已驳回' },
            ]}
          />
          <Select
            style={{ width: 130 }}
            allowClear
            placeholder="付款状态"
            value={paymentFilter}
            onChange={setPaymentFilter}
            options={[
              { value: 'unpaid', label: '未付款' },
              { value: 'paid', label: '已付款' },
            ]}
          />
          <Input.Search
            style={{ width: 260 }}
            allowClear
            placeholder="单号/标题/备注/报销人"
            value={keyword}
            onChange={(e) => setKeyword(e.target.value)}
            onSearch={fetchClaims}
          />
          <Button type="primary" onClick={fetchClaims}>查询</Button>
        </Space>
      </Card>

      <Card>
        <Table
          rowKey="id"
          columns={claimColumns}
          dataSource={claims}
          loading={loading}
          size="small"
          pagination={{ pageSize: 20, showSizeChanger: true, showTotal: (t) => `共 ${t} 条` }}
          scroll={{ x: 1400 }}
        />
      </Card>

      <Modal
        title={form.id ? '编辑报销单' : '新增报销单'}
        open={modalOpen}
        onCancel={() => setModalOpen(false)}
        onOk={handleSave}
        okText="保存"
        cancelText="取消"
        width={980}
        okButtonProps={{ loading: saving }}
      >
        <Form layout="vertical">
          <Row gutter={16}>
            <Col span={8}>
              <Form.Item label="归属月份" required>
                <DatePicker
                  picker="month"
                  allowClear={false}
                  value={dayjs(form.belong_month)}
                  style={{ width: '100%' }}
                  onChange={(value) => setForm((prev) => ({
                    ...prev,
                    belong_month: value ? value.format('YYYY-MM') : prev.belong_month,
                  }))}
                />
              </Form.Item>
            </Col>
            <Col span={8}>
              <Form.Item label="报销人" required>
                <Select
                  showSearch
                  placeholder="选择报销人"
                  value={form.employee_id}
                  onChange={(value) => {
                    setSelectedInvoiceIds([]);
                    setForm((prev) => ({ ...prev, employee_id: value }));
                  }}
                  options={employeeOptions}
                />
              </Form.Item>
            </Col>
            <Col span={8}>
              {/* spec 5.2：审批状态不可在表单中编辑，只能经列表操作流转（新增恒为草稿） */}
              <Form.Item label="审批状态">
                <Input value="草稿（提交后经审批流转）" disabled />
              </Form.Item>
            </Col>
          </Row>
          <Form.Item label="标题" required>
            <Input value={form.title} onChange={(e) => setForm((prev) => ({ ...prev, title: e.target.value }))} />
          </Form.Item>
          <Form.Item label="备注">
            <TextArea rows={2} value={form.remark} onChange={(e) => setForm((prev) => ({ ...prev, remark: e.target.value }))} />
          </Form.Item>
        </Form>

        <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 8 }}>
          <Space>
            <strong>发票明细</strong>
            <Tag>{selectedInvoiceIds.length} 张</Tag>
            <Tag color="blue"><SensitiveText type="amount" value={selectedTotal} revealable={false} /></Tag>
          </Space>
          <Button size="small" onClick={fetchAvailableInvoices} loading={invoiceLoading}>重新加载</Button>
        </div>
        <Spin spinning={invoiceLoading}>
          <Table
            rowKey="id"
            columns={invoiceColumns}
            dataSource={availableInvoices}
            size="small"
            pagination={{ pageSize: 8 }}
            rowSelection={{
              selectedRowKeys: selectedInvoiceIds,
              onChange: (keys) => setSelectedInvoiceIds(keys.map(Number)),
            }}
            scroll={{ x: 760 }}
          />
        </Spin>
      </Modal>

      <Modal
        title={commentAction ? commentActionMeta[commentAction.type].title : ''}
        open={!!commentAction}
        onCancel={() => setCommentAction(null)}
        onOk={handleCommentOk}
        okText="确认"
        cancelText="取消"
        confirmLoading={commentSaving}
        destroyOnHidden
      >
        {commentAction && (
          <>
            <p>
              <Text type="secondary">
                报销单 {commentAction.claim.claim_no}（金额 {commentAction.claim.total_amount.toFixed(2)}）
                {operator && <> · 当前操作人：{operator.name}</>}
              </Text>
            </p>
            {commentAction.type === 'unapprove' && (
              <p>
                <Text type="warning">
                  反审批将作废原计提凭证并使报销单回到草稿；修改附件/发票后须重新提交审批。
                </Text>
              </p>
            )}
            <Form form={commentForm} layout="vertical">
              <Form.Item
                name="comment"
                label={commentActionMeta[commentAction.type].label}
                rules={[{ required: true, whitespace: true, message: commentActionMeta[commentAction.type].required }]}
              >
                <TextArea rows={3} placeholder={commentActionMeta[commentAction.type].required} />
              </Form.Item>
            </Form>
          </>
        )}
      </Modal>

      <Drawer
        title={detailClaim ? `报销单 ${detailClaim.claim_no}` : '报销单详情'}
        open={!!detailClaim}
        onClose={() => {
          setDetailClaim(null);
          setDetailInvoices([]);
          setDetailEvents([]);
        }}
        width={720}
      >
        {detailClaim && (
          <div>
            <Row gutter={16} className="mb-16">
              <Col span={8}><SensitiveStatistic title="金额" value={detailClaim.total_amount} /></Col>
              <Col span={8}><Statistic title="发票张数" value={detailClaim.invoice_count} /></Col>
              <Col span={8}><Statistic title="付款状态" value={paymentMap[detailClaim.payment_status]?.text} /></Col>
            </Row>
            <p><b>审批状态：</b>
              <Tag color={statusMap[detailClaim.status]?.color}>
                {statusMap[detailClaim.status]?.text ?? detailClaim.status}
              </Tag>
            </p>
            <p><b>标题：</b>{detailClaim.title}</p>
            <p><b>报销人：</b>{detailClaim.employee_name || '-'} / {detailClaim.department || '-'}</p>
            <p><b>归属月份：</b>{detailClaim.belong_month}</p>
            <p><b>备注：</b>{detailClaim.remark || '-'}</p>
            <Table
              rowKey="id"
              columns={invoiceColumns}
              dataSource={detailInvoices.map((invoice) => ({
                id: invoice.invoice_id,
                invoice_number: invoice.invoice_number,
                issue_date: invoice.issue_date,
                seller_name: invoice.seller_name,
                expense_type_code: invoice.expense_type_code,
                total_amount: invoice.total_amount,
              }))}
              size="small"
              pagination={false}
              scroll={{ x: 650 }}
            />

            <Title level={5} style={{ marginTop: 24 }}>审批轨迹</Title>
            {detailEvents.length === 0 ? (
              <Text type="secondary">尚无审批记录（草稿未提交或历史数据）</Text>
            ) : (
              <Timeline
                items={detailEvents.map((e) => ({
                  color:
                    e.action === 'reject' || e.action === 'void'
                      ? 'red'
                      : e.action === 'approve'
                        ? 'green'
                        : 'blue',
                  content: (
                    <div key={e.id}>
                      <div>
                        <Text strong>{approvalActionLabel[e.action] ?? e.action}</Text>
                        <Text type="secondary">
                          {' '}
                          {statusMap[(e.from_status ?? 'draft') as ReimbursementStatus]?.text ?? e.from_status ?? '—'} →{' '}
                          {statusMap[(e.to_status ?? 'draft') as ReimbursementStatus]?.text ?? e.to_status ?? '—'}
                        </Text>
                      </div>
                      <Text type="secondary">
                        {operatorName(e.operator_id)} · {dayjs(e.created_at).format('YYYY-MM-DD HH:mm')}
                      </Text>
                      {e.comment && <div>意见：{e.comment}</div>}
                    </div>
                  ),
                }))}
              />
            )}
          </div>
        )}
      </Drawer>
    </div>
  );
};

export default Reimbursements;
