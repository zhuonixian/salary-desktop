import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Button,
  Card,
  Col,
  DatePicker,
  Row,
  Select,
  Spin,
  Statistic,
  Table,
  Tag,
  message,
} from 'antd';
import { ReloadOutlined } from '@ant-design/icons';
import dayjs from 'dayjs';
import type { Dayjs } from 'dayjs';
import { getVouchers } from '@/api';
import type { Voucher, VoucherLine } from '@/types';
import { VOUCHER_SOURCE_LABEL } from '@/types';
import { SensitiveText } from '@/components/SensitiveText';
import { SensitiveStatistic } from '@/components/SensitiveStatistic';

const sourceTagColor: Record<string, string> = {
  salary_accrual: 'blue',
  salary_payment: 'geekblue',
  reimbursement_accrual: 'cyan',
  reimbursement_payment: 'green',
  invoice_expense: 'purple',
  bank_manual: 'orange',
};

const sourceText = (sourceType: string) =>
  VOUCHER_SOURCE_LABEL[sourceType] ?? sourceType;

const voucherStatusMeta: Record<Voucher['status'], { text: string; color: string }> = {
  active: { text: '有效', color: 'green' },
  void: { text: '已作废', color: 'red' },
};

const Vouchers: React.FC = () => {
  const [month, setMonth] = useState<Dayjs>(dayjs());
  const [sourceFilter, setSourceFilter] = useState<string | undefined>(undefined);
  const [statusFilter, setStatusFilter] = useState<string | undefined>(undefined);
  const [vouchers, setVouchers] = useState<Voucher[]>([]);
  const [loading, setLoading] = useState(false);

  const fetchData = useCallback(async () => {
    setLoading(true);
    try {
      setVouchers(await getVouchers({
        month: month.format('YYYY-MM'),
        source_type: sourceFilter,
        status: statusFilter,
      }));
    } catch (e: unknown) {
      message.error('查询记账凭证失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setLoading(false);
    }
  }, [month, sourceFilter, statusFilter]);

  useEffect(() => {
    fetchData();
  }, [fetchData]);

  const summary = useMemo(() => ({
    total: vouchers.length,
    activeCount: vouchers.filter((item) => item.status === 'active').length,
    voidCount: vouchers.filter((item) => item.status === 'void').length,
    activeAmount: vouchers
      .filter((item) => item.status === 'active')
      .reduce((sum, item) => sum + item.total_amount, 0),
  }), [vouchers]);

  const columns = [
    { title: '凭证号', dataIndex: 'voucher_no', key: 'voucher_no', width: 170, fixed: 'left' as const },
    { title: '凭证日期', dataIndex: 'voucher_date', key: 'voucher_date', width: 110 },
    { title: '月份', dataIndex: 'belong_month', key: 'belong_month', width: 90 },
    {
      title: '来源',
      dataIndex: 'source_type',
      key: 'source_type',
      width: 100,
      render: (sourceType: string) => (
        <Tag color={sourceTagColor[sourceType] ?? 'default'}>{sourceText(sourceType)}</Tag>
      ),
    },
    {
      title: '金额',
      dataIndex: 'total_amount',
      key: 'total_amount',
      width: 130,
      align: 'right' as const,
      render: (value: number) => <SensitiveText type="amount" value={value} />,
    },
    {
      title: '状态',
      dataIndex: 'status',
      key: 'status',
      width: 90,
      render: (status: Voucher['status']) => (
        <Tag color={voucherStatusMeta[status].color}>{voucherStatusMeta[status].text}</Tag>
      ),
    },
    { title: '摘要/备注', dataIndex: 'remark', key: 'remark', ellipsis: true },
  ];

  const lineColumns = [
    { title: '序号', dataIndex: 'line_order', key: 'line_order', width: 60 },
    { title: '科目编码', dataIndex: 'account_code', key: 'account_code', width: 110 },
    {
      title: '借方金额',
      dataIndex: 'debit_amount',
      key: 'debit_amount',
      width: 130,
      align: 'right' as const,
      render: (value: number) => (value > 0 ? <SensitiveText type="amount" value={value} /> : '-'),
    },
    {
      title: '贷方金额',
      dataIndex: 'credit_amount',
      key: 'credit_amount',
      width: 130,
      align: 'right' as const,
      render: (value: number) => (value > 0 ? <SensitiveText type="amount" value={value} /> : '-'),
    },
    { title: '摘要', dataIndex: 'summary', key: 'summary', ellipsis: true },
  ];

  return (
    <div>
      <div className="page-header">
        <span className="page-title">记账凭证</span>
        <div className="page-header-actions">
          <DatePicker
            picker="month"
            value={month}
            allowClear={false}
            onChange={(value) => value && setMonth(value)}
            style={{ width: 160 }}
          />
          <Select
            placeholder="凭证来源"
            allowClear
            value={sourceFilter}
            onChange={setSourceFilter}
            style={{ width: 140 }}
            options={Object.entries(VOUCHER_SOURCE_LABEL).map(([value, label]) => ({ value, label }))}
          />
          <Select
            placeholder="凭证状态"
            allowClear
            value={statusFilter}
            onChange={setStatusFilter}
            style={{ width: 130 }}
            options={[
              { value: 'active', label: '有效' },
              { value: 'void', label: '已作废' },
            ]}
          />
          <Button icon={<ReloadOutlined />} loading={loading} onClick={fetchData}>
            刷新
          </Button>
        </div>
      </div>

      <Spin spinning={loading}>
        <Row gutter={[16, 16]} className="mb-16">
          <Col xs={24} sm={12} lg={6}>
            <Card className="stat-card"><Statistic title="凭证数" value={summary.total} /></Card>
          </Col>
          <Col xs={24} sm={12} lg={6}>
            <Card className="stat-card"><Statistic title="有效凭证" value={summary.activeCount} /></Card>
          </Col>
          <Col xs={24} sm={12} lg={6}>
            <Card className="stat-card"><Statistic title="已作废" value={summary.voidCount} /></Card>
          </Col>
          <Col xs={24} sm={12} lg={6}>
            <Card className="stat-card">
              <SensitiveStatistic title="有效凭证金额" value={summary.activeAmount} />
            </Card>
          </Col>
        </Row>

        <Card>
          <Table<Voucher>
            rowKey="id"
            columns={columns}
            dataSource={vouchers}
            pagination={{ pageSize: 10 }}
            scroll={{ x: 900 }}
            expandable={{
              expandedRowRender: (record: Voucher) => (
                <Table<VoucherLine>
                  rowKey="id"
                  columns={lineColumns}
                  dataSource={[...record.lines].sort((a, b) => a.line_order - b.line_order)}
                  pagination={false}
                  size="small"
                />
              ),
            }}
          />
        </Card>
      </Spin>
    </div>
  );
};

export default Vouchers;
