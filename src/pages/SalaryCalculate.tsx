import { useState, useEffect, useCallback } from 'react';
import {
  Table, Button, DatePicker, Tag, Modal, Form, InputNumber, Input, Space, message, Spin, Popconfirm,
} from 'antd';
import {
  CalculatorOutlined, LockOutlined, CheckCircleOutlined, ReloadOutlined,
} from '@ant-design/icons';
import dayjs from 'dayjs';
import type { Dayjs } from 'dayjs';
import {
  getSalaryResults, calculateSalary, recalculateSingle,
  updateSalaryResult, lockSalary, reviewSalary,
  getEmployees, getAttendanceRecords, getSalaryRule, getTaxRules,
} from '@/api';
import type { SalaryResult, SalaryResultUpdate, SalaryStatus } from '@/types';

const statusColorMap: Record<SalaryStatus, string> = {
  '草稿': 'default',
  '已复核': 'blue',
  '已锁定': 'green',
};

const fmt = (val?: number | null) => (val ?? 0).toLocaleString('zh-CN', { minimumFractionDigits: 2, maximumFractionDigits: 2 });

const SalaryCalculate: React.FC = () => {
  const [month, setMonth] = useState<Dayjs>(dayjs());
  const [results, setResults] = useState<SalaryResult[]>([]);
  const [loading, setLoading] = useState(false);
  const [calculating, setCalculating] = useState(false);
  const [adjustModalOpen, setAdjustModalOpen] = useState(false);
  const [adjustingRecord, setAdjustingRecord] = useState<SalaryResult | null>(null);
  const [adjustForm] = Form.useForm();

  const fetchData = useCallback(async (m: string) => {
    setLoading(true);
    try {
      const data = await getSalaryResults(m);
      setResults(data);
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      message.error('获取工资数据失败: ' + msg);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchData(month.format('YYYY-MM'));
  }, [month, fetchData]);

  const monthStr = month.format('YYYY-MM');
  const isLocked = results.length > 0 && results.every((r) => r.status === '已锁定');
  const isReviewed = results.length > 0 && results.some((r) => r.status === '已复核');

  const handleCalculate = async () => {
    setCalculating(true);
    try {
      if (isLocked) {
        message.warning('本月工资已锁定，不能重新计算');
        return;
      }

      const [employees, attendanceRecords, salaryRule, taxRules] = await Promise.all([
        getEmployees(),
        getAttendanceRecords(monthStr),
        getSalaryRule(),
        getTaxRules(),
      ]);

      if (employees.length === 0) {
        message.warning('请先在员工管理中新增或导入员工');
        return;
      }
      if (!employees.some((employee) => employee.status === '在职')) {
        message.warning('当前没有在职员工，无法计算工资');
        return;
      }
      if (attendanceRecords.length === 0) {
        message.warning('请先导入或确认本月考勤数据');
        return;
      }
      if (!salaryRule || taxRules.length === 0) {
        message.warning('请先完成工资规则和个税规则配置');
        return;
      }

      const data = await calculateSalary(monthStr);
      setResults(data);
      if (data.length === 0) {
        message.warning('没有生成工资结果，请检查员工和考勤数据是否匹配');
      } else {
        message.success('计算完成');
      }
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      message.error('计算失败: ' + msg);
    } finally {
      setCalculating(false);
    }
  };

  const handleRecalculateSingle = async (employeeId: number) => {
    try {
      const updated = await recalculateSingle(monthStr, employeeId);
      setResults((prev) => prev.map((r) => (r.employee_id === employeeId ? updated : r)));
      message.success('重算完成');
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      message.error('重算失败: ' + msg);
    }
  };

  const handleAdjust = (record: SalaryResult) => {
    setAdjustingRecord(record);
    adjustForm.setFieldsValue({
      other_allowance: record.other_allowance,
      other_deduction: record.other_deduction,
      remark: record.remark,
    });
    setAdjustModalOpen(true);
  };

  const handleAdjustSubmit = async () => {
    if (!adjustingRecord) return;
    try {
      const values = await adjustForm.validateFields();
      const updated = await updateSalaryResult(adjustingRecord.id, values as SalaryResultUpdate);
      setResults((prev) => prev.map((r) => (r.id === updated.id ? { ...r, ...values } : r)));
      message.success('调整成功');
      setAdjustModalOpen(false);
    } catch (e: unknown) {
      if (e instanceof Error) {
        message.error('调整失败: ' + e.message);
      }
    }
  };

  const handleReview = async () => {
    try {
      await reviewSalary(monthStr);
      message.success('复核成功');
      fetchData(monthStr);
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      message.error('复核失败: ' + msg);
    }
  };

  const handleLock = async () => {
    try {
      await lockSalary(monthStr);
      message.success('锁定成功');
      fetchData(monthStr);
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      message.error('锁定失败: ' + msg);
    }
  };

  const mainColumns = [
    { title: '工号', dataIndex: 'employee_no', key: 'employee_no', width: 90, fixed: 'left' as const },
    { title: '姓名', dataIndex: 'employee_name', key: 'employee_name', width: 80, fixed: 'left' as const },
    { title: '部门', dataIndex: 'department', key: 'department', width: 80 },
    {
      title: '基本工资', dataIndex: 'base_salary', key: 'base_salary', width: 100, align: 'right' as const,
      render: (v: number) => fmt(v),
    },
    {
      title: '岗位工资', dataIndex: 'position_salary', key: 'position_salary', width: 100, align: 'right' as const,
      render: (v: number) => fmt(v),
    },
    {
      title: '绩效工资', dataIndex: 'performance_salary', key: 'performance_salary', width: 100, align: 'right' as const,
      render: (v: number) => fmt(v),
    },
    {
      title: '加班工资', dataIndex: 'overtime_pay', key: 'overtime_pay', width: 100, align: 'right' as const,
      render: (v: number) => fmt(v),
    },
    {
      title: '餐补', dataIndex: 'meal_allowance', key: 'meal_allowance', width: 80, align: 'right' as const,
      render: (v: number) => fmt(v),
    },
    {
      title: '交通补助', dataIndex: 'transport_allowance', key: 'transport_allowance', width: 90, align: 'right' as const,
      render: (v: number) => fmt(v),
    },
    {
      title: '其他补助', dataIndex: 'other_allowance', key: 'other_allowance', width: 90, align: 'right' as const,
      render: (v: number) => fmt(v),
    },
    {
      title: '应发工资', dataIndex: 'gross_salary', key: 'gross_salary', width: 110, align: 'right' as const,
      render: (v: number) => <span style={{ fontWeight: 600 }}>{fmt(v)}</span>,
    },
    {
      title: '社保扣款', dataIndex: 'social_insurance', key: 'social_insurance', width: 100, align: 'right' as const,
      render: (v: number) => fmt(v),
    },
    {
      title: '公积金扣款', dataIndex: 'housing_fund', key: 'housing_fund', width: 100, align: 'right' as const,
      render: (v: number) => fmt(v),
    },
    {
      title: '考勤扣款', dataIndex: 'attendance_deduction', key: 'attendance_deduction', width: 100, align: 'right' as const,
      render: (v: number) => fmt(v),
    },
    {
      title: '个税', dataIndex: 'income_tax', key: 'income_tax', width: 90, align: 'right' as const,
      render: (v: number) => fmt(v),
    },
    {
      title: '其他扣款', dataIndex: 'other_deduction', key: 'other_deduction', width: 90, align: 'right' as const,
      render: (v: number) => fmt(v),
    },
    {
      title: '实发工资', dataIndex: 'net_salary', key: 'net_salary', width: 110, align: 'right' as const, fixed: 'right' as const,
      render: (v: number) => <span style={{ fontWeight: 600, color: '#52c41a' }}>{fmt(v)}</span>,
    },
    {
      title: '状态', dataIndex: 'status', key: 'status', width: 80, fixed: 'right' as const,
      render: (status: SalaryStatus) => <Tag color={statusColorMap[status]}>{status}</Tag>,
    },
    {
      title: '操作', key: 'action', width: 160, fixed: 'right' as const,
      render: (_: unknown, record: SalaryResult) => (
        <Space size={4}>
          <Button type="link" size="small" onClick={() => handleAdjust(record)} disabled={record.status === '已锁定'}>
            调整
          </Button>
          <Popconfirm
            title="确认重算该员工?"
            onConfirm={() => handleRecalculateSingle(record.employee_id)}
            okText="确认"
            cancelText="取消"
          >
            <Button type="link" size="small" icon={<ReloadOutlined />} disabled={record.status === '已锁定'}>
              重算
            </Button>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  const expandedRowRender = (record: SalaryResult) => (
    <div style={{ padding: '8px 16px' }}>
      <p style={{ marginBottom: 4 }}><strong>备注：</strong>{record.remark || '无'}</p>
      <p style={{ marginBottom: 4, color: '#999', fontSize: 12 }}>
        创建时间: {record.created_at} | 更新时间: {record.updated_at}
      </p>
    </div>
  );

  return (
    <div>
      <div className="page-header">
        <span className="page-title">月度工资计算</span>
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
            icon={<CalculatorOutlined />}
            onClick={handleCalculate}
            loading={calculating}
            disabled={isLocked}
          >
            一键计算
          </Button>
          {!isLocked && results.length > 0 && (
            <Button
              icon={<CheckCircleOutlined />}
              onClick={handleReview}
              disabled={isReviewed}
            >
              复核
            </Button>
          )}
          {isReviewed && !isLocked && (
            <Popconfirm title="锁定后将无法修改，确认锁定?" onConfirm={handleLock} okText="确认锁定" cancelText="取消">
              <Button danger icon={<LockOutlined />}>
                锁定
              </Button>
            </Popconfirm>
          )}
        </div>
      </div>

      <Table
        rowKey="id"
        columns={mainColumns}
        dataSource={results}
        loading={loading}
        pagination={{ pageSize: 20, showSizeChanger: true, showTotal: (t) => `共 ${t} 条` }}
        scroll={{ x: 2200 }}
        size="middle"
        expandable={{ expandedRowRender }}
        summary={(data) => {
          if (data.length === 0) return null;
          const totals = data.reduce(
            (acc, r) => ({
              gross: acc.gross + r.gross_salary,
              deduction: acc.deduction + r.total_deduction,
              net: acc.net + r.net_salary,
            }),
            { gross: 0, deduction: 0, net: 0 }
          );
          return (
            <Table.Summary fixed>
              <Table.Summary.Row>
                <Table.Summary.Cell index={0} colSpan={10}>
                  <strong>合计</strong>
                </Table.Summary.Cell>
                <Table.Summary.Cell index={10} align="right">
                  <strong>{fmt(totals.gross)}</strong>
                </Table.Summary.Cell>
                <Table.Summary.Cell index={11} colSpan={5} />
                <Table.Summary.Cell index={16} align="right">
                  <strong style={{ color: '#52c41a' }}>{fmt(totals.net)}</strong>
                </Table.Summary.Cell>
                <Table.Summary.Cell index={17} colSpan={2} />
              </Table.Summary.Row>
            </Table.Summary>
          );
        }}
      />

      <Modal
        title={`调整 - ${adjustingRecord?.employee_name || ''}`}
        open={adjustModalOpen}
        onOk={handleAdjustSubmit}
        onCancel={() => setAdjustModalOpen(false)}
        width={480}
        destroyOnClose
        okText="保存"
        cancelText="取消"
      >
        <Form form={adjustForm} layout="vertical">
          <Form.Item name="other_allowance" label="其他补助">
            <InputNumber min={0} precision={2} style={{ width: '100%' }} addonAfter="元" />
          </Form.Item>
          <Form.Item name="other_deduction" label="其他扣款">
            <InputNumber min={0} precision={2} style={{ width: '100%' }} addonAfter="元" />
          </Form.Item>
          <Form.Item name="remark" label="备注">
            <Input.TextArea rows={3} placeholder="备注信息" />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  );
};

export default SalaryCalculate;
