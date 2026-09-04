import { useState } from 'react';
import { Row, Col, Card, DatePicker, Button, message } from 'antd';
import {
  FileExcelOutlined, BankOutlined, FileTextOutlined, CalendarOutlined,
} from '@ant-design/icons';
import { open, save } from '@tauri-apps/plugin-dialog';
import {
  exportSalaryDetail,
  exportBankPaymentFile,
  exportSalarySlips,
  exportAttendanceSummaryFile,
} from '@/api';
import { useBusinessMonth } from '@/contexts/BusinessMonthContext';

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
];

const ExportCenter: React.FC = () => {
  const { month, monthStr, setMonth } = useBusinessMonth();
  const [exporting, setExporting] = useState<string | null>(null);

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
      const msg = e instanceof Error ? e.message : String(e);
      message.error(`${item.title} 导出失败: ${msg}`);
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
    </div>
  );
};

export default ExportCenter;
