import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Alert,
  Button,
  Card,
  Col,
  DatePicker,
  Row,
  Spin,
  Table,
  Tabs,
  message,
} from 'antd';
import { DownloadOutlined, ReloadOutlined } from '@ant-design/icons';
import { save } from '@tauri-apps/plugin-dialog';
import dayjs from 'dayjs';
import type { Dayjs } from 'dayjs';
import {
  exportFinancialReport,
  getBalanceSheet,
  getCashFlowStatement,
  getIncomeStatement,
} from '@/api';
import { SensitiveText } from '@/components/SensitiveText';
import { SensitiveStatistic } from '@/components/SensitiveStatistic';
import type {
  BalanceSheet,
  CashFlowStatement,
  FinancialReportType,
  IncomeStatement,
  ReportRow,
} from '@/types';

const fmtMoney = (value?: number | null) =>
  (value ?? 0).toLocaleString('zh-CN', { minimumFractionDigits: 2, maximumFractionDigits: 2 });

// 报表 Tab 元数据：Tab key === 后端 report_type，导出命令直接复用。
const REPORT_META: Record<FinancialReportType, { title: string }> = {
  balance_sheet: { title: '资产负债表' },
  income_statement: { title: '利润表' },
  cash_flow_statement: { title: '现金流量表' },
};

// 利润表渲染行序调整（审查裁决）：后端把"其他未列示损益"（other_pl）放在
// 所得税费用与营业利润之间；渲染时移到 利润总额 之后、净利润 之前，
// 让 净利润 = 利润总额 + 其他未列示损益 − 所得税 的加总关系视觉上成立。
const orderIncomeRows = (rows: ReportRow[]): ReportRow[] => {
  const other = rows.find((row) => row.key === 'other_pl');
  if (!other) return rows;
  const rest = rows.filter((row) => row.key !== 'other_pl');
  const totalIndex = rest.findIndex((row) => row.key === 'total_profit');
  if (totalIndex < 0) return rows;
  return [...rest.slice(0, totalIndex + 1), other, ...rest.slice(totalIndex + 1)];
};

// 加粗的小计/合计行：利润表三个计算行
const INCOME_SUMMARY_KEYS = new Set(['operating_profit', 'total_profit', 'net_profit']);

const FinancialReports: React.FC = () => {
  const [month, setMonth] = useState<Dayjs>(dayjs());
  const [activeTab, setActiveTab] = useState<FinancialReportType>('balance_sheet');
  const [balanceSheet, setBalanceSheet] = useState<BalanceSheet | null>(null);
  const [incomeStatement, setIncomeStatement] = useState<IncomeStatement | null>(null);
  const [cashFlowStatement, setCashFlowStatement] = useState<CashFlowStatement | null>(null);
  const [loading, setLoading] = useState(false);
  const [exporting, setExporting] = useState<FinancialReportType | null>(null);

  const monthStr = month.format('YYYY-MM');

  const fetchData = useCallback(async () => {
    setLoading(true);
    try {
      const [bs, is, cf] = await Promise.all([
        getBalanceSheet(monthStr),
        getIncomeStatement(monthStr),
        getCashFlowStatement(monthStr),
      ]);
      setBalanceSheet(bs);
      setIncomeStatement(is);
      setCashFlowStatement(cf);
    } catch (e: unknown) {
      message.error('获取财务报表失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setLoading(false);
    }
  }, [monthStr]);

  useEffect(() => {
    fetchData();
  }, [fetchData]);

  const handleExport = async (reportType: FinancialReportType) => {
    const meta = REPORT_META[reportType];
    const path = await save({
      title: `导出${meta.title}`,
      defaultPath: `${meta.title}_${month.format('YYYYMM')}.xlsx`,
      filters: [{ name: 'Excel', extensions: ['xlsx'] }],
    });
    if (!path) return;
    setExporting(reportType);
    try {
      await exportFinancialReport(monthStr, reportType, String(path));
      message.success(`${meta.title}已导出`);
    } catch (e: unknown) {
      message.error(`导出${meta.title}失败: ` + (e instanceof Error ? e.message : String(e)));
    } finally {
      setExporting(null);
    }
  };

  // 渲染辅助函数（非组件）：直接调用返回 JSX，避免 react-hooks/static-components 报错。
  const renderExportButton = (reportType: FinancialReportType) => (
    <Button
      type="primary"
      ghost
      size="small"
      icon={<DownloadOutlined />}
      loading={exporting === reportType}
      onClick={() => handleExport(reportType)}
    >
      导出 Excel
    </Button>
  );

  const renderAmount = (value: number) => <SensitiveText type="amount" value={value} />;

  const reportColumns = (currentTitle: string, comparativeTitle: string, summaryKeys?: Set<string>) => [
    { title: '项目', dataIndex: 'label', key: 'label' },
    {
      title: currentTitle,
      dataIndex: 'current',
      key: 'current',
      align: 'right' as const,
      width: 180,
      render: (value: number, row: ReportRow) =>
        summaryKeys?.has(row.key) ? <strong>{renderAmount(value)}</strong> : renderAmount(value),
    },
    {
      title: comparativeTitle,
      dataIndex: 'comparative',
      key: 'comparative',
      align: 'right' as const,
      width: 180,
      render: (value: number, row: ReportRow) =>
        summaryKeys?.has(row.key) ? <strong>{renderAmount(value)}</strong> : renderAmount(value),
    },
  ];

  // 表尾合计行（Table.Summary）：资产负债表两条总计 + 现金流量表净增加额
  const renderSummaryRow = (label: string, value: number) => (
    <Table.Summary.Row>
      <Table.Summary.Cell index={0}>
        <strong>{label}</strong>
      </Table.Summary.Cell>
      <Table.Summary.Cell index={1} align="right">
        <strong>{renderAmount(value)}</strong>
      </Table.Summary.Cell>
      <Table.Summary.Cell index={2} align="right">
        <span style={{ color: '#999' }}>—</span>
      </Table.Summary.Cell>
    </Table.Summary.Row>
  );

  const incomeRows = useMemo(
    () => (incomeStatement ? orderIncomeRows(incomeStatement.rows) : []),
    [incomeStatement],
  );

  const unclassifiedTotal = useMemo(
    () =>
      (cashFlowStatement?.unclassified ?? []).reduce((sum, item) => sum + item.amount, 0),
    [cashFlowStatement],
  );

  const balanceEnabled = balanceSheet?.enabled ?? false;

  return (
    <div>
      <div className="page-header">
        <span className="page-title">财务报表</span>
        <div className="page-header-actions">
          <DatePicker
            picker="month"
            value={month}
            allowClear={false}
            onChange={(value) => value && setMonth(value)}
            style={{ width: 160 }}
          />
          <Button icon={<ReloadOutlined />} loading={loading} onClick={fetchData}>
            刷新
          </Button>
        </div>
      </div>

      <Spin spinning={loading}>
        <Tabs
          activeKey={activeTab}
          onChange={(key) => setActiveTab(key as FinancialReportType)}
          items={[
            {
              key: 'balance_sheet',
              label: '资产负债表',
              children: (
                <div>
                  {!balanceEnabled && balanceSheet && (
                    <Alert
                      type="info"
                      showIcon
                      message="该月份早于启用月（或未录期初余额），报表为空"
                      className="mb-16"
                    />
                  )}
                  {balanceSheet?.enabled && !balanceSheet.balanced && (
                    <Alert
                      type="error"
                      showIcon
                      message="资产与负债权益不平衡，请联系检查凭证"
                      description={`资产总计 ${fmtMoney(balanceSheet.asset_total)}，负债和权益总计 ${fmtMoney(balanceSheet.liability_equity_total)}，差额 ${fmtMoney(Math.abs(balanceSheet.asset_total - balanceSheet.liability_equity_total))}`}
                      className="mb-16"
                    />
                  )}
                  <div style={{ display: 'flex', justifyContent: 'flex-end', marginBottom: 12 }}>
                    {renderExportButton('balance_sheet')}
                  </div>
                  <Row gutter={[16, 16]} className="mb-16">
                    <Col xs={24} sm={12}>
                      <Card className="stat-card">
                        <SensitiveStatistic
                          title={`资产总计（${monthStr}）`}
                          value={balanceSheet?.asset_total ?? 0}
                        />
                      </Card>
                    </Col>
                    <Col xs={24} sm={12}>
                      <Card className="stat-card">
                        <SensitiveStatistic
                          title="负债和权益总计"
                          value={balanceSheet?.liability_equity_total ?? 0}
                        />
                      </Card>
                    </Col>
                  </Row>
                  <Card title="资产">
                    <Table<ReportRow>
                      rowKey="key"
                      columns={reportColumns('期末余额', '年初余额')}
                      dataSource={balanceSheet?.asset_rows ?? []}
                      pagination={false}
                      summary={() =>
                        balanceEnabled ? (
                          renderSummaryRow('资产总计', balanceSheet?.asset_total ?? 0)
                        ) : undefined
                      }
                    />
                  </Card>
                  <Card title="负债和所有者权益" style={{ marginTop: 16 }}>
                    <Table<ReportRow>
                      rowKey="key"
                      columns={reportColumns('期末余额', '年初余额')}
                      dataSource={balanceSheet?.liability_equity_rows ?? []}
                      pagination={false}
                      summary={() =>
                        balanceEnabled ? (
                          renderSummaryRow('负债和权益总计', balanceSheet?.liability_equity_total ?? 0)
                        ) : undefined
                      }
                    />
                  </Card>
                </div>
              ),
            },
            {
              key: 'income_statement',
              label: '利润表',
              children: (
                <div>
                  <div style={{ display: 'flex', justifyContent: 'flex-end', marginBottom: 12 }}>
                    {renderExportButton('income_statement')}
                  </div>
                  <Row gutter={[16, 16]} className="mb-16">
                    <Col xs={24} sm={12}>
                      <Card className="stat-card">
                        <SensitiveStatistic
                          title={`净利润（本月，${monthStr}）`}
                          value={incomeStatement?.net_profit_month ?? 0}
                        />
                      </Card>
                    </Col>
                    <Col xs={24} sm={12}>
                      <Card className="stat-card">
                        <SensitiveStatistic
                          title="净利润（本年累计）"
                          value={incomeStatement?.net_profit_year ?? 0}
                        />
                      </Card>
                    </Col>
                  </Row>
                  <Card>
                    <Table<ReportRow>
                      rowKey="key"
                      columns={reportColumns('本月金额', '本年累计', INCOME_SUMMARY_KEYS)}
                      dataSource={incomeRows}
                      pagination={false}
                    />
                  </Card>
                </div>
              ),
            },
            {
              key: 'cash_flow_statement',
              label: '现金流量表',
              children: (
                <div>
                  {cashFlowStatement && cashFlowStatement.unclassified.length > 0 && (
                    <Alert
                      type="warning"
                      showIcon
                      message="存在未归类现金流量，请到科目表补充现金流量分类"
                      description={
                        <div>
                          <p style={{ margin: '4px 0' }}>
                            以下 {cashFlowStatement.unclassified.length} 笔现金收支的对方科目未设置现金流量分类，
                            已合并计入「其他（未分类）」行，合计 {fmtMoney(unclassifiedTotal)}：
                          </p>
                          <ul style={{ margin: 0, paddingLeft: 20 }}>
                            {cashFlowStatement.unclassified.map((item, index) => (
                              <li key={`${item.voucher_no}-${index}`}>
                                凭证号 {item.voucher_no}
                                {item.summary ? `｜${item.summary}` : ''}｜
                                <SensitiveText type="amount" value={item.amount} />
                              </li>
                            ))}
                          </ul>
                        </div>
                      }
                      className="mb-16"
                    />
                  )}
                  <div style={{ display: 'flex', justifyContent: 'flex-end', marginBottom: 12 }}>
                    {renderExportButton('cash_flow_statement')}
                  </div>
                  <Row gutter={[16, 16]} className="mb-16">
                    <Col xs={24} sm={12}>
                      <Card className="stat-card">
                        <SensitiveStatistic
                          title={`现金净增加额（${monthStr}）`}
                          value={cashFlowStatement?.net_increase ?? 0}
                        />
                      </Card>
                    </Col>
                  </Row>
                  <Card>
                    <Table<ReportRow>
                      rowKey="key"
                      columns={reportColumns('本期金额', '对比金额')}
                      dataSource={cashFlowStatement?.rows ?? []}
                      pagination={false}
                      summary={() =>
                        cashFlowStatement ? (
                          renderSummaryRow('现金净增加额', cashFlowStatement.net_increase)
                        ) : undefined
                      }
                    />
                  </Card>
                </div>
              ),
            },
          ]}
        />
      </Spin>
    </div>
  );
};

export default FinancialReports;
