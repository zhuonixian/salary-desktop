import { useState, useEffect, useCallback } from 'react';
import {
  Table, Button, DatePicker, Modal, Form, InputNumber, Space, message, Tag,
} from 'antd';
import { ImportOutlined, EditOutlined } from '@ant-design/icons';
import { open } from '@tauri-apps/plugin-dialog';
import dayjs from 'dayjs';
import type { Dayjs } from 'dayjs';
import { getAttendanceRecords, updateAttendanceRecord, importAttendanceExcel } from '@/api';
import type { AttendanceRecord, AttendanceRecordInput } from '@/types';

const Attendance: React.FC = () => {
  const [month, setMonth] = useState<Dayjs>(dayjs());
  const [records, setRecords] = useState<AttendanceRecord[]>([]);
  const [loading, setLoading] = useState(false);
  const [modalOpen, setModalOpen] = useState(false);
  const [editingRecord, setEditingRecord] = useState<AttendanceRecord | null>(null);
  const [form] = Form.useForm<AttendanceRecordInput>();

  const fetchData = useCallback(async (m: string) => {
    setLoading(true);
    try {
      const data = await getAttendanceRecords(m);
      setRecords(data);
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      message.error('获取考勤数据失败: ' + msg);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchData(month.format('YYYY-MM'));
  }, [month, fetchData]);

  const handleEdit = (record: AttendanceRecord) => {
    setEditingRecord(record);
    form.setFieldsValue({
      month: record.month,
      employee_id: record.employee_id,
      required_days: record.required_days,
      actual_days: record.actual_days,
      late_count: record.late_count,
      early_leave_count: record.early_leave_count,
      leave_days: record.leave_days,
      sick_leave_days: record.sick_leave_days,
      personal_leave_days: record.personal_leave_days,
      absent_days: record.absent_days,
      overtime_hours: record.overtime_hours,
    });
    setModalOpen(true);
  };

  const handleSubmit = async () => {
    if (!editingRecord) return;
    try {
      const values = await form.validateFields();
      await updateAttendanceRecord(editingRecord.id, values);
      message.success('更新成功');
      setModalOpen(false);
      fetchData(month.format('YYYY-MM'));
    } catch (e: unknown) {
      if (e instanceof Error) {
        message.error('更新失败: ' + e.message);
      }
    }
  };

  const handleImport = async () => {
    try {
      const path = await open({
        filters: [{ name: 'Excel', extensions: ['xlsx', 'xls'] }],
        multiple: false,
      });
      if (!path) return;
      const result = await importAttendanceExcel(path as string, month.format('YYYY-MM'));
      if (result.success) {
        message.success(`导入成功：共 ${result.total} 条，成功 ${result.imported} 条`);
      } else {
        message.error('导入失败: ' + result.errors.join('; '));
      }
      fetchData(month.format('YYYY-MM'));
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      message.error('导入失败: ' + msg);
    }
  };

  const columns = [
    { title: '工号', dataIndex: 'employee_no', key: 'employee_no', width: 100 },
    { title: '姓名', dataIndex: 'employee_name', key: 'employee_name', width: 90 },
    { title: '应出勤(天)', dataIndex: 'required_days', key: 'required_days', width: 110, align: 'center' as const },
    {
      title: '实际出勤(天)',
      dataIndex: 'actual_days',
      key: 'actual_days',
      width: 120,
      align: 'center' as const,
      render: (v: number, record: AttendanceRecord) =>
        v < record.required_days ? (
          <Tag color="red">{v}</Tag>
        ) : (
          v
        ),
    },
    { title: '迟到(次)', dataIndex: 'late_count', key: 'late_count', width: 90, align: 'center' as const },
    { title: '早退(次)', dataIndex: 'early_leave_count', key: 'early_leave_count', width: 90, align: 'center' as const },
    { title: '请假(天)', dataIndex: 'leave_days', key: 'leave_days', width: 90, align: 'center' as const },
    { title: '病假(天)', dataIndex: 'sick_leave_days', key: 'sick_leave_days', width: 90, align: 'center' as const },
    { title: '事假(天)', dataIndex: 'personal_leave_days', key: 'personal_leave_days', width: 90, align: 'center' as const },
    {
      title: '旷工(天)',
      dataIndex: 'absent_days',
      key: 'absent_days',
      width: 90,
      align: 'center' as const,
      render: (v: number) => (v > 0 ? <Tag color="red">{v}</Tag> : v),
    },
    { title: '加班(小时)', dataIndex: 'overtime_hours', key: 'overtime_hours', width: 100, align: 'center' as const },
    {
      title: '操作',
      key: 'action',
      width: 80,
      render: (_: unknown, record: AttendanceRecord) => (
        <Button type="link" icon={<EditOutlined />} onClick={() => handleEdit(record)}>
          编辑
        </Button>
      ),
    },
  ];

  return (
    <div>
      <div className="page-header">
        <span className="page-title">考勤管理</span>
        <div className="page-header-actions">
          <DatePicker
            picker="month"
            value={month}
            onChange={(d) => d && setMonth(d)}
            allowClear={false}
            style={{ width: 180 }}
          />
          <Button icon={<ImportOutlined />} onClick={handleImport}>
            导入Excel
          </Button>
        </div>
      </div>

      <Table
        rowKey="id"
        columns={columns}
        dataSource={records}
        loading={loading}
        pagination={{ pageSize: 20, showSizeChanger: true, showTotal: (t) => `共 ${t} 条` }}
        scroll={{ x: 1200 }}
        size="middle"
        rowClassName={(record) => (record.actual_days < record.required_days ? 'row-abnormal' : '')}
      />

      <Modal
        title="编辑考勤记录"
        open={modalOpen}
        onOk={handleSubmit}
        onCancel={() => setModalOpen(false)}
        width={600}
        destroyOnClose
        okText="保存"
        cancelText="取消"
      >
        <Form form={form} layout="vertical">
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '0 16px' }}>
            <Form.Item name="required_days" label="应出勤天数" rules={[{ required: true }]}>
              <InputNumber min={0} max={31} precision={0} style={{ width: '100%' }} />
            </Form.Item>
            <Form.Item name="actual_days" label="实际出勤天数" rules={[{ required: true }]}>
              <InputNumber min={0} max={31} precision={0} style={{ width: '100%' }} />
            </Form.Item>
            <Form.Item name="late_count" label="迟到次数">
              <InputNumber min={0} precision={0} style={{ width: '100%' }} />
            </Form.Item>
            <Form.Item name="early_leave_count" label="早退次数">
              <InputNumber min={0} precision={0} style={{ width: '100%' }} />
            </Form.Item>
            <Form.Item name="leave_days" label="请假天数">
              <InputNumber min={0} precision={1} style={{ width: '100%' }} />
            </Form.Item>
            <Form.Item name="sick_leave_days" label="病假天数">
              <InputNumber min={0} precision={1} style={{ width: '100%' }} />
            </Form.Item>
            <Form.Item name="personal_leave_days" label="事假天数">
              <InputNumber min={0} precision={1} style={{ width: '100%' }} />
            </Form.Item>
            <Form.Item name="absent_days" label="旷工天数">
              <InputNumber min={0} precision={1} style={{ width: '100%' }} />
            </Form.Item>
            <Form.Item name="overtime_hours" label="加班小时">
              <InputNumber min={0} precision={1} style={{ width: '100%' }} />
            </Form.Item>
          </div>
        </Form>
      </Modal>
    </div>
  );
};

export default Attendance;
