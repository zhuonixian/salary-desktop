import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Row,
  Col,
  Card,
  Statistic,
  DatePicker,
  Spin,
  message,
  Tag,
  Table,
  Space,
  Select,
  Button,
  Alert,
  Checkbox,
  Popconfirm,
  Popover,
} from 'antd';
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
  BellOutlined,
  RightOutlined,
  SettingOutlined,
} from '@ant-design/icons';
import dayjs from 'dayjs';
import { useNavigate } from 'react-router-dom';
import {
  clearDemoData,
  completeTaxFiling,
  dismissSetupChecklist,
  getBootstrapStatus,
  getDashboardReminders,
  getDashboardSummary,
  getMonthCloseWorkbench,
  getReminderAdvanceDays,
  getTaxCalendar,
  getTaxCalendarSettings,
  saveTaxCalendarSettings,
  setReminderAdvanceDays,
  undoTaxFiling,
} from '@/api';
import { SensitiveText } from '@/components/SensitiveText';
import { SensitiveStatistic } from '@/components/SensitiveStatistic';
import { useBusinessMonth } from '@/contexts/BusinessMonthContext';
import type {
  BootstrapStatus,
  DashboardSummary,
  MonthCloseCheckItem,
  MonthCloseWorkbench,
  ReminderCategory,
  ReminderItem,
  TaxCalendarItem,
} from '@/types';
import { TAX_CALENDAR_KIND_LABEL } from '@/types';

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

// ==================== 账期提醒（第八阶段 Task 4，spec 5） ====================

const reminderGroups: { category: ReminderCategory; label: string }[] = [
  { category: 'advance_due', label: '借款到期' },
  { category: 'instrument_due', label: '票据到期' },
  { category: 'payable_stuck', label: '滞留应付' },
  { category: 'ar_overdue', label: '应收账龄' },
];

const reminderRoute: Record<ReminderCategory, string> = {
  advance_due: '/advances',
  instrument_due: '/notes-instruments',
  payable_stuck: '/reimbursements',
  ar_overdue: '/ar-ledger',
};

// 借款/票据：逾期红、当天与临近橙；滞留应付：滞留即橙、满 30 天升级红；
// 应收账龄：仅账龄超阈值（>90 天）的销项未核销才提醒，红色
const reminderBadge = (item: ReminderItem): { color: string; text: string } => {
  if (item.category === 'payable_stuck') {
    return item.days_left >= 30
      ? { color: 'red', text: `滞留 ${item.days_left} 天` }
      : { color: 'gold', text: `滞留 ${item.days_left} 天` };
  }
  if (item.category === 'ar_overdue') {
    return { color: 'red', text: `账龄 ${item.days_left} 天` };
  }
  if (item.days_left < 0) return { color: 'red', text: `逾期 ${-item.days_left} 天` };
  if (item.days_left === 0) return { color: 'gold', text: '今日到期' };
  return { color: 'gold', text: `剩 ${item.days_left} 天` };
};

// ==================== 上手检查清单卡（第十阶段） ====================

const errText = (e: unknown): string => (e instanceof Error ? e.message : String(e));

interface ChecklistEntry {
  key: string;
  text: string;
  done: boolean;
  path: string;
}

const SetupChecklistCard: React.FC<{ onDataChanged: () => void }> = ({ onDataChanged }) => {
  const navigate = useNavigate();
  const [status, setStatus] = useState<BootstrapStatus | null>(null);
  const [clearing, setClearing] = useState(false);

  const fetchStatus = useCallback(async () => {
    try {
      setStatus(await getBootstrapStatus());
    } catch (e: unknown) {
      message.error('获取初始化状态失败: ' + errText(e));
    }
  }, []);

  useEffect(() => {
    void fetchStatus();
  }, [fetchStatus]);

  const handleDismiss = async () => {
    try {
      await dismissSetupChecklist();
      // 后端已持久化「不再显示」，本地同步隐藏即可。
      setStatus((prev) => (prev ? { ...prev, setup_checklist_dismissed: true } : prev));
    } catch (e: unknown) {
      message.error('操作失败: ' + errText(e));
    }
  };

  const handleClearDemo = async () => {
    setClearing(true);
    try {
      await clearDemoData();
      message.success('示例数据已清除');
      // 清除后刷新清单状态与整页仪表盘数据（不用 location.reload）。
      await fetchStatus();
      onDataChanged();
    } catch (e: unknown) {
      message.error('清除示例数据失败: ' + errText(e));
    } finally {
      setClearing(false);
    }
  };

  // 加载中 / 已永久隐藏 / 全部完成且无示例数据 → 不展示。
  if (!status) return null;
  const allDone =
    status.has_password &&
    status.has_employees &&
    status.has_social_profiles &&
    status.has_opening_balance &&
    status.has_locked_salary;
  if (status.setup_checklist_dismissed || (allDone && !status.has_demo_data)) return null;

  // has_salary_rules 恒真（后端预置默认规则），不进清单。
  const entries: ChecklistEntry[] = [
    { key: 'password', text: '设置启动密码', done: status.has_password, path: '/security' },
    { key: 'employees', text: '导入员工', done: status.has_employees, path: '/employees' },
    { key: 'social', text: '社保基数台账', done: status.has_social_profiles, path: '/social-insurance' },
    { key: 'opening', text: '期初余额', done: status.has_opening_balance, path: '/accounts' },
    { key: 'locked', text: '首次算薪并锁定', done: status.has_locked_salary, path: '/salary' },
  ];

  return (
    <Card
      size="small"
      title="上手检查清单"
      extra={
        <Button type="link" size="small" onClick={() => void handleDismiss()}>
          不再显示
        </Button>
      }
    >
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit, minmax(200px, 1fr))',
          gap: 8,
        }}
      >
        {entries.map((entry) => (
          <div
            key={entry.key}
            onClick={() => !entry.done && navigate(entry.path)}
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 8,
              padding: '8px 12px',
              borderRadius: 6,
              background: '#f7f9fc',
              cursor: entry.done ? 'default' : 'pointer',
            }}
          >
            {entry.done ? (
              <CheckCircleOutlined style={{ color: '#52c41a' }} />
            ) : (
              <RightOutlined style={{ color: '#faad14' }} />
            )}
            <span style={{ color: entry.done ? '#8c98a8' : '#1f2a37' }}>{entry.text}</span>
            {!entry.done && (
              <Tag color="gold" style={{ margin: '0 0 0 auto' }}>
                去完成
              </Tag>
            )}
          </div>
        ))}
      </div>
      {status.has_demo_data && (
        <Alert
          type="warning"
          showIcon
          style={{ marginTop: 12 }}
          message={
            <div
              style={{
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'space-between',
                flexWrap: 'wrap',
                gap: 8,
              }}
            >
              <span>当前为示例数据，体验完成后请清除</span>
              <Popconfirm
                title="确认清除示例数据？"
                description="仅清除示例数据产生的记录，不影响后续正式录入。"
                okText="清除"
                cancelText="取消"
                onConfirm={() => void handleClearDemo()}
              >
                <Button danger size="small" loading={clearing}>
                  清除示例数据
                </Button>
              </Popconfirm>
            </div>
          }
        />
      )}
    </Card>
  );
};

// ==================== 征期提醒卡（第十阶段） ====================

const TAX_CALENDAR_MONTHS = 3;

const taxKindOptions = Object.entries(TAX_CALENDAR_KIND_LABEL).map(([value, label]) => ({
  value,
  label,
}));

// done 灰 / 逾期红 / 15 日内金 / 其余默认
const taxBadge = (item: TaxCalendarItem): { color: string; text: string } => {
  if (item.status === 'done') return { color: 'default', text: '已完成' };
  if (item.overdue) return { color: 'red', text: `已逾期 ${-item.days_left} 天` };
  if (item.days_left <= 15) return { color: 'gold', text: `剩 ${item.days_left} 天` };
  return { color: 'default', text: `剩 ${item.days_left} 天` };
};

const TaxCalendarCard: React.FC = () => {
  const [items, setItems] = useState<TaxCalendarItem[]>([]);
  const [enabledKinds, setEnabledKinds] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const [savingSettings, setSavingSettings] = useState(false);
  const [actingKey, setActingKey] = useState<string | null>(null);

  const fetchCalendar = useCallback(async () => {
    try {
      setItems(await getTaxCalendar(dayjs().format('YYYY-MM'), TAX_CALENDAR_MONTHS));
    } catch (e: unknown) {
      message.error('获取征期日历失败: ' + errText(e));
    }
  }, []);

  const fetchAll = useCallback(async () => {
    setLoading(true);
    try {
      const [calendar, settings] = await Promise.all([
        getTaxCalendar(dayjs().format('YYYY-MM'), TAX_CALENDAR_MONTHS),
        getTaxCalendarSettings(),
      ]);
      setItems(calendar);
      setEnabledKinds(settings.enabled_kinds);
    } catch (e: unknown) {
      message.error('获取征期提醒失败: ' + errText(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void fetchAll();
  }, [fetchAll]);

  // 勾选即保存，保存后按新启用项刷新列表。
  const handleSettingsChange = async (kinds: string[]) => {
    setSavingSettings(true);
    try {
      await saveTaxCalendarSettings({ enabled_kinds: kinds });
      setEnabledKinds(kinds);
      await fetchCalendar();
      message.success('征期提醒设置已保存');
    } catch (e: unknown) {
      message.error('保存征期提醒设置失败: ' + errText(e));
    } finally {
      setSavingSettings(false);
    }
  };

  const handleFilingAction = async (item: TaxCalendarItem, action: 'complete' | 'undo') => {
    const key = `${item.period}|${item.kind}`;
    setActingKey(key);
    try {
      if (action === 'complete') {
        await completeTaxFiling(item.period, item.kind);
        message.success(`已标记 ${item.period} ${item.label} 为已完成`);
      } else {
        await undoTaxFiling(item.period, item.kind);
        message.success(`已撤销 ${item.period} ${item.label} 的完成标记`);
      }
      await fetchCalendar();
    } catch (e: unknown) {
      message.error('操作失败: ' + errText(e));
    } finally {
      setActingKey(null);
    }
  };

  return (
    <Card
      title={<Space><ScheduleOutlined />征期提醒</Space>}
      extra={
        <Popover
          trigger="click"
          placement="bottomRight"
          content={
            <div style={{ minWidth: 240 }}>
              <div style={{ fontWeight: 600, marginBottom: 8 }}>启用的征期类型</div>
              <Checkbox.Group
                value={enabledKinds}
                onChange={(values) => void handleSettingsChange(values.map(String))}
                style={{ display: 'grid', gap: 6 }}
              >
                {taxKindOptions.map((option) => (
                  <Checkbox key={option.value} value={option.value}>
                    {option.label}
                  </Checkbox>
                ))}
              </Checkbox.Group>
            </div>
          }
        >
          <Button type="text" size="small" icon={<SettingOutlined />} loading={savingSettings} />
        </Popover>
      }
    >
      <Spin spinning={loading}>
        {items.length === 0 ? (
          <div style={{ color: '#5f6b7a', padding: '12px 0' }}>
            近期暂无征期提醒，可在右上角设置启用的申报类型
          </div>
        ) : (
          <div style={{ display: 'grid', gap: 8 }}>
            {items.map((item) => {
              const badge = taxBadge(item);
              const key = `${item.period}|${item.kind}`;
              return (
                <div key={key}>
                  <div
                    style={{
                      display: 'grid',
                      gridTemplateColumns: '86px 1fr 110px 92px 92px',
                      alignItems: 'center',
                      gap: 8,
                      padding: '6px 8px',
                      borderRadius: 6,
                      background: '#f7f9fc',
                    }}
                  >
                    <span
                      style={{
                        fontVariantNumeric: 'tabular-nums',
                        color: '#5f6b7a',
                        overflow: 'hidden',
                        textOverflow: 'ellipsis',
                        whiteSpace: 'nowrap',
                      }}
                      title={item.period}
                    >
                      {item.period}
                    </span>
                    <span
                      style={{
                        color: item.status === 'done' ? '#8c98a8' : '#1f2a37',
                        overflow: 'hidden',
                        textOverflow: 'ellipsis',
                        whiteSpace: 'nowrap',
                      }}
                      title={item.label}
                    >
                      {item.label}
                    </span>
                    <span style={{ fontVariantNumeric: 'tabular-nums', color: '#5f6b7a' }}>
                      截止 {item.due_date}
                    </span>
                    <Tag color={badge.color} style={{ margin: 0, textAlign: 'center' }}>
                      {badge.text}
                    </Tag>
                    {item.status === 'pending' ? (
                      <Button
                        type="link"
                        size="small"
                        loading={actingKey === key}
                        onClick={() => void handleFilingAction(item, 'complete')}
                      >
                        标记已完成
                      </Button>
                    ) : (
                      <Button
                        type="link"
                        size="small"
                        loading={actingKey === key}
                        onClick={() => void handleFilingAction(item, 'undo')}
                      >
                        撤销
                      </Button>
                    )}
                  </div>
                  {item.hint && (
                    <div style={{ color: '#8c98a8', fontSize: 12, padding: '2px 8px 0' }}>
                      {item.hint}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}
        <div style={{ color: '#8c98a8', fontSize: 12, marginTop: 10 }}>
          如遇节假日顺延，以税局公告为准
        </div>
      </Spin>
    </Card>
  );
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
  const [reminders, setReminders] = useState<ReminderItem[]>([]);
  const [reminderDays, setReminderDays] = useState(7);

  const fetchSummary = useCallback(async (m: string) => {
    setLoading(true);
    try {
      const [dashboardData, monthCloseData, reminderItems, advanceDays] = await Promise.all([
        getDashboardSummary(m),
        getMonthCloseWorkbench(m),
        getDashboardReminders(),
        getReminderAdvanceDays(),
      ]);
      setSummary(dashboardData);
      setWorkbench(monthCloseData);
      setReminders(reminderItems);
      setReminderDays(advanceDays);
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      message.error('获取仪表盘数据失败: ' + msg);
    } finally {
      setLoading(false);
    }
  }, []);

  const handleReminderDaysChange = useCallback(async (days: number) => {
    try {
      await setReminderAdvanceDays(days);
      setReminderDays(days);
      setReminders(await getDashboardReminders());
      message.success(`账期提醒提前天数已设为 ${days} 天`);
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      message.error('设置提前天数失败: ' + msg);
    }
  }, []);

  useEffect(() => {
    fetchSummary(month.format('YYYY-MM'));
  }, [month, fetchSummary]);

  // 检查清单卡「清除示例数据」后整页数据重跑（不用 location.reload）。
  const reloadAfterDataChange = useCallback(() => {
    void fetchSummary(month.format('YYYY-MM'));
  }, [fetchSummary, month]);

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
        <Row className="mb-16">
          <Col span={24}>
            <SetupChecklistCard onDataChanged={reloadAfterDataChange} />
          </Col>
        </Row>

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

        <Row gutter={[16, 16]} className="mb-16">
          <Col xs={24} lg={12}>
            <Card
              title={<Space><BellOutlined />账期提醒</Space>}
              extra={
                <Space>
                  <span style={{ color: '#5f6b7a', fontSize: 13 }}>提前天数</span>
                  <Select
                    value={reminderDays}
                    onChange={handleReminderDaysChange}
                    style={{ width: 88 }}
                    options={[3, 7, 15, 30].map((d) => ({ value: d, label: `${d} 天` }))}
                  />
                </Space>
              }
            >
              {reminders.length === 0 ? (
                <div style={{ color: '#5f6b7a', padding: '12px 0' }}>暂无账期提醒</div>
              ) : (
                <Row gutter={[16, 16]}>
                  {reminderGroups.map((group) => {
                    const items = reminders.filter((r) => r.category === group.category);
                    return (
                      <Col span={24} key={group.category}>
                        <div style={{ fontWeight: 600, marginBottom: 10 }}>
                          {group.label}
                          <Tag
                            color={items.length > 0 ? 'orange' : 'green'}
                            style={{ marginLeft: 8 }}
                          >
                            {items.length} 项
                          </Tag>
                        </div>
                        {items.length === 0 ? (
                          <div style={{ color: '#8c98a8', fontSize: 13 }}>无到期或滞留项</div>
                        ) : (
                          <div style={{ display: 'grid', gap: 8 }}>
                            {items.map((item) => {
                              const badge = reminderBadge(item);
                              return (
                                <div
                                  key={`${item.category}-${item.ref_id}`}
                                  onClick={() => navigate(reminderRoute[item.category])}
                                  style={{
                                    display: 'grid',
                                    gridTemplateColumns: '104px 1fr 96px',
                                    alignItems: 'center',
                                    gap: 8,
                                    cursor: 'pointer',
                                    padding: '6px 8px',
                                    borderRadius: 6,
                                    background: '#f7f9fc',
                                  }}
                                >
                                  <Tag color={badge.color} style={{ margin: 0 }}>
                                    {badge.text}
                                  </Tag>
                                  <span
                                    style={{
                                      overflow: 'hidden',
                                      textOverflow: 'ellipsis',
                                      whiteSpace: 'nowrap',
                                      color: '#1f2a37',
                                    }}
                                    title={`${item.title}（${item.due_date}）`}
                                  >
                                    {item.title}
                                  </span>
                                  <span
                                    style={{
                                      textAlign: 'right',
                                      fontVariantNumeric: 'tabular-nums',
                                      color: '#5f6b7a',
                                    }}
                                  >
                                    <SensitiveText type="amount" value={item.amount ?? 0} />
                                  </span>
                                </div>
                              );
                            })}
                          </div>
                        )}
                      </Col>
                    );
                  })}
                </Row>
              )}
            </Card>
          </Col>
          <Col xs={24} lg={12}>
            <TaxCalendarCard />
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
