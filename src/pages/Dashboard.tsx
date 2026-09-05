import { useCallback, useEffect, useMemo, useState } from 'react';
import { Row, Col, Card, Statistic, DatePicker, Spin, message, Tag, Table, Space } from 'antd';
import {
  TeamOutlined,
  CalculatorOutlined,
  CheckCircleOutlined,
  WarningOutlined,
  PayCircleOutlined,
  MinusCircleOutlined,
  FileTextOutlined,
  WalletOutlined,
  AuditOutlined,
  FundOutlined,
  ScheduleOutlined,
  AccountBookOutlined,
  SwapOutlined,
} from '@ant-design/icons';
import { useNavigate } from 'react-router-dom';
import { getDashboardSummary, getMonthCloseWorkbench } from '@/api';
import { SensitiveText } from '@/components/SensitiveText';
import { SensitiveStatistic } from '@/components/SensitiveStatistic';
import { useBusinessMonth } from '@/contexts/BusinessMonthContext';
import type { DashboardSummary, MonthCloseCheckItem, MonthCloseWorkbench } from '@/types';

type ChartDatum = {
  label: string;
  value: number;
  color: string;
};

const fmtNumber = (val?: number | null) => (val ?? 0).toLocaleString('zh-CN');

const statusMeta = {
  ok: { color: 'green', text: '正常' },
  warning: { color: 'gold', text: '提醒' },
  blocking: { color: 'red', text: '阻塞' },
};

const MiniBarChart: React.FC<{ data: ChartDatum[]; unit?: string }> = ({ data, unit = '' }) => {
  const max = Math.max(...data.map((item) => item.value), 1);

  return (
    <div style={{ display: 'grid', gap: 12 }}>
      {data.map((item) => (
        <div key={item.label} style={{ display: 'grid', gridTemplateColumns: '92px 1fr 110px', alignItems: 'center', gap: 10 }}>
          <span style={{ color: '#5f6b7a', fontSize: 13 }}>{item.label}</span>
          <div style={{ height: 10, background: '#eef1f5', borderRadius: 6, overflow: 'hidden' }}>
            <div
              style={{
                width: `${Math.max((item.value / max) * 100, item.value > 0 ? 4 : 0)}%`,
                height: '100%',
                background: item.color,
                borderRadius: 6,
              }}
            />
          </div>
          <span style={{ textAlign: 'right', fontVariantNumeric: 'tabular-nums', color: '#1f2a37' }}>
            {unit === '¥' ? <SensitiveText type="amount" value={item.value} /> : `${fmtNumber(item.value)}${unit}`}
          </span>
        </div>
      ))}
    </div>
  );
};

const RingChart: React.FC<{
  title: string;
  center: string;
  data: ChartDatum[];
}> = ({ title, center, data }) => {
  const total = data.reduce((sum, item) => sum + item.value, 0);
  const radius = 42;
  const circumference = 2 * Math.PI * radius;
  let offset = 0;

  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 18, minHeight: 150 }}>
      <div style={{ position: 'relative', width: 132, height: 132, flex: '0 0 auto' }}>
        <svg width="132" height="132" viewBox="0 0 132 132" role="img" aria-label={title}>
          <circle cx="66" cy="66" r={radius} fill="none" stroke="#eef1f5" strokeWidth="16" />
          {total > 0 && data.map((item) => {
            const dash = (item.value / total) * circumference;
            const circle = (
              <circle
                key={item.label}
                cx="66"
                cy="66"
                r={radius}
                fill="none"
                stroke={item.color}
                strokeWidth="16"
                strokeLinecap="round"
                strokeDasharray={`${dash} ${circumference - dash}`}
                strokeDashoffset={-offset}
                transform="rotate(-90 66 66)"
              />
            );
            offset += dash;
            return circle;
          })}
        </svg>
        <div style={{
          position: 'absolute',
          inset: 0,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          fontSize: 22,
          fontWeight: 700,
          color: '#1f2a37',
        }}>
          {center}
        </div>
      </div>
      <div style={{ minWidth: 0, flex: 1 }}>
        <div style={{ fontWeight: 600, marginBottom: 12 }}>{title}</div>
        <Space direction="vertical" size={8}>
          {data.map((item) => (
            <span key={item.label} style={{ display: 'flex', alignItems: 'center', gap: 8, color: '#5f6b7a' }}>
              <i style={{ width: 10, height: 10, borderRadius: 10, background: item.color, display: 'inline-block' }} />
              {item.label}
              <strong style={{ color: '#1f2a37', fontVariantNumeric: 'tabular-nums' }}>{fmtNumber(item.value)}</strong>
            </span>
          ))}
        </Space>
      </div>
    </div>
  );
};

const Dashboard: React.FC = () => {
  const { month, setMonth } = useBusinessMonth();
  const navigate = useNavigate();
  const [loading, setLoading] = useState(false);
  const [summary, setSummary] = useState<DashboardSummary | null>(null);
  const [workbench, setWorkbench] = useState<MonthCloseWorkbench | null>(null);

  const fetchSummary = useCallback(async (m: string) => {
    setLoading(true);
    try {
      const [dashboardData, monthCloseData] = await Promise.all([
        getDashboardSummary(m),
        getMonthCloseWorkbench(m),
      ]);
      setSummary(dashboardData);
      setWorkbench(monthCloseData);
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      message.error('获取仪表盘数据失败: ' + msg);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchSummary(month.format('YYYY-MM'));
  }, [month, fetchSummary]);

  const monthStr = month.format('YYYY-MM');
  const data = summary ?? {
    month: monthStr,
    total_employees: 0,
    pending_count: 0,
    calculated_count: 0,
    abnormal_attendance_count: 0,
    total_gross_salary: 0,
    total_deduction: 0,
    total_net_salary: 0,
    fund_pending_approval_count: 0,
    fund_unpaid_count: 0,
    unassigned_bank_tx_count: 0,
    unreconciled_tx_count: 0,
    advance_overdue_count: 0,
    advance_overdue_amount: 0,
    fund_total_balance: 0,
  };
  const closeSummary = workbench?.summary;
  const checks = useMemo(() => workbench?.checks ?? [], [workbench?.checks]);
  const checkStatusCount = useMemo(() => ({
    ok: checks.filter((item) => item.status === 'ok').length,
    warning: checks.filter((item) => item.status === 'warning').length,
    blocking: checks.filter((item) => item.status === 'blocking').length,
  }), [checks]);

  const costData: ChartDatum[] = [
    { label: '应发工资', value: data.total_gross_salary, color: '#1677ff' },
    { label: '工资扣款', value: data.total_deduction, color: '#d4380d' },
    { label: '发票金额', value: closeSummary?.total_invoice_amount ?? 0, color: '#13a8a8' },
    { label: '已批报销', value: closeSummary?.approved_reimbursement_amount ?? 0, color: '#722ed1' },
    { label: '已付报销', value: closeSummary?.paid_reimbursement_amount ?? 0, color: '#389e0d' },
  ];
  const invoiceData: ChartDatum[] = [
    {
      label: '已归类发票',
      value: Math.max((closeSummary?.invoice_count ?? 0) - (closeSummary?.uncategorized_invoice_count ?? 0), 0),
      color: '#13a8a8',
    },
    { label: '待归类发票', value: closeSummary?.uncategorized_invoice_count ?? 0, color: '#faad14' },
    { label: '待付款报销', value: closeSummary?.unpaid_reimbursement_count ?? 0, color: '#ff4d4f' },
  ];
  const closeData: ChartDatum[] = [
    { label: '正常', value: checkStatusCount.ok, color: '#52c41a' },
    { label: '提醒', value: checkStatusCount.warning, color: '#faad14' },
    { label: '阻塞', value: checkStatusCount.blocking, color: '#ff4d4f' },
  ];
  const closePercent = checks.length === 0 ? 0 : Math.round((checkStatusCount.ok / checks.length) * 100);

  const checkColumns = [
    {
      title: '月结检查',
      dataIndex: 'title',
      key: 'title',
      render: (text: string, record: MonthCloseCheckItem) => (
        <Space>
          <Tag color={statusMeta[record.status].color}>{statusMeta[record.status].text}</Tag>
          <span>{text}</span>
        </Space>
      ),
    },
    { title: '数量', dataIndex: 'count', key: 'count', width: 80, align: 'right' as const },
    { title: '说明', dataIndex: 'description', key: 'description', ellipsis: true },
  ];

  return (
    <div>
      <div className="page-header">
        <span className="page-title">首页仪表盘</span>
        <DatePicker
          picker="month"
          value={month}
          onChange={(d) => d && setMonth(d)}
          allowClear={false}
          style={{ width: 180 }}
        />
      </div>

      <Spin spinning={loading}>
        <Row gutter={[16, 16]} className="mb-16">
          <Col xs={24} sm={12} lg={6}>
            <Card className="stat-card">
              <Statistic title="员工总数" value={data.total_employees} prefix={<TeamOutlined />} valueStyle={{ color: '#1677ff' }} />
            </Card>
          </Col>
          <Col xs={24} sm={12} lg={6}>
            <Card className="stat-card">
              <Statistic title="待计算人数" value={data.pending_count} prefix={<CalculatorOutlined />} valueStyle={{ color: '#faad14' }} />
            </Card>
          </Col>
          <Col xs={24} sm={12} lg={6}>
            <Card className="stat-card">
              <Statistic title="已计算人数" value={data.calculated_count} prefix={<CheckCircleOutlined />} valueStyle={{ color: '#52c41a' }} />
            </Card>
          </Col>
          <Col xs={24} sm={12} lg={6}>
            <Card className="stat-card">
              <Statistic title="异常考勤人数" value={data.abnormal_attendance_count} prefix={<WarningOutlined />} valueStyle={{ color: '#ff4d4f' }} />
            </Card>
          </Col>
        </Row>

        <Row gutter={[16, 16]} className="mb-16">
          <Col xs={24} sm={12} lg={6}>
            <Card className="stat-card">
              <SensitiveStatistic title="应发工资合计" value={data.total_gross_salary} prefix={<PayCircleOutlined />} valueStyle={{ color: '#1677ff' }} />
            </Card>
          </Col>
          <Col xs={24} sm={12} lg={6}>
            <Card className="stat-card">
              <SensitiveStatistic title="扣款合计" value={data.total_deduction} prefix={<MinusCircleOutlined />} valueStyle={{ color: '#d4380d' }} />
            </Card>
          </Col>
          <Col xs={24} sm={12} lg={6}>
            <Card className="stat-card">
              <SensitiveStatistic title="发票价税合计" value={closeSummary?.total_invoice_amount ?? 0} prefix={<FileTextOutlined />} valueStyle={{ color: '#13a8a8' }} />
            </Card>
          </Col>
          <Col xs={24} sm={12} lg={6}>
            <Card className="stat-card">
              <SensitiveStatistic title="已付款报销" value={closeSummary?.paid_reimbursement_amount ?? 0} prefix={<WalletOutlined />} valueStyle={{ color: '#389e0d' }} />
            </Card>
          </Col>
        </Row>

        <Row gutter={[16, 16]} className="mb-16">
          <Col xs={24} sm={12} lg={4}>
            <Card
              hoverable
              className="stat-card"
              onClick={() => navigate('/fund-documents')}
            >
              <Statistic
                title="待审批资金单"
                value={data.fund_pending_approval_count}
                suffix="张"
                prefix={<ScheduleOutlined />}
                valueStyle={{ color: data.fund_pending_approval_count > 0 ? '#faad14' : '#52c41a' }}
              />
            </Card>
          </Col>
          <Col xs={24} sm={12} lg={4}>
            <Card
              hoverable
              className="stat-card"
              onClick={() => navigate('/fund-documents')}
            >
              <Statistic
                title="待付款/结算"
                value={data.fund_unpaid_count}
                suffix="张"
                prefix={<PayCircleOutlined />}
                valueStyle={{ color: data.fund_unpaid_count > 0 ? '#ff4d4f' : '#52c41a' }}
              />
            </Card>
          </Col>
          <Col xs={24} sm={12} lg={4}>
            <Card
              hoverable
              className="stat-card"
              onClick={() => navigate('/fund-accounts')}
            >
              <Statistic
                title="待归集流水"
                value={data.unassigned_bank_tx_count}
                suffix="条"
                prefix={<FundOutlined />}
                valueStyle={{ color: data.unassigned_bank_tx_count > 0 ? '#faad14' : '#52c41a' }}
              />
            </Card>
          </Col>
          <Col xs={24} sm={12} lg={4}>
            <Card
              hoverable
              className="stat-card"
              onClick={() => navigate('/bank-transactions')}
            >
              <Statistic
                title="未核销流水"
                value={data.unreconciled_tx_count}
                suffix="条"
                prefix={<SwapOutlined />}
                valueStyle={{ color: data.unreconciled_tx_count > 0 ? '#faad14' : '#52c41a' }}
              />
            </Card>
          </Col>
          <Col xs={24} sm={12} lg={4}>
            <Card
              hoverable
              className="stat-card"
              onClick={() => navigate('/advances')}
            >
              <Statistic
                title="借款逾期"
                value={data.advance_overdue_count}
                suffix="笔"
                prefix={<WarningOutlined />}
                valueStyle={{ color: data.advance_overdue_count > 0 ? '#ff4d4f' : '#52c41a' }}
              />
            </Card>
          </Col>
          <Col xs={24} sm={12} lg={4}>
            <Card
              hoverable
              className="stat-card"
              onClick={() => navigate('/fund-journals')}
            >
              <SensitiveStatistic
                title="账户余额合计"
                value={data.fund_total_balance}
                prefix={<AccountBookOutlined />}
                valueStyle={{ color: '#1677ff' }}
              />
            </Card>
          </Col>
        </Row>

        <Row gutter={[16, 16]} className="mb-16">
          <Col xs={24} lg={10}>
            <Card title="月度成本结构">
              <MiniBarChart data={costData} unit="¥" />
            </Card>
          </Col>
          <Col xs={24} md={12} lg={7}>
            <Card title="发票与报销状态">
              <RingChart
                title="发票处理"
                center={`${closeSummary?.invoice_count ?? 0}张`}
                data={invoiceData}
              />
            </Card>
          </Col>
          <Col xs={24} md={12} lg={7}>
            <Card title="月结状态">
              <RingChart
                title="检查完成度"
                center={`${closePercent}%`}
                data={closeData}
              />
            </Card>
          </Col>
        </Row>

        <Row gutter={[16, 16]}>
          <Col xs={24} lg={10}>
            <Card title="发票管理摘要">
              <Row gutter={[12, 12]}>
                <Col span={12}>
                  <Statistic title="本月发票" value={closeSummary?.invoice_count ?? 0} suffix="张" />
                </Col>
                <Col span={12}>
                  <Statistic title="待归类发票" value={closeSummary?.uncategorized_invoice_count ?? 0} suffix="张" valueStyle={{ color: '#faad14' }} />
                </Col>
                <Col span={12}>
                  <Statistic title="报销单" value={closeSummary?.reimbursement_count ?? 0} suffix="张" />
                </Col>
                <Col span={12}>
                  <Statistic title="待审批/付款" value={(closeSummary?.pending_reimbursement_count ?? 0) + (closeSummary?.unpaid_reimbursement_count ?? 0)} suffix="张" valueStyle={{ color: '#ff4d4f' }} />
                </Col>
              </Row>
            </Card>
          </Col>
          <Col xs={24} lg={14}>
            <Card title={<Space><AuditOutlined />月结提醒</Space>}>
              <Table
                rowKey="key"
                columns={checkColumns}
                dataSource={checks.filter((item) => item.status !== 'ok')}
                pagination={false}
                size="small"
                scroll={{ x: 760 }}
                locale={{ emptyText: '当前月份暂无待处理提醒' }}
              />
            </Card>
          </Col>
        </Row>
      </Spin>
    </div>
  );
};

export default Dashboard;
