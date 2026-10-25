import { useCallback, useEffect, useState } from 'react';
import { Table, Button, DatePicker, Modal, message } from 'antd';
import { DownloadOutlined } from '@ant-design/icons';
import { save } from '@tauri-apps/plugin-dialog';
import dayjs from 'dayjs';
import type { Dayjs } from 'dayjs';
import { getAnnualTaxSummary, exportAnnualTaxSummary } from '@/api';
import type { AnnualTaxSummaryRow } from '@/types';
import { SensitiveText } from '@/components/SensitiveText';

interface SalaryAnnualTaxModalProps {
  /** 是否展示年度汇总弹窗 */
  open: boolean;
  /** 请求关闭弹窗 */
  onClose: () => void;
}

// 个税年度汇总弹窗：按年度汇总员工累计收入/扣除/已预扣与年度应预扣差额，支持导出 Excel。
// 年度切换、数据加载与导出均在组件内自理，外层只需控制 open / onClose。
export function SalaryAnnualTaxModal({ open, onClose }: SalaryAnnualTaxModalProps) {
  const [annualYear, setAnnualYear] = useState<Dayjs>(dayjs());
  const [annualRows, setAnnualRows] = useState<AnnualTaxSummaryRow[]>([]);
  const [annualLoading, setAnnualLoading] = useState(false);
  const [annualExporting, setAnnualExporting] = useState(false);

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
    if (open) fetchAnnualSummary(annualYear.year());
  }, [open, annualYear, fetchAnnualSummary]);

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

  return (
    <Modal
      title={`个税年度汇总（${annualYear.year()}年度）`}
      open={open}
      onCancel={onClose}
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
  );
}

export default SalaryAnnualTaxModal;
