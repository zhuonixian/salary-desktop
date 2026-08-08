import { useCallback, useEffect, useState } from 'react';
import { Button, Card, Col, DatePicker, Progress, Row, Space, Spin, Statistic, Table, Tag, message } from 'antd';
import {
  BankOutlined,
  CheckCircleOutlined,
  ExclamationCircleOutlined,
  FileDoneOutlined,
  ReloadOutlined,
  RightOutlined,
  StopOutlined,
} from '@ant-design/icons';
import dayjs from 'dayjs';
import type { Dayjs } from 'dayjs';
import { useNavigate } from 'react-router-dom';
import { getMonthCloseWorkbench } from '@/api';
import type { MonthCloseCheckItem, MonthCloseWorkbench } from '@/types';

const fmtMoney = (value?: number | null) =>
  (value ?? 0).toLocaleString('zh-CN', { minimumFractionDigits: 2, maximumFractionDigits: 2 });

const statusMeta = {
  ok: { color: 'green', text: '正常', icon: <CheckCircleOutlined /> },
  warning: { color: 'gold', text: '提醒', icon: <ExclamationCircleOutlined /> },
  blocking: { color: 'red', text: '阻塞', icon: <StopOutlined /> },
};

const MonthClose: React.FC = () => {
  const navigate = useNavigate();
  const [month, setMonth] = useState<Dayjs>(dayjs());
  const [loading, setLoading] = useState(false);
  const [workbench, setWorkbench] = useState<MonthCloseWorkbench | null>(null);

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
                  <div style={{ fontSize: 32, fontWeight: 700 }}>{progress}%</div>
                  <div style={{ color: '#999', marginTop: 4 }}>
                    {blockingCount} 阻塞 / {warningCount} 提醒 / {okCount} 正常
                  </div>
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
              <Statistic title="工资应发合计" value={fmtMoney(summary?.total_salary_cost)} prefix="¥" />
            </Card>
          </Col>
          <Col xs={24} md={8}>
            <Card className="stat-card">
              <Statistic title="发票价税合计" value={fmtMoney(summary?.total_invoice_amount)} prefix="¥" />
            </Card>
          </Col>
          <Col xs={24} md={8}>
            <Card className="stat-card">
              <Statistic title="已付款报销" value={fmtMoney(summary?.paid_reimbursement_amount)} prefix="¥" />
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
