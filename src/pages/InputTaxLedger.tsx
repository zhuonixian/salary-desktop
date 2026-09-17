import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Alert,
  Button,
  Card,
  Col,
  DatePicker,
  Row,
  Space,
  Spin,
  Statistic,
  Table,
  Tooltip,
  message,
} from 'antd';
import type { ColumnsType } from 'antd/es/table';
import { DownloadOutlined, ReloadOutlined } from '@ant-design/icons';
import { save } from '@tauri-apps/plugin-dialog';
import type { Dayjs } from 'dayjs';
import { exportInputTaxLedger, getInputTaxLedger } from '@/api';
import { SensitiveText } from '@/components/SensitiveText';
import { SensitiveStatistic } from '@/components/SensitiveStatistic';
import { useBusinessMonth } from '@/contexts/BusinessMonthContext';
import { useSecurity } from '@/contexts/SecurityContext';
import type { InputTaxLedgerMonthlySubtotal, InputTaxLedgerReport, InputTaxLedgerRow } from '@/types';

const monthPickerStyle = { width: 140 };

const fmtMoney = (value: number) =>
  (value ?? 0).toLocaleString('zh-CN', { minimumFractionDigits: 2, maximumFractionDigits: 2 });

/** 增值税进项台账（第八阶段 Task 10，spec 9）：发票维度区间查询（排除 void）+ 月度小计 +
 *  区间合计 + Excel 导出（敏感门禁）。口径提示：发票登记 ≠ 进项认证，认证状态以税务系统为准。 */
const InputTaxLedgerPage: React.FC = () => {
  const { month } = useBusinessMonth();
  const { isSensitiveRevealed } = useSecurity();
  const [fromMonth, setFromMonth] = useState<Dayjs>(month);
  const [toMonth, setToMonth] = useState<Dayjs>(month);
  const [report, setReport] = useState<InputTaxLedgerReport | null>(null);
  const [loading, setLoading] = useState(false);
  const [exporting, setExporting] = useState(false);

  const fetchData = useCallback(async () => {
    setLoading(true);
    try {
      const data = await getInputTaxLedger(
        fromMonth.format('YYYY-MM'),
        toMonth.format('YYYY-MM'),
      );
      setReport(data);
    } catch (e: unknown) {
      message.error('查询进项台账失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setLoading(false);
    }
  }, [fromMonth, toMonth]);

  useEffect(() => {
    void fetchData();
  }, [fetchData]);

  /** 导出（敏感门禁与既有导出一致）：进项台账_YYYYMM-YYYYMM.xlsx */
  const handleExport = async () => {
    const from = fromMonth.format('YYYY-MM');
    const to = toMonth.format('YYYY-MM');
    const target = await save({
      defaultPath: `进项台账_${from.replace('-', '')}-${to.replace('-', '')}.xlsx`,
      filters: [{ name: '进项台账', extensions: ['xlsx'] }],
    });
    if (!target) return;
    setExporting(true);
    try {
      await exportInputTaxLedger(from, to, String(target));
      message.success('进项台账已导出');
    } catch (e: unknown) {
      message.error('导出进项台账失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setExporting(false);
    }
  };

  const columns: ColumnsType<InputTaxLedgerRow> = useMemo(
    () => [
      { title: '所属月份', dataIndex: 'belong_month', key: 'belong_month', width: 100 },
      {
        title: '发票代码',
        dataIndex: 'invoice_code',
        key: 'invoice_code',
        width: 150,
        render: (v?: string) => v || '-',
      },
      {
        title: '发票号码',
        dataIndex: 'invoice_number',
        key: 'invoice_number',
        width: 120,
        render: (v?: string) => v || '-',
      },
      {
        title: '开票日期',
        dataIndex: 'issue_date',
        key: 'issue_date',
        width: 110,
        render: (v?: string) => v || '-',
      },
      {
        title: '销方名称',
        dataIndex: 'seller_name',
        key: 'seller_name',
        width: 200,
        ellipsis: true,
        render: (v?: string) => v || '-',
      },
      {
        title: '销方税号',
        dataIndex: 'seller_tax_id',
        key: 'seller_tax_id',
        width: 170,
        ellipsis: true,
        render: (v?: string) => v || '-',
      },
      {
        title: '不含税金额',
        dataIndex: 'amount',
        key: 'amount',
        width: 120,
        align: 'right',
        render: (v: number) => <SensitiveText type="amount" value={v || 0} />,
      },
      {
        title: '税额',
        dataIndex: 'tax_amount',
        key: 'tax_amount',
        width: 110,
        align: 'right',
        render: (v: number) => <SensitiveText type="amount" value={v || 0} />,
      },
      {
        title: '价税合计',
        dataIndex: 'total_amount',
        key: 'total_amount',
        width: 120,
        align: 'right',
        render: (v: number) => <SensitiveText type="amount" value={v || 0} />,
      },
      {
        title: '费用类型',
        dataIndex: 'expense_type_name',
        key: 'expense_type_name',
        width: 110,
        render: (v?: string) => v || '-',
      },
      {
        title: '关联报销单号',
        key: 'claim_no',
        width: 170,
        ellipsis: true,
        render: (_: unknown, r: InputTaxLedgerRow) => {
          if (!r.claim_no) return '-';
          return r.refs_count > 1 ? `${r.claim_no}（共${r.refs_count}张）` : r.claim_no;
        },
      },
    ],
    [],
  );

  const subtotalColumns: ColumnsType<InputTaxLedgerMonthlySubtotal> = useMemo(
    () => [
      { title: '月份', dataIndex: 'belong_month', key: 'belong_month', width: 110 },
      {
        title: '张数',
        dataIndex: 'invoice_count',
        key: 'invoice_count',
        width: 80,
        align: 'right',
      },
      {
        title: '不含税金额',
        dataIndex: 'amount',
        key: 'amount',
        width: 130,
        align: 'right',
        render: (v: number) => <SensitiveText type="amount" value={v || 0} />,
      },
      {
        title: '税额',
        dataIndex: 'tax_amount',
        key: 'tax_amount',
        width: 120,
        align: 'right',
        render: (v: number) => <SensitiveText type="amount" value={v || 0} />,
      },
      {
        title: '价税合计',
        dataIndex: 'total_amount',
        key: 'total_amount',
        width: 130,
        align: 'right',
        render: (v: number) => <SensitiveText type="amount" value={v || 0} />,
      },
    ],
    [],
  );

  return (
    <div>
      <div className="page-header">
        <span className="page-title">进项台账</span>
        <div className="page-header-actions">
          <Space.Compact>
            <DatePicker
              picker="month"
              allowClear={false}
              style={monthPickerStyle}
              value={fromMonth}
              onChange={(d) => d && setFromMonth(d)}
            />
            <DatePicker
              picker="month"
              allowClear={false}
              style={monthPickerStyle}
              value={toMonth}
              onChange={(d) => d && setToMonth(d)}
            />
          </Space.Compact>
          <Button icon={<ReloadOutlined />} loading={loading} onClick={() => void fetchData()}>
            刷新
          </Button>
          <Tooltip
            title={isSensitiveRevealed ? '导出进项台账 Excel（明细 + 月度小计 + 合计）' : '敏感导出需先在页面中解锁敏感数据'}
          >
            <Button
              type="primary"
              icon={<DownloadOutlined />}
              disabled={!isSensitiveRevealed}
              loading={exporting}
              onClick={() => void handleExport()}
            >
              导出
            </Button>
          </Tooltip>
        </div>
      </div>

      <Spin spinning={loading}>
        <Alert
          type="info"
          showIcon
          message="发票登记 ≠ 进项认证，认证状态以税务系统为准"
          description={`统计区间 ${fromMonth.format('YYYY-MM')} 至 ${toMonth.format('YYYY-MM')}（按发票归属月份，已排除作废发票）`}
          className="mb-16"
        />
        <Row gutter={[16, 16]} className="mb-16">
          <Col xs={24} sm={12} md={6}>
            <Card className="stat-card">
              <Statistic title="发票张数" value={report?.invoice_count ?? 0} />
            </Card>
          </Col>
          <Col xs={24} sm={12} md={6}>
            <Card className="stat-card">
              <SensitiveStatistic title="不含税金额合计" value={report?.sum_amount ?? 0} precision={2} />
            </Card>
          </Col>
          <Col xs={24} sm={12} md={6}>
            <Card className="stat-card">
              <SensitiveStatistic title="税额合计" value={report?.sum_tax_amount ?? 0} precision={2} />
            </Card>
          </Col>
          <Col xs={24} sm={12} md={6}>
            <Card className="stat-card">
              <SensitiveStatistic title="价税合计" value={report?.sum_total_amount ?? 0} precision={2} />
            </Card>
          </Col>
        </Row>

        <Table<InputTaxLedgerRow>
          rowKey="invoice_id"
          columns={columns}
          dataSource={report?.rows ?? []}
          loading={loading}
          pagination={{ pageSize: 50, hideOnSinglePage: true, showSizeChanger: false }}
          scroll={{ x: 1380 }}
          summary={() => (
            <Table.Summary fixed>
              <Table.Summary.Row>
                <Table.Summary.Cell index={0} colSpan={6}>
                  <strong>区间合计（共 {report?.invoice_count ?? 0} 张）</strong>
                </Table.Summary.Cell>
                <Table.Summary.Cell index={6} align="right">
                  <strong>
                    <SensitiveText type="amount" value={report?.sum_amount ?? 0} />
                  </strong>
                </Table.Summary.Cell>
                <Table.Summary.Cell index={7} align="right">
                  <strong>
                    <SensitiveText type="amount" value={report?.sum_tax_amount ?? 0} />
                  </strong>
                </Table.Summary.Cell>
                <Table.Summary.Cell index={8} align="right">
                  <strong>
                    <SensitiveText type="amount" value={report?.sum_total_amount ?? 0} />
                  </strong>
                </Table.Summary.Cell>
                <Table.Summary.Cell index={9} colSpan={2} />
              </Table.Summary.Row>
            </Table.Summary>
          )}
        />

        <Card title="月度小计" className="mt-16" styles={{ body: { paddingTop: 8 } }}>
          <Table<InputTaxLedgerMonthlySubtotal>
            rowKey="belong_month"
            columns={subtotalColumns}
            dataSource={report?.monthly_subtotals ?? []}
            pagination={false}
            size="small"
            footer={() => (
              <span>
                合计 {report?.invoice_count ?? 0} 张 · 不含税 {fmtMoney(report?.sum_amount ?? 0)} · 税额{' '}
                {fmtMoney(report?.sum_tax_amount ?? 0)} · 价税合计 {fmtMoney(report?.sum_total_amount ?? 0)}
              </span>
            )}
          />
        </Card>
      </Spin>
    </div>
  );
};

export default InputTaxLedgerPage;
