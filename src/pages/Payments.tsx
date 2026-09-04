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
  DownloadOutlined,
  EyeOutlined,
  FileDoneOutlined,
  PlusOutlined,
  ReloadOutlined,
} from '@ant-design/icons';
import { save } from '@tauri-apps/plugin-dialog';
import dayjs from 'dayjs';
import type { Dayjs } from 'dayjs';
import {
  createPaymentBatch,
  exportPaymentBatchFile,
  getPaymentBatchDetail,
  markPaymentBatchPaid,
  queryPaymentBatches,
  updatePaymentBatchRemark,
  voidPaymentBatch,
} from '@/api';
import type {
  PaymentBatch,
  PaymentBatchDetail,
  PaymentBatchStatus,
  PaymentBatchType,
  PaymentItem,
} from '@/types';
import { SensitiveText } from '@/components/SensitiveText';
import { SensitiveStatistic } from '@/components/SensitiveStatistic';
import { useBusinessMonth } from '@/contexts/BusinessMonthContext';

const { TextArea } = Input;

const typeMeta: Record<PaymentBatchType, { text: string; color: string }> = {
  salary: { text: '工资', color: 'blue' },
  reimbursement: { text: '报销', color: 'cyan' },
};

const statusMeta: Record<PaymentBatchStatus, { text: string; color: string }> = {
  draft: { text: '待导出', color: 'default' },
  exported: { text: '待付款', color: 'gold' },
  paid: { text: '已付款', color: 'green' },
  void: { text: '已作废', color: 'red' },
};

const sourceText = (sourceType: string) => (sourceType === 'salary_result' ? '工资' : '报销');

const Payments: React.FC = () => {
  const { month, setMonth } = useBusinessMonth();
  const [typeFilter, setTypeFilter] = useState<PaymentBatchType | undefined>(undefined);
  const [statusFilter, setStatusFilter] = useState<PaymentBatchStatus | undefined>(undefined);
  const [batches, setBatches] = useState<PaymentBatch[]>([]);
  const [loading, setLoading] = useState(false);
  const [action, setAction] = useState<string | null>(null);
  const [detail, setDetail] = useState<PaymentBatchDetail | null>(null);
  const [paidForm] = Form.useForm<{ payment_date: Dayjs }>();
  const [remarkForm] = Form.useForm<{ remark?: string }>();

  const fetchData = useCallback(async () => {
    setLoading(true);
    try {
      setBatches(await queryPaymentBatches({
        belong_month: month.format('YYYY-MM'),
        batch_type: typeFilter,
        status: statusFilter,
      }));
    } catch (e: unknown) {
      message.error('查询付款批次失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setLoading(false);
    }
  }, [month, typeFilter, statusFilter]);

  useEffect(() => {
    fetchData();
  }, [fetchData]);

  const summary = useMemo(() => ({
    total: batches.length,
    draft: batches.filter((item) => item.status === 'draft').length,
    exported: batches.filter((item) => item.status === 'exported').length,
    paidAmount: batches
      .filter((item) => item.status === 'paid')
      .reduce((sum, item) => sum + item.total_amount, 0),
  }), [batches]);

  const openDetail = async (id: number) => {
    setAction(`detail-${id}`);
    try {
      setDetail(await getPaymentBatchDetail(id));
    } catch (e: unknown) {
      message.error('获取批次明细失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setAction(null);
    }
  };

  const handleCreate = async (batchType: PaymentBatchType) => {
    setAction(`create-${batchType}`);
    try {
      const result = await createPaymentBatch({
        belong_month: month.format('YYYY-MM'),
        batch_type: batchType,
      });
      message.success(`已生成付款批次 ${result.batch.batch_no}`);
      await fetchData();
      setDetail(result);
    } catch (e: unknown) {
      message.error('生成付款批次失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setAction(null);
    }
  };

  const handleExport = async (batch: PaymentBatch) => {
    const selected = await save({
      title: '导出付款批次',
      defaultPath: `${batch.batch_no}_付款明细.xlsx`,
      filters: [{ name: 'Excel', extensions: ['xlsx'] }],
    });
    if (!selected) return;
    setAction(`export-${batch.id}`);
    try {
      await exportPaymentBatchFile(batch.id, String(selected));
      message.success('付款明细已导出');
      await fetchData();
    } catch (e: unknown) {
      message.error('导出付款批次失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setAction(null);
    }
  };

  const openPaidModal = (batch: PaymentBatch) => {
    paidForm.setFieldsValue({ payment_date: dayjs() });
    Modal.confirm({
      title: `确认标记付款 ${batch.batch_no}?`,
      content: (
        <Form form={paidForm} layout="vertical">
          <Form.Item
            label="付款日期"
            name="payment_date"
            rules={[{ required: true, message: '请选择付款日期' }]}
          >
            <DatePicker style={{ width: '100%' }} />
          </Form.Item>
        </Form>
      ),
      okText: '标记已付款',
      cancelText: '取消',
      onOk: async () => {
        const values = await paidForm.validateFields();
        setAction(`paid-${batch.id}`);
        try {
          await markPaymentBatchPaid({
            id: batch.id,
            payment_date: values.payment_date.format('YYYY-MM-DD'),
          });
          message.success('付款状态已更新');
          await fetchData();
        } catch (e: unknown) {
          message.error('标记付款失败: ' + (e instanceof Error ? e.message : String(e)));
          throw e;
        } finally {
          setAction(null);
        }
      },
    });
  };

  const openRemarkModal = (batch: PaymentBatch) => {
    remarkForm.setFieldsValue({ remark: batch.remark });
    Modal.confirm({
      title: `更新备注 ${batch.batch_no}`,
      content: (
        <Form form={remarkForm} layout="vertical">
          <Form.Item label="备注" name="remark">
            <TextArea rows={3} placeholder="填写付款批次备注" />
          </Form.Item>
        </Form>
      ),
      okText: '保存',
      cancelText: '取消',
      onOk: async () => {
        const values = remarkForm.getFieldsValue();
        setAction(`remark-${batch.id}`);
        try {
          await updatePaymentBatchRemark({ id: batch.id, remark: values.remark });
          message.success('备注已更新');
          await fetchData();
        } catch (e: unknown) {
          message.error('更新备注失败: ' + (e instanceof Error ? e.message : String(e)));
          throw e;
        } finally {
          setAction(null);
        }
      },
    });
  };

  const handleVoid = async (batch: PaymentBatch) => {
    setAction(`void-${batch.id}`);
    try {
      await voidPaymentBatch({ id: batch.id, reason: '手动作废' });
      message.success('批次已作废');
      await fetchData();
    } catch (e: unknown) {
      message.error('作废付款批次失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setAction(null);
    }
  };

  const columns = [
    { title: '批次号', dataIndex: 'batch_no', key: 'batch_no', width: 180, fixed: 'left' as const },
    {
      title: '类型',
      dataIndex: 'batch_type',
      key: 'batch_type',
      width: 90,
      render: (type: PaymentBatchType) => <Tag color={typeMeta[type].color}>{typeMeta[type].text}</Tag>,
    },
    { title: '月份', dataIndex: 'belong_month', key: 'belong_month', width: 100 },
    {
      title: '状态',
      dataIndex: 'status',
      key: 'status',
      width: 90,
      render: (status: PaymentBatchStatus) => <Tag color={statusMeta[status].color}>{statusMeta[status].text}</Tag>,
    },
    { title: '笔数', dataIndex: 'item_count', key: 'item_count', width: 80, align: 'right' as const },
    {
      title: '金额',
      dataIndex: 'total_amount',
      key: 'total_amount',
      width: 130,
      align: 'right' as const,
      render: (value: number) => <SensitiveText type="amount" value={value} />,
    },
    { title: '付款日期', dataIndex: 'payment_date', key: 'payment_date', width: 110 },
    { title: '备注', dataIndex: 'remark', key: 'remark', ellipsis: true },
    {
      title: '操作',
      key: 'actions',
      width: 310,
      fixed: 'right' as const,
      render: (_: unknown, batch: PaymentBatch) => (
        <Space size={6} wrap>
          <Button
            size="small"
            icon={<EyeOutlined />}
            loading={action === `detail-${batch.id}`}
            onClick={() => openDetail(batch.id)}
          >
            明细
          </Button>
          <Button
            size="small"
            icon={<DownloadOutlined />}
            disabled={batch.status === 'void'}
            loading={action === `export-${batch.id}`}
            onClick={() => handleExport(batch)}
          >
            导出
          </Button>
          <Button
            size="small"
            icon={<CheckCircleOutlined />}
            disabled={batch.status !== 'exported'}
            loading={action === `paid-${batch.id}`}
            onClick={() => openPaidModal(batch)}
          >
            付款
          </Button>
          <Button size="small" onClick={() => openRemarkModal(batch)}>
            备注
          </Button>
          <Popconfirm
            title="确认作废该付款批次?"
            okText="作废"
            cancelText="取消"
            onConfirm={() => handleVoid(batch)}
          >
            <Button
              size="small"
              danger
              icon={<DeleteOutlined />}
              disabled={batch.status === 'void'}
              loading={action === `void-${batch.id}`}
            >
              作废
            </Button>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  const detailColumns = [
    { title: '收款人', dataIndex: 'employee_name', key: 'employee_name', width: 110 },
    { title: '来源', dataIndex: 'source_type', key: 'source_type', width: 80, render: sourceText },
    { title: '来源ID', dataIndex: 'source_id', key: 'source_id', width: 80 },
    { title: '工号', dataIndex: 'employee_no', key: 'employee_no', width: 100 },
    {
      title: '银行账号',
      dataIndex: 'bank_account',
      key: 'bank_account',
      width: 200,
      render: (value: string) => <SensitiveText type="bank_card" value={value} />,
    },
    {
      title: '开户行',
      dataIndex: 'bank_name',
      key: 'bank_name',
      width: 180,
      render: (value: string) => <SensitiveText type="address" value={value} />,
    },
    {
      title: '金额',
      dataIndex: 'amount',
      key: 'amount',
      width: 130,
      align: 'right' as const,
      render: (value: number) => <SensitiveText type="amount" value={value} />,
    },
    { title: '备注', dataIndex: 'remark', key: 'remark', width: 160 },
  ];

  return (
    <div>
      <div className="page-header">
        <span className="page-title">付款批次</span>
        <div className="page-header-actions">
          <DatePicker
            picker="month"
            value={month}
            allowClear={false}
            onChange={(value) => value && setMonth(value)}
            style={{ width: 160 }}
          />
          <Select
            placeholder="付款类型"
            allowClear
            value={typeFilter}
            onChange={setTypeFilter}
            style={{ width: 130 }}
            options={[
              { value: 'salary', label: '工资' },
              { value: 'reimbursement', label: '报销' },
            ]}
          />
          <Select
            placeholder="批次状态"
            allowClear
            value={statusFilter}
            onChange={setStatusFilter}
            style={{ width: 130 }}
            options={[
              { value: 'draft', label: '待导出' },
              { value: 'exported', label: '待付款' },
              { value: 'paid', label: '已付款' },
              { value: 'void', label: '已作废' },
            ]}
          />
          <Button icon={<ReloadOutlined />} loading={loading} onClick={fetchData}>
            刷新
          </Button>
          <Button
            type="primary"
            icon={<PlusOutlined />}
            loading={action === 'create-salary'}
            onClick={() => handleCreate('salary')}
          >
            生成工资批次
          </Button>
          <Button
            icon={<FileDoneOutlined />}
            loading={action === 'create-reimbursement'}
            onClick={() => handleCreate('reimbursement')}
          >
            生成报销批次
          </Button>
        </div>
      </div>

      <Spin spinning={loading}>
        <Row gutter={[16, 16]} className="mb-16">
          <Col xs={24} sm={12} lg={6}>
            <Card className="stat-card"><Statistic title="批次数" value={summary.total} /></Card>
          </Col>
          <Col xs={24} sm={12} lg={6}>
            <Card className="stat-card"><Statistic title="待导出" value={summary.draft} /></Card>
          </Col>
          <Col xs={24} sm={12} lg={6}>
            <Card className="stat-card"><Statistic title="待付款" value={summary.exported} /></Card>
          </Col>
          <Col xs={24} sm={12} lg={6}>
            <Card className="stat-card"><SensitiveStatistic title="已付款金额" value={summary.paidAmount} /></Card>
          </Col>
        </Row>

        <Card>
          <Table
            rowKey="id"
            columns={columns}
            dataSource={batches}
            pagination={{ pageSize: 10 }}
            scroll={{ x: 1180 }}
          />
        </Card>
      </Spin>

      <Drawer
        title={detail ? `批次明细 ${detail.batch.batch_no}` : '批次明细'}
        width={920}
        open={!!detail}
        onClose={() => setDetail(null)}
      >
        {detail && (
          <Space direction="vertical" style={{ width: '100%' }} size={16}>
            <Row gutter={[16, 16]}>
              <Col span={6}><Statistic title="类型" value={typeMeta[detail.batch.batch_type].text} /></Col>
              <Col span={6}><Statistic title="状态" value={statusMeta[detail.batch.status].text} /></Col>
              <Col span={6}><Statistic title="笔数" value={detail.batch.item_count} /></Col>
              <Col span={6}><SensitiveStatistic title="总金额" value={detail.batch.total_amount} /></Col>
            </Row>
            <Table<PaymentItem>
              rowKey="id"
              columns={detailColumns}
              dataSource={detail.items}
              pagination={false}
              scroll={{ x: 980 }}
              size="middle"
            />
          </Space>
        )}
      </Drawer>
    </div>
  );
};

export default Payments;
