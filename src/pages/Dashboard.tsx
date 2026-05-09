import { useState, useEffect } from 'react';
import { Row, Col, Card, Statistic, DatePicker, Spin, message } from 'antd';
import {
  TeamOutlined,
  CalculatorOutlined,
  CheckCircleOutlined,
  WarningOutlined,
  PayCircleOutlined,
  MinusCircleOutlined,
  DollarOutlined,
} from '@ant-design/icons';
import dayjs from 'dayjs';
import type { Dayjs } from 'dayjs';
import { getDashboardSummary } from '@/api';
import type { DashboardSummary } from '@/types';

const Dashboard: React.FC = () => {
  const [month, setMonth] = useState<Dayjs>(dayjs());
  const [loading, setLoading] = useState(false);
  const [summary, setSummary] = useState<DashboardSummary | null>(null);

  const fetchSummary = async (m: string) => {
    setLoading(true);
    try {
      const data = await getDashboardSummary(m);
      setSummary(data);
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      message.error('获取仪表盘数据失败: ' + msg);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchSummary(month.format('YYYY-MM'));
  }, [month]);

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
  };

  const fmt = (val?: number | null) => (val ?? 0).toLocaleString('zh-CN', { minimumFractionDigits: 2, maximumFractionDigits: 2 });

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
        <Row gutter={[16, 16]} className="mb-24">
          <Col xs={24} sm={12} lg={6}>
            <Card className="stat-card">
              <Statistic
                title="员工总数"
                value={data.total_employees}
                prefix={<TeamOutlined />}
                valueStyle={{ color: '#1677ff' }}
              />
            </Card>
          </Col>
          <Col xs={24} sm={12} lg={6}>
            <Card className="stat-card">
              <Statistic
                title="待计算人数"
                value={data.pending_count}
                prefix={<CalculatorOutlined />}
                valueStyle={{ color: '#faad14' }}
              />
            </Card>
          </Col>
          <Col xs={24} sm={12} lg={6}>
            <Card className="stat-card">
              <Statistic
                title="已计算人数"
                value={data.calculated_count}
                prefix={<CheckCircleOutlined />}
                valueStyle={{ color: '#52c41a' }}
              />
            </Card>
          </Col>
          <Col xs={24} sm={12} lg={6}>
            <Card className="stat-card">
              <Statistic
                title="异常考勤人数"
                value={data.abnormal_attendance_count}
                prefix={<WarningOutlined />}
                valueStyle={{ color: '#ff4d4f' }}
              />
            </Card>
          </Col>
        </Row>

        <Row gutter={[16, 16]}>
          <Col xs={24} sm={8}>
            <Card className="stat-card">
              <Statistic
                title="本月应发工资合计"
                value={fmt(data.total_gross_salary)}
                prefix={<PayCircleOutlined />}
                valueStyle={{ color: '#1677ff' }}
              />
            </Card>
          </Col>
          <Col xs={24} sm={8}>
            <Card className="stat-card">
              <Statistic
                title="扣款合计"
                value={fmt(data.total_deduction)}
                prefix={<MinusCircleOutlined />}
                valueStyle={{ color: '#ff4d4f' }}
              />
            </Card>
          </Col>
          <Col xs={24} sm={8}>
            <Card className="stat-card">
              <Statistic
                title="实发工资合计"
                value={fmt(data.total_net_salary)}
                prefix={<DollarOutlined />}
                valueStyle={{ color: '#52c41a' }}
              />
            </Card>
          </Col>
        </Row>
      </Spin>
    </div>
  );
};

export default Dashboard;
