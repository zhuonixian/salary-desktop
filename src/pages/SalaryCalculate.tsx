import { useState, useEffect, useCallback } from 'react';
import {
  Table, Button, DatePicker, Tag, Modal, Form, InputNumber, Input, Space, message, Popconfirm, Alert,
} from 'antd';
import {
  CalculatorOutlined, LockOutlined, CheckCircleOutlined, ReloadOutlined, UnlockOutlined,
  PieChartOutlined, DownloadOutlined, PrinterOutlined,
} from '@ant-design/icons';
import { save } from '@tauri-apps/plugin-dialog';
import dayjs from 'dayjs';
import type { Dayjs } from 'dayjs';
import {
  getSalaryResults, calculateSalary, recalculateSingle,
  updateSalaryResult, lockSalary, unlockSalaryResults, reviewSalary,
  getEmployees, getAttendanceRecords, getSalaryRule, getTaxRules,
  getAnnualTaxSummary, exportAnnualTaxSummary,
} from '@/api';
import type { AnnualTaxSummaryRow, SalaryResult, SalaryResultUpdate, SalaryStatus } from '@/types';
import { SensitiveText } from '@/components/SensitiveText';
import { useSecurity } from '@/contexts/SecurityContext';

// 工资条明文金额格式化（发放核对用途，不走 SensitiveText）
const fmtMoney = (v: number): string => `¥ ${Number(v ?? 0).toFixed(2)}`;

const statusColorMap: Record<SalaryStatus, string> = {
  '草稿': 'default',
  '已复核': 'blue',
  '已锁定': 'green',
};

const SalaryCalculate: React.FC = () => {
  const [month, setMonth] = useState<Dayjs>(dayjs());
  const [results, setResults] = useState<SalaryResult[]>([]);
  const [loading, setLoading] = useState(false);
  const [calculating, setCalculating] = useState(false);
  const [adjustModalOpen, setAdjustModalOpen] = useState(false);
  const [adjustingRecord, setAdjustingRecord] = useState<SalaryResult | null>(null);
  const [adjustForm] = Form.useForm();
  const [unlockModal, setUnlockModal] = useState({ visible: false, password: '', reason: '', loading: false });
  const [unlockedMonths, setUnlockedMonths] = useState<Set<string>>(new Set());
  const [annualModalOpen, setAnnualModalOpen] = useState(false);
  const [annualYear, setAnnualYear] = useState<Dayjs>(dayjs());
  const [annualRows, setAnnualRows] = useState<AnnualTaxSummaryRow[]>([]);
  const [annualLoading, setAnnualLoading] = useState(false);
  const [annualExporting, setAnnualExporting] = useState(false);
  const [payslipOpen, setPayslipOpen] = useState(false);

  const { isSensitiveRevealed } = useSecurity();

  const fetchAnnualSummary = useCallback(async (year: number) => {
    setAnnualLoading(true);
    try {
      const data = await getAnnualTaxSummary(year);
      setAnnualRows(data);
    } catch (e: unknown) {
      message.error('获取个税年度汇总失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setAnnualLoading(false);
    }
  }, []);

  useEffect(() => {
    if (annualModalOpen) fetchAnnualSummary(annualYear.year());
  }, [annualModalOpen, annualYear, fetchAnnualSummary]);

  const handleAnnualExport = async () => {
    const year = annualYear.year();
    const path = await save({
      title: '导出个税年度汇总',
      defaultPath: `个税年度汇总_${year}.xlsx`,
      filters: [{ name: 'Excel', extensions: ['xlsx'] }],
    });
    if (!path) return;
    setAnnualExporting(true);
    try {
      await exportAnnualTaxSummary(year, String(path));
      message.success('个税年度汇总已导出');
    } catch (e: unknown) {
      message.error('导出个税年度汇总失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setAnnualExporting(false);
    }
  };

  const annualColumns = [
    { title: '工号', dataIndex: 'employee_no', key: 'employee_no', width: 90 },
    { title: '姓名', dataIndex: 'name', key: 'name', width: 80 },
    { title: '月数', dataIndex: 'month_count', key: 'month_count', width: 60, align: 'right' as const },
    {
      title: '累计收入', dataIndex: 'total_gross', key: 'total_gross', width: 110, align: 'right' as const,
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '累计社保个人', dataIndex: 'total_ss_personal', key: 'total_ss_personal', width: 120, align: 'right' as const,
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '累计公积金个人', dataIndex: 'total_hf_personal', key: 'total_hf_personal', width: 130, align: 'right' as const,
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '累计专项附加', dataIndex: 'total_special_deduction', key: 'total_special_deduction', width: 120, align: 'right' as const,
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '累计已预扣', dataIndex: 'total_tax_withheld', key: 'total_tax_withheld', width: 110, align: 'right' as const,
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '年度应预扣', dataIndex: 'annual_tax_due', key: 'annual_tax_due', width: 110, align: 'right' as const,
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '差额', dataIndex: 'difference', key: 'difference', width: 100, align: 'right' as const,
      render: (v: number) => (
        <span style={{ color: v < 0 ? '#52c41a' : v > 0 ? '#cf1322' : undefined }}>
          <SensitiveText type="amount" value={v} />
        </span>
      ),
    },
  ];

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
  const isControlUnlocked = unlockedMonths.has(monthStr);

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
      setUnlockedMonths((prev) => {
        if (!prev.has(monthStr)) return prev;
        const next = new Set(prev);
        next.delete(monthStr);
        return next;
      });
      fetchData(monthStr);
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      message.error('锁定失败: ' + msg);
    }
  };

  const handleUnlock = async () => {
    if (unlockModal.reason.trim().length < 5) {
      message.warning('请填写解锁原因（至少 5 个字）');
      return;
    }
    setUnlockModal((s) => ({ ...s, loading: true }));
    try {
      await unlockSalaryResults(unlockModal.password, monthStr, unlockModal.reason);
      message.success('已受控解锁，修改完成后请重新锁定');
      setUnlockModal({ visible: false, password: '', reason: '', loading: false });
      setUnlockedMonths((prev) => new Set(prev).add(monthStr));
      fetchData(monthStr);
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      message.error('解锁失败: ' + msg);
      // 解锁失败清空密码输入，避免误以为已解锁；原因保留便于修正后重试
      setUnlockModal((s) => ({ ...s, password: '' }));
    } finally {
      setUnlockModal((s) => ({ ...s, loading: false }));
    }
  };

  const mainColumns = [
    { title: '工号', dataIndex: 'employee_no', key: 'employee_no', width: 90, fixed: 'left' as const },
    { title: '姓名', dataIndex: 'employee_name', key: 'employee_name', width: 80, fixed: 'left' as const },
    { title: '部门', dataIndex: 'department', key: 'department', width: 80 },
    {
      title: '基本工资', dataIndex: 'base_salary', key: 'base_salary', width: 110, align: 'right' as const,
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '岗位工资', dataIndex: 'position_salary', key: 'position_salary', width: 110, align: 'right' as const,
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '绩效工资', dataIndex: 'performance_salary', key: 'performance_salary', width: 110, align: 'right' as const,
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '加班工资', dataIndex: 'overtime_pay', key: 'overtime_pay', width: 110, align: 'right' as const,
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '餐补', dataIndex: 'meal_allowance', key: 'meal_allowance', width: 90, align: 'right' as const,
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '交通补助', dataIndex: 'transport_allowance', key: 'transport_allowance', width: 100, align: 'right' as const,
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '其他补助', dataIndex: 'other_allowance', key: 'other_allowance', width: 100, align: 'right' as const,
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '应发工资', dataIndex: 'gross_salary', key: 'gross_salary', width: 120, align: 'right' as const,
      render: (v: number) => <span style={{ fontWeight: 600 }}><SensitiveText type="amount" value={v} /></span>,
    },
    {
      title: '社保扣款', dataIndex: 'social_insurance', key: 'social_insurance', width: 110, align: 'right' as const,
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '公积金扣款', dataIndex: 'housing_fund', key: 'housing_fund', width: 110, align: 'right' as const,
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '社保(单位)', dataIndex: 'social_insurance_employer', key: 'social_insurance_employer', width: 110, align: 'right' as const,
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '公积金(单位)', dataIndex: 'housing_fund_employer', key: 'housing_fund_employer', width: 120, align: 'right' as const,
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '考勤扣款', dataIndex: 'attendance_deduction', key: 'attendance_deduction', width: 110, align: 'right' as const,
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '个税', dataIndex: 'income_tax', key: 'income_tax', width: 100, align: 'right' as const,
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '其他扣款', dataIndex: 'other_deduction', key: 'other_deduction', width: 100, align: 'right' as const,
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '实发工资', dataIndex: 'net_salary', key: 'net_salary', width: 120, align: 'right' as const, fixed: 'right' as const,
      render: (v: number) => <span style={{ fontWeight: 600, color: '#52c41a' }}><SensitiveText type="amount" value={v} /></span>,
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
          {isLocked && (
            <Button
              danger
              icon={<UnlockOutlined />}
              onClick={() => setUnlockModal({ visible: true, password: '', reason: '', loading: false })}
            >
              受控解锁
            </Button>
          )}
          {isControlUnlocked && <Tag color="orange" style={{ marginRight: 0 }}>已受控解锁</Tag>}
          <Button
            icon={<PrinterOutlined />}
            onClick={() => setPayslipOpen(true)}
            disabled={results.length === 0}
          >
            工资条
          </Button>
          <Button
            icon={<PieChartOutlined />}
            onClick={() => setAnnualModalOpen(true)}
          >
            个税年度汇总
          </Button>
          {isReviewed && !isLocked && (
            <Popconfirm title="锁定后将无法修改，确认锁定?" onConfirm={handleLock} okText="确认锁定" cancelText="取消">
              <Button danger icon={<LockOutlined />}>
                {isControlUnlocked ? '重新锁定' : '锁定'}
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
                  <strong><SensitiveText type="amount" value={totals.gross} /></strong>
                </Table.Summary.Cell>
                <Table.Summary.Cell index={11} colSpan={5} />
                <Table.Summary.Cell index={16} align="right">
                  <strong style={{ color: '#52c41a' }}><SensitiveText type="amount" value={totals.net} /></strong>
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

      <Modal
        title="受控解锁工资"
        open={unlockModal.visible}
        confirmLoading={unlockModal.loading}
        okText="解锁"
        okButtonProps={{ danger: true }}
        cancelText="取消"
        onCancel={() => setUnlockModal({ visible: false, password: '', reason: '', loading: false })}
        onOk={handleUnlock}
      >
        <Alert
          type="error"
          showIcon
          message="高风险操作"
          description="解锁后该月工资恢复可编辑，已有计提凭证将作废；需输入启动密码，操作将完整记录到操作日志。修改完成后请重新锁定。月结前必须重新锁定，否则月结体检会阻塞。"
          style={{ marginBottom: 16 }}
        />
        <Form layout="vertical">
          <Form.Item label="启动密码" required>
            <Input.Password
              value={unlockModal.password}
              onChange={(e) => setUnlockModal((s) => ({ ...s, password: e.target.value }))}
            />
          </Form.Item>
          <Form.Item label="解锁原因" required extra="至少 5 个字，将记入操作日志">
            <Input.TextArea
              rows={3}
              value={unlockModal.reason}
              onChange={(e) => setUnlockModal((s) => ({ ...s, reason: e.target.value }))}
            />
          </Form.Item>
        </Form>
      </Modal>
      <Modal
        title={`个税年度汇总（${annualYear.year()}年度）`}
        open={annualModalOpen}
        onCancel={() => setAnnualModalOpen(false)}
        width={1000}
        footer={[
          <Button
            key="export"
            type="primary"
            icon={<DownloadOutlined />}
            loading={annualExporting}
            disabled={annualRows.length === 0}
            onClick={handleAnnualExport}
          >
            导出 Excel
          </Button>,
        ]}
      >
        <div style={{ marginBottom: 12, display: 'flex', justifyContent: 'space-between' }}>
          <DatePicker
            picker="year"
            value={annualYear}
            onChange={(d) => d && setAnnualYear(d)}
            allowClear={false}
            style={{ width: 120 }}
          />
          <span style={{ color: '#999', fontSize: 12 }}>差额 = 年度应预扣 - 累计已预扣，负数表示多缴</span>
        </div>
        <Table
          rowKey="employee_no"
          columns={annualColumns}
          dataSource={annualRows}
          loading={annualLoading}
          size="small"
          pagination={{ pageSize: 10, showTotal: (t) => `共 ${t} 条` }}
          scroll={{ x: 1000 }}
        />
      </Modal>
      <Modal
        open={payslipOpen}
        onCancel={() => setPayslipOpen(false)}
        title={`${monthStr} 工资条预览`}
        width={720}
        footer={[
          <Button key="print" type="primary" disabled={!isSensitiveRevealed} onClick={() => window.print()}>
            打印 / 另存 PDF
          </Button>,
        ]}
      >
        {!isSensitiveRevealed && (
          <Alert
            type="warning"
            showIcon
            message="工资条含明文金额，请先解锁敏感数据（点击任意金额的眼睛图标解锁）"
            style={{ marginBottom: 16 }}
          />
        )}
        <div className="payslip-print-area">
          {results.map((r) => (
            <div key={r.id} className="payslip-card">
              <div style={{ display: 'flex', justifyContent: 'space-between', fontWeight: 600, marginBottom: 8 }}>
                <span>{r.employee_name}（{r.employee_no}）</span>
                <span>{monthStr}</span>
              </div>
              <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 13 }}>
                <tbody>
                  {([
                    ['基本工资', r.base_salary], ['岗位工资', r.position_salary], ['绩效工资', r.performance_salary],
                    ['加班费', r.overtime_pay], ['餐补', r.meal_allowance], ['交通补贴', r.transport_allowance],
                    ['应发合计', r.gross_salary], ['社保(个人)', -r.social_insurance],
                    ['公积金(个人)', -r.housing_fund], ['考勤扣款', -r.attendance_deduction],
                    ['个税', -r.income_tax], ['其他扣款', -r.other_deduction],
                  ] as [string, number][]).map(([label, value]) => (
                    <tr key={label}>
                      <td style={{ border: '1px solid #d9d9d9', padding: '2px 8px', width: '50%' }}>{label}</td>
                      <td style={{ border: '1px solid #d9d9d9', padding: '2px 8px', textAlign: 'right' }}>
                        {fmtMoney(value)}
                      </td>
                    </tr>
                  ))}
                  <tr>
                    <td style={{ border: '1px solid #333', padding: '4px 8px', fontWeight: 700 }}>实发工资</td>
                    <td style={{ border: '1px solid #333', padding: '4px 8px', textAlign: 'right', fontWeight: 700 }}>
                      {fmtMoney(r.net_salary)}
                    </td>
                  </tr>
                </tbody>
              </table>
            </div>
          ))}
        </div>
      </Modal>
    </div>
  );
};

export default SalaryCalculate;
