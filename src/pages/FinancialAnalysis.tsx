import { useCallback, useEffect, useMemo, useState } from 'react';
import { Button, Card, Col, DatePicker, Empty, Row, Select, Space, Spin, Statistic, Table, Tabs, Tag, message } from 'antd';
import {
  BarChartOutlined,
  DownloadOutlined,
  FileExcelOutlined,
  LineChartOutlined,
  ReloadOutlined,
  RiseOutlined,
  TeamOutlined,
} from '@ant-design/icons';
import { save } from '@tauri-apps/plugin-dialog';
import dayjs from 'dayjs';
import type { Dayjs } from 'dayjs';
import {
  exportDepartmentCostReport,
  exportExpenseAnalysisReport,
  exportMonthCloseReport,
  getFinancialAnalysis,
} from '@/api';
import type {
  DepartmentCostAnalysis,
  EmployeeCostView,
  ExpenseTypeTrend,
  FinancialAnalysisQuery,
  FinancialAnalysisReport,
  MonthlyComparison,
} from '@/types';

const fmtMoney = (value?: number | null) =>
  (value ?? 0).toLocaleString('zh-CN', { minimumFractionDigits: 2, maximumFractionDigits: 2 });

const fmtPercent = (current: number, previous: number) => {
  if (previous === 0) return current === 0 ? '0.0%' : '+100.0%';
  const value = ((current - previous) / Math.abs(previous)) * 100;
  return `${value >= 0 ? '+' : ''}${value.toFixed(1)}%`;
};

const costTone = ['#1677ff', '#13a8a8', '#d48806', '#722ed1', '#d4380d'];

type MetricKey = 'gross_salary' | 'net_salary' | 'deduction' | 'invoice_amount' | 'reimbursement_amount' | 'total_cost';

const metricLabels: Record<MetricKey, string> = {
  gross_salary: '应发',
  net_salary: '实发',
  deduction: '扣款',
  invoice_amount: '发票',
  reimbursement_amount: '报销',
  total_cost: '总成本',
};

const FinancialAnalysis: React.FC = () => {
  const [month, setMonth] = useState<Dayjs>(dayjs());
  const [months, setMonths] = useState(6);
  const [loading, setLoading] = useState(false);
  const [exporting, setExporting] = useState<string | null>(null);
  const [report, setReport] = useState<FinancialAnalysisReport | null>(null);

  const query = useMemo<FinancialAnalysisQuery>(() => ({
    month: month.format('YYYY-MM'),
    months,
  }), [month, months]);

  const fetchData = useCallback(async () => {
    setLoading(true);
    try {
      setReport(await getFinancialAnalysis(query));
    } catch (e: unknown) {
      message.error('获取财务分析失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setLoading(false);
    }
  }, [query]);

  useEffect(() => {
    fetchData();
  }, [fetchData]);

  const totals = useMemo(() => {
    const current = report?.monthly_comparison.find((item) => item.month === query.month);
    const previous = report?.monthly_comparison.find((item) => item.month !== query.month);
    return { current, previous };
  }, [report, query.month]);

  const handleExport = async (
    type: 'department' | 'expense' | 'monthClose',
    defaultName: string,
  ) => {
    const path = await save({
      defaultPath: defaultName,
      filters: [{ name: 'Excel', extensions: ['xlsx'] }],
    });
    if (!path) return;
    setExporting(type);
    try {
      if (type === 'department') {
        await exportDepartmentCostReport(query, path);
      } else if (type === 'expense') {
        await exportExpenseAnalysisReport(query, path);
      } else {
        await exportMonthCloseReport(query, path);
      }
      message.success('导出成功');
    } catch (e: unknown) {
      message.error('导出失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setExporting(null);
    }
  };

  const departmentColumns = [
    { title: '部门', dataIndex: 'department', key: 'department', fixed: 'left' as const, width: 120 },
    { title: '人数', dataIndex: 'employee_count', key: 'employee_count', width: 80, align: 'right' as const },
    {
      title: '工资成本',
      dataIndex: 'salary_cost',
      key: 'salary_cost',
      width: 130,
      align: 'right' as const,
      render: (v: number, row: DepartmentCostAnalysis) => (
        <MoneyStack main={v} sub={`应发 ${fmtMoney(row.gross_salary)}`} />
      ),
    },
    { title: '社保', dataIndex: 'social_security', key: 'social_security', width: 110, align: 'right' as const, render: fmtMoney },
    { title: '公积金', dataIndex: 'housing_fund', key: 'housing_fund', width: 110, align: 'right' as const, render: fmtMoney },
    { title: '发票费用', dataIndex: 'invoice_amount', key: 'invoice_amount', width: 120, align: 'right' as const, render: fmtMoney },
    { title: '报销金额', dataIndex: 'reimbursement_amount', key: 'reimbursement_amount', width: 120, align: 'right' as const, render: fmtMoney },
    {
      title: '总成本',
      dataIndex: 'total_cost',
      key: 'total_cost',
      width: 130,
      align: 'right' as const,
      render: (v: number) => <strong>¥{fmtMoney(v)}</strong>,
      sorter: (a: DepartmentCostAnalysis, b: DepartmentCostAnalysis) => a.total_cost - b.total_cost,
      defaultSortOrder: 'descend' as const,
    },
  ];

  const employeeColumns = [
    { title: '部门', dataIndex: 'department', key: 'department', width: 110 },
    { title: '工号', dataIndex: 'employee_no', key: 'employee_no', width: 100 },
    { title: '姓名', dataIndex: 'name', key: 'name', width: 100 },
    { title: '应发', dataIndex: 'gross_salary', key: 'gross_salary', width: 110, align: 'right' as const, render: fmtMoney },
    { title: '实发', dataIndex: 'net_salary', key: 'net_salary', width: 110, align: 'right' as const, render: fmtMoney },
    { title: '社保', dataIndex: 'social_security', key: 'social_security', width: 100, align: 'right' as const, render: fmtMoney },
    { title: '公积金', dataIndex: 'housing_fund', key: 'housing_fund', width: 100, align: 'right' as const, render: fmtMoney },
    {
      title: '考勤影响',
      dataIndex: 'attendance_deduction',
      key: 'attendance_deduction',
      width: 140,
      align: 'right' as const,
      render: (v: number, row: EmployeeCostView) => (
        <Space>
          <span>¥{fmtMoney(v)}</span>
          {row.abnormal_attendance_count > 0 && <Tag color="orange">{row.abnormal_attendance_count} 异常</Tag>}
        </Space>
      ),
    },
    { title: '发票', dataIndex: 'invoice_amount', key: 'invoice_amount', width: 100, align: 'right' as const, render: fmtMoney },
    { title: '报销', dataIndex: 'reimbursement_amount', key: 'reimbursement_amount', width: 100, align: 'right' as const, render: fmtMoney },
    {
      title: '总成本',
      dataIndex: 'total_cost',
      key: 'total_cost',
      width: 120,
      align: 'right' as const,
      render: (v: number) => <strong>¥{fmtMoney(v)}</strong>,
      sorter: (a: EmployeeCostView, b: EmployeeCostView) => a.total_cost - b.total_cost,
      defaultSortOrder: 'descend' as const,
    },
  ];

  const comparisonColumns = [
    { title: '月份', dataIndex: 'month', key: 'month', width: 100 },
    ...Object.entries(metricLabels).map(([key, title]) => ({
      title,
      dataIndex: key,
      key,
      align: 'right' as const,
      render: (value: number) => `¥${fmtMoney(value)}`,
    })),
  ];

  return (
    <div>
      <div className="page-header">
        <span className="page-title">财务分析</span>
        <div className="page-header-actions">
          <DatePicker
            picker="month"
            value={month}
            onChange={(d) => d && setMonth(d)}
            allowClear={false}
            style={{ width: 150 }}
          />
          <Select
            value={months}
            onChange={setMonths}
            style={{ width: 120 }}
            options={[
              { value: 3, label: '近3个月' },
              { value: 6, label: '近6个月' },
              { value: 12, label: '近12个月' },
            ]}
          />
          <Button icon={<ReloadOutlined />} onClick={fetchData} loading={loading}>
            刷新
          </Button>
        </div>
      </div>

      <Spin spinning={loading}>
        <Row gutter={[16, 16]} className="mb-16">
          <Col xs={24} md={12} xl={6}>
            <MetricCard
              title="月度总成本"
              value={totals.current?.total_cost}
              previous={totals.previous?.total_cost}
              icon={<BarChartOutlined />}
              tone="#1677ff"
            />
          </Col>
          <Col xs={24} md={12} xl={6}>
            <MetricCard
              title="应发工资"
              value={totals.current?.gross_salary}
              previous={totals.previous?.gross_salary}
              icon={<TeamOutlined />}
              tone="#13a8a8"
            />
          </Col>
          <Col xs={24} md={12} xl={6}>
            <MetricCard
              title="发票费用"
              value={totals.current?.invoice_amount}
              previous={totals.previous?.invoice_amount}
              icon={<FileExcelOutlined />}
              tone="#d48806"
            />
          </Col>
          <Col xs={24} md={12} xl={6}>
            <MetricCard
              title="报销金额"
              value={totals.current?.reimbursement_amount}
              previous={totals.previous?.reimbursement_amount}
              icon={<RiseOutlined />}
              tone="#722ed1"
            />
          </Col>
        </Row>

        <Tabs
          items={[
            {
              key: 'department',
              label: '部门成本分析',
              children: (
                <Space direction="vertical" size={16} style={{ width: '100%' }}>
                  <Toolbar
                    title="部门成本表"
                    onExport={() => handleExport('department', `${query.month}-部门成本表.xlsx`)}
                    loading={exporting === 'department'}
                  />
                  <Card>
                    <Table
                      rowKey="department"
                      columns={departmentColumns}
                      dataSource={report?.department_costs ?? []}
                      pagination={false}
                      scroll={{ x: 1000 }}
                    />
                  </Card>
                  <CostBars rows={report?.department_costs ?? []} />
                </Space>
              ),
            },
            {
              key: 'expense',
              label: '费用类型趋势',
              children: (
                <Space direction="vertical" size={16} style={{ width: '100%' }}>
                  <Toolbar
                    title="费用分析表"
                    onExport={() => handleExport('expense', `${query.month}-费用分析表.xlsx`)}
                    loading={exporting === 'expense'}
                  />
                  <ExpenseTrendBoard rows={report?.expense_trends ?? []} />
                </Space>
              ),
            },
            {
              key: 'employee',
              label: '员工成本视图',
              children: (
                <Card>
                  <Table
                    rowKey={(row) => row.employee_id ?? row.employee_no}
                    columns={employeeColumns}
                    dataSource={report?.employee_costs ?? []}
                    pagination={{ pageSize: 12 }}
                    scroll={{ x: 1180 }}
                  />
                </Card>
              ),
            },
            {
              key: 'comparison',
              label: '月度对比',
              children: (
                <Space direction="vertical" size={16} style={{ width: '100%' }}>
                  <Toolbar
                    title="月结报告"
                    onExport={() => handleExport('monthClose', `${query.month}-月结报告.xlsx`)}
                    loading={exporting === 'monthClose'}
                  />
                  <MonthlyDelta rows={report?.monthly_comparison ?? []} />
                  <Card>
                    <Table
                      rowKey="month"
                      columns={comparisonColumns}
                      dataSource={report?.monthly_comparison ?? []}
                      pagination={false}
                      scroll={{ x: 920 }}
                    />
                  </Card>
                </Space>
              ),
            },
          ]}
        />
      </Spin>
    </div>
  );
};

const MetricCard: React.FC<{
  title: string;
  value?: number;
  previous?: number;
  icon: React.ReactNode;
  tone: string;
}> = ({ title, value = 0, previous = 0, icon, tone }) => (
  <Card className="stat-card" style={{ borderTop: `3px solid ${tone}` }}>
    <Statistic
      title={title}
      value={fmtMoney(value)}
      prefix={icon}
      valueStyle={{ color: tone }}
    />
    <div style={{ marginTop: 8, color: value >= previous ? '#cf1322' : '#389e0d', fontSize: 13 }}>
      环比 {fmtPercent(value, previous)}
    </div>
  </Card>
);

const Toolbar: React.FC<{ title: string; onExport: () => void; loading: boolean }> = ({ title, onExport, loading }) => (
  <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12 }}>
    <div style={{ fontWeight: 600 }}>{title}</div>
    <Button icon={<DownloadOutlined />} onClick={onExport} loading={loading}>
      导出
    </Button>
  </div>
);

const MoneyStack: React.FC<{ main: number; sub: string }> = ({ main, sub }) => (
  <div>
    <strong>¥{fmtMoney(main)}</strong>
    <div style={{ color: '#8c8c8c', fontSize: 12 }}>{sub}</div>
  </div>
);

const CostBars: React.FC<{ rows: DepartmentCostAnalysis[] }> = ({ rows }) => {
  const max = Math.max(...rows.map((row) => row.total_cost), 1);
  if (rows.length === 0) return null;

  return (
    <Card title={<Space><LineChartOutlined />部门总成本排行</Space>}>
      <div style={{ display: 'grid', gap: 12 }}>
        {rows.slice(0, 8).map((row, index) => (
          <div key={row.department} style={{ display: 'grid', gridTemplateColumns: '120px 1fr 120px', alignItems: 'center', gap: 12 }}>
            <span style={{ color: '#1f2a37', fontWeight: 500 }}>{row.department}</span>
            <div style={{ height: 12, background: '#f0f2f5', overflow: 'hidden', borderRadius: 4 }}>
              <div
                style={{
                  height: '100%',
                  width: `${Math.max((row.total_cost / max) * 100, 2)}%`,
                  background: costTone[index % costTone.length],
                }}
              />
            </div>
            <span style={{ textAlign: 'right', fontVariantNumeric: 'tabular-nums' }}>¥{fmtMoney(row.total_cost)}</span>
          </div>
        ))}
      </div>
    </Card>
  );
};

const ExpenseTrendBoard: React.FC<{ rows: ExpenseTypeTrend[] }> = ({ rows }) => {
  const months = Array.from(new Set(rows.map((row) => row.month))).sort();
  const names = Array.from(new Set(rows.map((row) => row.expense_type_name))).sort();
  const max = Math.max(...rows.map((row) => row.invoice_amount + row.reimbursement_amount), 1);
  const byKey = new Map(rows.map((row) => [`${row.month}-${row.expense_type_name}`, row]));

  if (rows.length === 0) {
    return <Card><Empty description="暂无费用趋势数据" /></Card>;
  }

  return (
    <Card title="费用类型按月对比">
      <div style={{ overflowX: 'auto' }}>
        <div style={{ minWidth: Math.max(760, months.length * 128), display: 'grid', gap: 10 }}>
          <div style={{ display: 'grid', gridTemplateColumns: `120px repeat(${months.length}, 1fr)`, gap: 8, color: '#5f6b7a', fontSize: 13 }}>
            <span>费用类型</span>
            {months.map((month) => <span key={month} style={{ textAlign: 'right' }}>{month}</span>)}
          </div>
          {names.map((name, rowIndex) => (
            <div key={name} style={{ display: 'grid', gridTemplateColumns: `120px repeat(${months.length}, 1fr)`, gap: 8, alignItems: 'center' }}>
              <span style={{ fontWeight: 500 }}>{name}</span>
              {months.map((month) => {
                const item = byKey.get(`${month}-${name}`);
                const value = (item?.invoice_amount ?? 0) + (item?.reimbursement_amount ?? 0);
                return (
                  <div key={month} style={{ minWidth: 0 }}>
                    <div style={{ textAlign: 'right', fontSize: 12, fontVariantNumeric: 'tabular-nums' }}>¥{fmtMoney(value)}</div>
                    <div style={{ height: 8, background: '#f0f2f5', borderRadius: 3, overflow: 'hidden' }}>
                      <div
                        style={{
                          height: '100%',
                          width: `${Math.max((value / max) * 100, value > 0 ? 4 : 0)}%`,
                          background: costTone[rowIndex % costTone.length],
                        }}
                      />
                    </div>
                  </div>
                );
              })}
            </div>
          ))}
        </div>
      </div>
    </Card>
  );
};

const MonthlyDelta: React.FC<{ rows: MonthlyComparison[] }> = ({ rows }) => {
  if (rows.length < 2) return <Card><Empty description="暂无上月对比数据" /></Card>;
  const previous = rows[0];
  const current = rows[rows.length - 1];
  const keys = Object.keys(metricLabels) as MetricKey[];

  return (
    <Row gutter={[16, 16]}>
      {keys.map((key) => {
        const currentValue = current[key];
        const previousValue = previous[key];
        const up = currentValue >= previousValue;
        return (
          <Col xs={24} sm={12} lg={8} key={key}>
            <Card>
              <div style={{ display: 'flex', justifyContent: 'space-between', gap: 12 }}>
                <span style={{ color: '#5f6b7a' }}>{metricLabels[key]}</span>
                <Tag color={up ? 'red' : 'green'}>{fmtPercent(currentValue, previousValue)}</Tag>
              </div>
              <div style={{ marginTop: 10, fontSize: 22, fontWeight: 700 }}>¥{fmtMoney(currentValue)}</div>
              <div style={{ color: '#8c8c8c', marginTop: 4 }}>{previous.month}：¥{fmtMoney(previousValue)}</div>
            </Card>
          </Col>
        );
      })}
    </Row>
  );
};

export default FinancialAnalysis;
