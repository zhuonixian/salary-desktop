import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Button,
  Card,
  Col,
  DatePicker,
  Row,
  Select,
  Space,
  Spin,
  Switch,
  Table,
  Tag,
  Tooltip,
  message,
} from 'antd';
import type { ColumnsType } from 'antd/es/table';
import { DownloadOutlined, ReloadOutlined } from '@ant-design/icons';
import { save } from '@tauri-apps/plugin-dialog';
import { exportFundJournal, getFundAccounts, getFundJournal } from '@/api';
import SensitiveText from '@/components/SensitiveText';
import SensitiveStatistic from '@/components/SensitiveStatistic';
import { useBusinessMonth } from '@/contexts/BusinessMonthContext';
import { useSecurity } from '@/contexts/SecurityContext';
import type { FundAccount, FundJournal, FundJournalRow } from '@/types';

const SOURCE_TYPE_LABEL: Record<string, string> = {
  salary_accrual: '工资计提',
  salary_payment: '工资付款',
  reimbursement_accrual: '报销计提',
  reimbursement_payment: '报销付款',
  invoice_expense: '费用报销',
  bank_manual: '流水入账',
  period_close: '年末结转',
  fund_document: '资金单据',
};

const RECONCILE_META: Record<string, { text: string; color: string }> = {
  allocated: { text: '已核销', color: 'green' },
  partial: { text: '部分核销', color: 'orange' },
  unallocated: { text: '未核销', color: 'default' },
};

const fmtMoney = (value?: number | null) =>
  (value ?? 0).toLocaleString('zh-CN', { minimumFractionDigits: 2, maximumFractionDigits: 2 });

/** 资金日记账（spec 6.1）：账户 + 月份区间的账面滚动余额账，可导出（敏感导出需解锁） */
const FundJournals: React.FC = () => {
  const { month } = useBusinessMonth();
  const { isSensitiveRevealed } = useSecurity();
  const monthStr = month.format('YYYY-MM');
  const [accounts, setAccounts] = useState<FundAccount[]>([]);
  const [accountId, setAccountId] = useState<number | undefined>(undefined);
  const [allHistory, setAllHistory] = useState(false);
  const [journal, setJournal] = useState<FundJournal | null>(null);
  const [loading, setLoading] = useState(false);
  const [exporting, setExporting] = useState(false);

  useEffect(() => {
    getFundAccounts({ is_active: true })
      .then((list) => {
        setAccounts(list);
        setAccountId((prev) => prev ?? list[0]?.id);
      })
      .catch((e: unknown) =>
        message.error('获取资金账户失败: ' + (e instanceof Error ? e.message : String(e))),
      );
  }, []);

  const fetchData = useCallback(async () => {
    if (!accountId) {
      setJournal(null);
      return;
    }
    setLoading(true);
    try {
      const data = await getFundJournal({
        fund_account_id: accountId,
        from_month: allHistory ? undefined : monthStr,
        to_month: monthStr,
      });
      setJournal(data);
    } catch (e: unknown) {
      message.error('查询资金日记账失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setLoading(false);
    }
  }, [accountId, allHistory, monthStr]);

  useEffect(() => {
    void fetchData();
  }, [fetchData]);

  const handleExport = async () => {
    if (!accountId || !journal) {
      message.warning('请先选择资金账户');
      return;
    }
    const defaultName = `${journal.account_type === 'cash' ? '现金日记账' : '银行存款日记账'}_${monthStr.replace('-', '')}.xlsx`;
    const target = await save({
      defaultPath: defaultName,
      filters: [{ name: '日记账', extensions: ['xlsx'] }],
    });
    if (!target) return;
    setExporting(true);
    try {
      await exportFundJournal(
        {
          fund_account_id: accountId,
          from_month: allHistory ? undefined : monthStr,
          to_month: monthStr,
        },
        String(target),
      );
      message.success('日记账已导出');
    } catch (e: unknown) {
      message.error('导出失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setExporting(false);
    }
  };

  const columns: ColumnsType<FundJournalRow> = useMemo(
    () => [
      { title: '日期', dataIndex: 'voucher_date', key: 'voucher_date', width: 100 },
      { title: '凭证号', dataIndex: 'voucher_no', key: 'voucher_no', width: 130 },
      {
        title: '来源',
        dataIndex: 'source_type',
        key: 'source_type',
        width: 90,
        render: (t: string) => SOURCE_TYPE_LABEL[t] ?? t,
      },
      { title: '摘要', dataIndex: 'summary', key: 'summary', width: 200, ellipsis: true },
      {
        title: '对方单位',
        dataIndex: 'partner_name',
        key: 'partner_name',
        width: 130,
        ellipsis: true,
        render: (v?: string) => v || '-',
      },
      {
        title: '收入',
        dataIndex: 'income_amount',
        key: 'income_amount',
        width: 120,
        align: 'right',
        render: (v: number) => (v > 0 ? <SensitiveText type="amount" value={v} /> : '-'),
      },
      {
        title: '支出',
        dataIndex: 'expense_amount',
        key: 'expense_amount',
        width: 120,
        align: 'right',
        render: (v: number) => (v > 0 ? <SensitiveText type="amount" value={v} /> : '-'),
      },
      {
        title: '余额',
        dataIndex: 'balance',
        key: 'balance',
        width: 140,
        align: 'right',
        render: (v: number) => <SensitiveText type="amount" value={v} />,
      },
      {
        title: '对账状态',
        dataIndex: 'reconcile_status',
        key: 'reconcile_status',
        width: 100,
        render: (s: string) => {
          const meta = RECONCILE_META[s] ?? { text: s, color: 'default' };
          return <Tag color={meta.color}>{meta.text}</Tag>;
        },
      },
    ],
    [],
  );

  return (
    <div>
      <div className="page-header">
        <span className="page-title">资金日记账</span>
        <div className="page-header-actions">
          <Select
            placeholder="选择资金账户"
            showSearch
            optionFilterProp="label"
            value={accountId}
            onChange={setAccountId}
            style={{ width: 220 }}
            options={accounts.map((a) => ({
              value: a.id,
              label: `${a.name}（${a.account_code}）`,
            }))}
          />
          <DatePicker
            picker="month"
            value={month}
            allowClear={false}
            style={{ width: 140 }}
            disabled
          />
          <Tooltip title="开启后自账户期初起显示全部历史（期初为账户期初余额）">
            <Space size={4}>
              <span style={{ color: '#666' }}>全部历史</span>
              <Switch size="small" checked={allHistory} onChange={setAllHistory} />
            </Space>
          </Tooltip>
          <Button icon={<ReloadOutlined />} loading={loading} onClick={() => void fetchData()}>
            刷新
          </Button>
          <Tooltip title={isSensitiveRevealed ? '' : '敏感导出需先在页面中解锁敏感数据'}>
            <Button
              type="primary"
              icon={<DownloadOutlined />}
              disabled={!isSensitiveRevealed}
              loading={exporting}
              onClick={handleExport}
            >
              导出
            </Button>
          </Tooltip>
        </div>
      </div>

      <Spin spinning={loading}>
        <Row gutter={[16, 16]} className="mb-16">
          <Col xs={24} sm={12} lg={6}>
            <Card className="stat-card">
              <SensitiveStatistic title="期初余额" value={journal?.opening_balance ?? 0} />
            </Card>
          </Col>
          <Col xs={24} sm={12} lg={6}>
            <Card className="stat-card">
              <SensitiveStatistic title="本期收入" value={journal?.total_income ?? 0} />
            </Card>
          </Col>
          <Col xs={24} sm={12} lg={6}>
            <Card className="stat-card">
              <SensitiveStatistic title="本期支出" value={journal?.total_expense ?? 0} />
            </Card>
          </Col>
          <Col xs={24} sm={12} lg={6}>
            <Card className="stat-card">
              <SensitiveStatistic title="期末余额" value={journal?.closing_balance ?? 0} />
            </Card>
          </Col>
        </Row>

        <Card
          title={`${journal?.fund_account_name ?? '资金日记账'}（${allHistory ? `至 ${monthStr}` : monthStr}）`}
        >
          <Table
            rowKey="voucher_line_id"
            columns={columns}
            dataSource={journal?.rows ?? []}
            pagination={{ pageSize: 15, showSizeChanger: true }}
            scroll={{ x: 1130 }}
            footer={() => (
              <span style={{ color: '#8c8c8c' }}>
                余额 = 期初 {fmtMoney(journal?.opening_balance)} 按日期+凭证号顺序累计收支；
                账户期初余额与历史月份自动滚入
              </span>
            )}
          />
        </Card>
      </Spin>
    </div>
  );
};

export default FundJournals;
