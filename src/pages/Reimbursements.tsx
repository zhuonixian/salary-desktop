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
  message,
} from 'antd';
import {
  CheckCircleOutlined,
  DeleteOutlined,
  EditOutlined,
  EyeOutlined,
  PayCircleOutlined,
  PlusOutlined,
  ReloadOutlined,
  SendOutlined,
  StopOutlined,
} from '@ant-design/icons';
import dayjs from 'dayjs';
import {
  deleteReimbursementClaim,
  getEmployees,
  getInvoiceExpenseTypes,
  getReimbursementInvoices,
  queryInvoices,
  queryReimbursementClaims,
  saveReimbursementClaim,
  updateReimbursementClaimStatus,
} from '@/api';
import type {
  Employee,
  Invoice,
  InvoiceExpenseType,
  PaymentStatus,
  ReimbursementClaim,
  ReimbursementClaimInput,
  ReimbursementInvoice,
  ReimbursementStatus,
} from '@/types';
import { SensitiveText } from '@/components/SensitiveText';
import { SensitiveStatistic } from '@/components/SensitiveStatistic';
import { useBusinessMonth } from '@/contexts/BusinessMonthContext';

const { TextArea } = Input;

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

const Reimbursements: React.FC = () => {
  const [claims, setClaims] = useState<ReimbursementClaim[]>([]);
  const [employees, setEmployees] = useState<Employee[]>([]);
  const [expenseTypes, setExpenseTypes] = useState<InvoiceExpenseType[]>([]);
  const [loading, setLoading] = useState(false);
  const { month, setMonth } = useBusinessMonth();
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
    status: 'draft',
    payment_status: 'unpaid',
  });

  const [detailClaim, setDetailClaim] = useState<ReimbursementClaim | null>(null);
  const [detailInvoices, setDetailInvoices] = useState<ReimbursementInvoice[]>([]);

  const employeeOptions = employees.map((employee) => ({
    value: employee.id,
    label: `${employee.name} (${employee.employee_no})`,
  }));

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
      status: 'draft',
      payment_status: 'unpaid',
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
        status: claim.status,
        payment_status: claim.payment_status,
        payment_date: claim.payment_date,
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
    try {
      setDetailInvoices(await getReimbursementInvoices(claim.id));
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
      message.success(form.id ? '报销单已更新' : '报销单已创建');
      setModalOpen(false);
      fetchClaims();
    } catch (e: unknown) {
      message.error('保存报销单失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setSaving(false);
    }
  };

  const updateStatus = async (
    claim: ReimbursementClaim,
    status?: ReimbursementStatus,
    paymentStatus?: PaymentStatus,
    paymentDate?: string,
  ) => {
    try {
      await updateReimbursementClaimStatus(claim.id, status, paymentStatus, paymentDate);
      message.success('状态已更新');
      fetchClaims();
    } catch (e: unknown) {
      message.error('更新状态失败: ' + (e instanceof Error ? e.message : String(e)));
    }
  };

  const handleDelete = async (claim: ReimbursementClaim) => {
    try {
      await deleteReimbursementClaim(claim.id);
      message.success('报销单已作废');
      fetchClaims();
    } catch (e: unknown) {
      message.error('作废失败: ' + (e instanceof Error ? e.message : String(e)));
    }
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
      render: (status: PaymentStatus) => <Tag color={paymentMap[status]?.color}>{paymentMap[status]?.text ?? status}</Tag>,
    },
    { title: '付款日期', dataIndex: 'payment_date', key: 'payment_date', width: 110 },
    {
      title: '操作',
      key: 'actions',
      width: 260,
      fixed: 'right' as const,
      render: (_: unknown, claim: ReimbursementClaim) => (
        <Space size={4}>
          <Button size="small" icon={<EyeOutlined />} onClick={() => openDetail(claim)} />
          <Button
            size="small"
            icon={<EditOutlined />}
            disabled={claim.payment_status === 'paid'}
            onClick={() => openEdit(claim)}
          />
          {claim.status === 'draft' && (
            <Button size="small" icon={<SendOutlined />} onClick={() => updateStatus(claim, 'submitted')}>
              提交
            </Button>
          )}
          {(claim.status === 'draft' || claim.status === 'submitted') && (
            <Button size="small" icon={<CheckCircleOutlined />} onClick={() => updateStatus(claim, 'approved')}>
              审批
            </Button>
          )}
          {claim.status === 'submitted' && (
            <Button size="small" danger icon={<StopOutlined />} onClick={() => updateStatus(claim, 'rejected')}>
              驳回
            </Button>
          )}
          {claim.status === 'approved' && claim.payment_status !== 'paid' && (
            <Button
              size="small"
              icon={<PayCircleOutlined />}
              onClick={() => updateStatus(claim, undefined, 'paid', dayjs().format('YYYY-MM-DD'))}
            >
              付款
            </Button>
          )}
          <Popconfirm title="确认作废该报销单?" onConfirm={() => handleDelete(claim)} okText="确认" cancelText="取消">
            <Button size="small" danger icon={<DeleteOutlined />} />
          </Popconfirm>
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
          <Card className="stat-card"><Statistic title="待处理" value={pendingCount + unpaidCount} suffix={`审批 ${pendingCount} / 付款 ${unpaidCount}`} /></Card>
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
          scroll={{ x: 1380 }}
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
              <Form.Item label="审批状态">
                <Select
                  value={form.status}
                  onChange={(value) => setForm((prev) => ({ ...prev, status: value }))}
                  options={[
                    { value: 'draft', label: '草稿' },
                    { value: 'submitted', label: '待审批' },
                    { value: 'approved', label: '已审批' },
                    { value: 'rejected', label: '已驳回' },
                  ]}
                />
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

      <Drawer
        title={detailClaim ? `报销单 ${detailClaim.claim_no}` : '报销单详情'}
        open={!!detailClaim}
        onClose={() => {
          setDetailClaim(null);
          setDetailInvoices([]);
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
          </div>
        )}
      </Drawer>
    </div>
  );
};

export default Reimbursements;
