import { useState } from 'react';
import { Row, Col, Card, DatePicker, Button, message, Space, Typography } from 'antd';
import {
  FileExcelOutlined, BankOutlined, FileTextOutlined, CalendarOutlined, FileDoneOutlined,
  IdcardOutlined, PayCircleOutlined, SafetyCertificateOutlined,
} from '@ant-design/icons';
import dayjs from 'dayjs';
import type { Dayjs } from 'dayjs';
import { open, save } from '@tauri-apps/plugin-dialog';
import {
  exportSalaryDetail,
  exportBankPaymentFile,
  exportSalarySlips,
  exportAttendanceSummaryFile,
  exportTaxWithholdingDeclaration,
  exportEitPayerTemplate,
  exportEitSalaryTemplate,
  exportSocialBaseDeclaration,
} from '@/api';
import { useBusinessMonth } from '@/contexts/BusinessMonthContext';

const { Text } = Typography;

interface ExportItem {
  key: string;
  title: string;
  description: string;
  icon: React.ReactNode;
  exportFn: (month: string, savePath: string) => Promise<void>;
  fileName: string;
  target: 'file' | 'directory';
}

const exportItems: ExportItem[] = [
  {
    key: 'salary_detail',
    title: '月度工资明细表',
    description: '导出当月所有员工的工资计算明细，包含各项收入、扣款及实发工资。',
    icon: <FileExcelOutlined />,
    exportFn: exportSalaryDetail,
    fileName: '工资明细表',
    target: 'file',
  },
  {
    key: 'bank_payment',
    title: '银行代发表',
    description: '生成银行代发工资所需的格式文件，包含员工银行账号和实发金额。',
    icon: <BankOutlined />,
    exportFn: exportBankPaymentFile,
    fileName: '银行代发表',
    target: 'file',
  },
  {
    key: 'salary_slips',
    title: '员工工资条',
    description: '导出每位员工的工资条，可用于分发或邮件发送。',
    icon: <FileTextOutlined />,
    exportFn: exportSalarySlips,
    fileName: '工资条',
    target: 'directory',
  },
  {
    key: 'attendance_summary',
    title: '考勤汇总表',
    description: '导出当月考勤汇总数据，包含出勤、迟到、请假、加班等统计。',
    icon: <CalendarOutlined />,
    exportFn: exportAttendanceSummaryFile,
    fileName: '考勤汇总表',
    target: 'file',
  },
  {
    key: 'tax_withholding_declaration',
    title: '个税扣缴申报表',
    description: '按电子税务局申报录入页列序导出当月个税扣缴数据，仅已锁定月份可导出。',
    icon: <FileDoneOutlined />,
    exportFn: async (month: string, savePath: string) => {
      await exportTaxWithholdingDeclaration(month, savePath);
    },
    fileName: '个税扣缴申报表',
    target: 'file',
  },
];

const errText = (e: unknown): string => (e instanceof Error ? e.message : String(e));

const ExportCenter: React.FC = () => {
  const { month, monthStr, setMonth } = useBusinessMonth();
  const [exporting, setExporting] = useState<string | null>(null);

  // 第十阶段：税务申报模板三件的独立参数（工资模板按月、社保基数申报按年）。
  const [eitSalaryMonth, setEitSalaryMonth] = useState<Dayjs>(month);
  const [socialYear, setSocialYear] = useState<Dayjs>(dayjs());

  const handleExport = async (item: ExportItem) => {
    setExporting(item.key);
    try {
      const targetPath = item.target === 'directory'
        ? await open({ directory: true, multiple: false })
        : await save({
            filters: [{ name: 'Excel', extensions: ['xlsx'] }],
            defaultPath: `${item.fileName}_${monthStr}.xlsx`,
          });
      if (!targetPath) {
        setExporting(null);
        return;
      }
      await item.exportFn(monthStr, targetPath as string);
      message.success(`${item.title} 导出成功`);
    } catch (e: unknown) {
      message.error(`${item.title} 导出失败: ${errText(e)}`);
    } finally {
      setExporting(null);
    }
  };

  // 税务申报模板通用导出：选择保存路径 → invoke → 成功提示路径。
  const handleTemplateExport = async (
    key: string,
    title: string,
    defaultPath: string,
    run: (savePath: string) => Promise<string>,
  ) => {
    setExporting(key);
    try {
      const targetPath = await save({
        filters: [{ name: 'Excel', extensions: ['xlsx'] }],
        defaultPath,
      });
      if (!targetPath) {
        setExporting(null);
        return;
      }
      await run(targetPath as string);
      message.success(`${title} 导出成功：${targetPath}`);
    } catch (e: unknown) {
      // 门禁类错误（如仅限已锁定月份）由后端返回中文信息，直接展示。
      message.error(`${title} 导出失败: ${errText(e)}`);
    } finally {
      setExporting(null);
    }
  };

  return (
    <div>
      <div className="page-header">
        <span className="page-title">导出中心</span>
        <DatePicker
          picker="month"
          value={month}
          onChange={(d) => d && setMonth(d)}
          allowClear={false}
          style={{ width: 180 }}
        />
      </div>

      <Row gutter={[24, 24]}>
        {exportItems.map((item) => (
          <Col xs={24} sm={12} lg={6} key={item.key}>
            <Card className="export-card" style={{ textAlign: 'center', padding: '16px 0' }}>
              <div className="export-card-icon">{item.icon}</div>
              <div className="export-card-title">{item.title}</div>
              <div className="export-card-desc">{item.description}</div>
              <Button
                type="primary"
                onClick={() => handleExport(item)}
                loading={exporting === item.key}
                block
              >
                {item.target === 'directory' ? '选择目录导出' : '导出 Excel'}
              </Button>
            </Card>
          </Col>
        ))}
      </Row>

      <Row gutter={[24, 24]} style={{ marginTop: 24 }}>
        <Col xs={24} sm={12} lg={6}>
          <Card className="export-card" style={{ textAlign: 'center', padding: '16px 0' }}>
            <div className="export-card-icon"><IdcardOutlined /></div>
            <div className="export-card-title">人员信息采集模板（扣缴端）</div>
            <div className="export-card-desc">
              列序对齐扣缴端导入模板，可直接填报人员信息。税局模板可能更新，导入不符请以扣缴端当前模板为准。
            </div>
            <Button
              type="primary"
              onClick={() =>
                void handleTemplateExport(
                  'eit_payer_template',
                  '人员信息采集模板（扣缴端）',
                  `${monthStr}_人员信息采集模板.xlsx`,
                  (savePath) => exportEitPayerTemplate(savePath),
                )}
              loading={exporting === 'eit_payer_template'}
              block
            >
              导出 Excel
            </Button>
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card className="export-card" style={{ textAlign: 'center', padding: '16px 0' }}>
            <div className="export-card-icon"><PayCircleOutlined /></div>
            <div className="export-card-title">工资薪金申报模板（扣缴端）</div>
            <div className="export-card-desc">
              按所选月份导出工资薪金收入与扣缴数据，仅限已锁定月份可导出。
            </div>
            <Space direction="vertical" style={{ width: '100%' }} size={8}>
              <DatePicker
                picker="month"
                value={eitSalaryMonth}
                onChange={(d) => d && setEitSalaryMonth(d)}
                allowClear={false}
                style={{ width: '100%' }}
              />
              <Button
                type="primary"
                onClick={() =>
                  void handleTemplateExport(
                    'eit_salary_template',
                    '工资薪金申报模板（扣缴端）',
                    `${eitSalaryMonth.format('YYYY-MM')}_工资薪金申报模板.xlsx`,
                    (savePath) => exportEitSalaryTemplate(eitSalaryMonth.format('YYYY-MM'), savePath),
                  )}
                loading={exporting === 'eit_salary_template'}
                block
              >
                导出 Excel
              </Button>
            </Space>
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card className="export-card" style={{ textAlign: 'center', padding: '16px 0' }}>
            <div className="export-card-icon"><SafetyCertificateOutlined /></div>
            <div className="export-card-title">社保缴费基数年度申报表</div>
            <div className="export-card-desc">
              按所选年度导出社保缴费基数台账，供年度基数申报使用。
            </div>
            <Space direction="vertical" style={{ width: '100%' }} size={8}>
              <DatePicker
                picker="year"
                value={socialYear}
                onChange={(d) => d && setSocialYear(d)}
                allowClear={false}
                style={{ width: '100%' }}
              />
              <Button
                type="primary"
                onClick={() =>
                  void handleTemplateExport(
                    'social_base_declaration',
                    '社保缴费基数年度申报表',
                    `${socialYear.format('YYYY')}_社保缴费基数申报表.xlsx`,
                    (savePath) => exportSocialBaseDeclaration(socialYear.year(), savePath),
                  )}
                loading={exporting === 'social_base_declaration'}
                block
              >
                导出 Excel
              </Button>
            </Space>
          </Card>
        </Col>
      </Row>

      <div style={{ marginTop: 16 }}>
        <Text type="secondary" style={{ fontSize: 12 }}>
          申报模板导出后请在自然人电子税务局（扣缴端）或社保申报系统中核对列序后再导入。
        </Text>
      </div>
    </div>
  );
};

export default ExportCenter;
