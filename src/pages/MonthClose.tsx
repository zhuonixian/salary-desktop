import { useCallback, useEffect, useState } from 'react';
import { Button, Card, Col, DatePicker, Form, Input, Modal, Progress, Row, Space, Spin, Statistic, Table, Tag, message } from 'antd';
import {
  BankOutlined,
  CheckCircleOutlined,
  DownloadOutlined,
  ExclamationCircleOutlined,
  FileDoneOutlined,
  LockOutlined,
  ReloadOutlined,
  RightOutlined,
  StopOutlined,
  UnlockOutlined,
} from '@ant-design/icons';
import { open } from '@tauri-apps/plugin-dialog';
import dayjs from 'dayjs';
import type { Dayjs } from 'dayjs';
import { useNavigate } from 'react-router-dom';
import { closeMonth, exportMonthClosePackage, getMonthCloseWorkbench, reopenMonth } from '@/api';
import { SensitiveStatistic } from '@/components/SensitiveStatistic';
import type { MonthCloseCheckItem, MonthCloseWorkbench } from '@/types';

const { TextArea } = Input;

const fmtTime = (value?: string) => (value ? dayjs(value).format('YYYY-MM-DD HH:mm:ss') : '-');

const statusMeta = {
  ok: { color: 'green', text: '正常', icon: <CheckCircleOutlined /> },
  warning: { color: 'gold', text: '提醒', icon: <ExclamationCircleOutlined /> },
  blocking: { color: 'red', text: '阻塞', icon: <StopOutlined /> },
};

const closeStatusMeta = {
  open: { color: 'default', text: '未月结' },
  closed: { color: 'green', text: '已月结' },
  reopened: { color: 'gold', text: '已反月结' },
};

const MonthClose: React.FC = () => {
  const navigate = useNavigate();
  const [month, setMonth] = useState<Dayjs>(dayjs());
  const [loading, setLoading] = useState(false);
  const [action, setAction] = useState<'close' | 'reopen' | 'export' | null>(null);
  const [workbench, setWorkbench] = useState<MonthCloseWorkbench | null>(null);
  const [closeForm] = Form.useForm<{ remark?: string }>();
  const [reopenForm] = Form.useForm<{ reason: string }>();

  const fetchData = useCallback(async () => {
    setLoading(true);
    try {
      setWorkbench(await getMonthCloseWorkbench(month.format('YYYY-MM')));
    } catch (e: unknown) {
      message.error('获取月结数据失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setLoading(false);
    }
  }, [month]);

  useEffect(() => {
    fetchData();
  }, [fetchData]);

  const summary = workbench?.summary;
  const checks = workbench?.checks ?? [];
  const okCount = checks.filter((item) => item.status === 'ok').length;
  const warningCount = checks.filter((item) => item.status === 'warning').length;
  const blockingCount = checks.filter((item) => item.status === 'blocking').length;
  const progress = checks.length === 0 ? 0 : Math.round((okCount / checks.length) * 100);
  const monthClose = workbench?.month_close;
  const closeStatus = monthClose?.status ?? 'open';
  const isClosed = closeStatus === 'closed';
  const canClose = !isClosed && checks.length > 0 && blockingCount === 0;

  const submitClose = async () => {
    if (!canClose) {
      message.warning(blockingCount > 0 ? '仍有阻塞检查项，不能正式月结' : '暂无可月结数据');
      return;
    }
    setAction('close');
    try {
      const values = closeForm.getFieldsValue();
      await closeMonth(month.format('YYYY-MM'), values.remark);
      message.success('正式月结完成');
      closeForm.resetFields();
      await fetchData();
    } catch (e: unknown) {
      message.error('正式月结失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setAction(null);
    }
  };

  const submitReopen = async () => {
    const values = await reopenForm.validateFields();
    setAction('reopen');
    try {
      await reopenMonth(month.format('YYYY-MM'), values.reason);
      message.success('反月结完成');
      reopenForm.resetFields();
      await fetchData();
    } catch (e: unknown) {
      message.error('反月结失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setAction(null);
    }
  };

  const handleExportPackage = async () => {
    const selected = await open({ directory: true, multiple: false, title: '选择月结包导出目录' });
    if (!selected) return;
    setAction('export');
    try {
      const result = await exportMonthClosePackage(month.format('YYYY-MM'), String(selected));
      message.success(`月结包已导出: ${result.output_dir}`);
    } catch (e: unknown) {
      message.error('导出月结包失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setAction(null);
    }
  };

  const columns = [
    {
      title: '检查项',
      dataIndex: 'title',
      key: 'title',
      width: 180,
      render: (text: string, record: MonthCloseCheckItem) => (
        <Space>
          <Tag color={statusMeta[record.status].color} icon={statusMeta[record.status].icon}>
            {statusMeta[record.status].text}
          </Tag>
          <span style={{ fontWeight: 500 }}>{text}</span>
        </Space>
      ),
    },
    { title: '数量', dataIndex: 'count', key: 'count', width: 80, align: 'right' as const },
    { title: '说明', dataIndex: 'description', key: 'description' },
    {
      title: '处理',
      key: 'action',
      width: 110,
      render: (_: unknown, record: MonthCloseCheckItem) => (
        <Button
          size="small"
          icon={<RightOutlined />}
          disabled={!record.action_route}
          onClick={() => record.action_route && navigate(record.action_route)}
        >
          前往
        </Button>
      ),
    },
  ];

  return (
    <div>
      <div className="page-header">
        <span className="page-title">月结工作台</span>
        <div className="page-header-actions">
          <DatePicker
            picker="month"
            value={month}
            onChange={(d) => d && setMonth(d)}
            allowClear={false}
            style={{ width: 180 }}
          />
          <Button
            type="primary"
            icon={<LockOutlined />}
            disabled={!canClose}
            loading={action === 'close'}
            onClick={() => Modal.confirm({
              title: `确认正式月结 ${month.format('YYYY-MM')}?`,
              content: (
                <Space direction="vertical" style={{ width: '100%' }}>
                  <span>月结后该月工资、考勤、发票和报销将禁止修改。</span>
                  <span>当前检查：{blockingCount} 阻塞 / {warningCount} 提醒 / {okCount} 正常。</span>
                  <Form form={closeForm} layout="vertical">
                    <Form.Item label="备注" name="remark">
                      <TextArea rows={3} placeholder="可填写本次月结说明" />
                    </Form.Item>
                  </Form>
                </Space>
              ),
              okText: '正式月结',
              cancelText: '取消',
              onOk: submitClose,
            })}
          >
            正式月结
          </Button>
          <Button
            danger
            icon={<UnlockOutlined />}
            disabled={!isClosed}
            loading={action === 'reopen'}
            onClick={() => Modal.confirm({
              title: `确认反月结 ${month.format('YYYY-MM')}?`,
              content: (
                <Form form={reopenForm} layout="vertical">
                  <Form.Item
                    label="反月结原因"
                    name="reason"
                    rules={[{ required: true, message: '请输入反月结原因' }]}
                  >
                    <TextArea rows={3} placeholder="请输入需要重新打开该月数据的原因" />
                  </Form.Item>
                </Form>
              ),
              okText: '反月结',
              cancelText: '取消',
              okButtonProps: { danger: true },
              onOk: submitReopen,
            })}
          >
            反月结
          </Button>
          <Button
            icon={<DownloadOutlined />}
            disabled={!isClosed}
            loading={action === 'export'}
            onClick={handleExportPackage}
          >
            导出月结包
          </Button>
          <Button icon={<ReloadOutlined />} onClick={fetchData} loading={loading}>
            刷新
          </Button>
        </div>
      </div>

      <Spin spinning={loading}>
        <Row gutter={[16, 16]} className="mb-16">
          <Col xs={24} lg={8}>
            <Card className="stat-card" style={{ height: '100%' }}>
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 16 }}>
                <div>
                  <div style={{ color: '#666', marginBottom: 8 }}>月结完成度</div>
                  <Space align="center">
                    <div style={{ fontSize: 32, fontWeight: 700 }}>{progress}%</div>
                    <Tag color={closeStatusMeta[closeStatus].color}>{closeStatusMeta[closeStatus].text}</Tag>
                  </Space>
                  <div style={{ color: '#999', marginTop: 4 }}>
                    {blockingCount} 阻塞 / {warningCount} 提醒 / {okCount} 正常
                  </div>
                  {monthClose && (
                    <div style={{ color: '#999', marginTop: 4 }}>
                      {isClosed ? `关账: ${fmtTime(monthClose.closed_at)}` : `反月结: ${fmtTime(monthClose.reopened_at)}`}
                    </div>
                  )}
                </div>
                <Progress type="circle" percent={progress} size={96} status={blockingCount > 0 ? 'exception' : 'success'} />
              </div>
            </Card>
          </Col>
          <Col xs={24} sm={12} lg={4}>
            <Card className="stat-card">
              <Statistic title="在职员工" value={summary?.active_employee_count ?? 0} prefix={<FileDoneOutlined />} />
            </Card>
          </Col>
          <Col xs={24} sm={12} lg={4}>
            <Card className="stat-card">
              <Statistic title="工资结果" value={summary?.salary_count ?? 0} suffix={`/ ${summary?.active_employee_count ?? 0}`} />
            </Card>
          </Col>
          <Col xs={24} sm={12} lg={4}>
            <Card className="stat-card">
              <Statistic title="发票张数" value={summary?.invoice_count ?? 0} />
            </Card>
          </Col>
          <Col xs={24} sm={12} lg={4}>
            <Card className="stat-card">
              <Statistic title="报销单" value={summary?.reimbursement_count ?? 0} prefix={<BankOutlined />} />
            </Card>
          </Col>
        </Row>

        <Row gutter={[16, 16]} className="mb-16">
          <Col xs={24} md={8}>
            <Card className="stat-card">
              <SensitiveStatistic title="工资应发合计" value={summary?.total_salary_cost ?? 0} />
            </Card>
          </Col>
          <Col xs={24} md={8}>
            <Card className="stat-card">
              <SensitiveStatistic title="发票价税合计" value={summary?.total_invoice_amount ?? 0} />
            </Card>
          </Col>
          <Col xs={24} md={8}>
            <Card className="stat-card">
              <SensitiveStatistic title="已付款报销" value={summary?.paid_reimbursement_amount ?? 0} />
            </Card>
          </Col>
        </Row>

        <Card>
          <Table
            rowKey="key"
            columns={columns}
            dataSource={checks}
            pagination={false}
            size="middle"
            scroll={{ x: 820 }}
          />
        </Card>
      </Spin>
    </div>
  );
};

export default MonthClose;
