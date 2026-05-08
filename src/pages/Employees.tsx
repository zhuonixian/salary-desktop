import { useState, useEffect, useCallback } from 'react';
import {
  Table, Button, Input, InputNumber, Modal, Form, Tag, Space, message, Popconfirm, Upload, Select,
} from 'antd';
import {
  PlusOutlined, SearchOutlined, ImportOutlined, EditOutlined, DeleteOutlined,
} from '@ant-design/icons';
import { open } from '@tauri-apps/plugin-dialog';
import { getEmployees, createEmployee, updateEmployee, deleteEmployee, importEmployeesExcel } from '@/api';
import type { Employee, EmployeeInput, EmployeeStatus } from '@/types';

const statusColorMap: Record<EmployeeStatus, string> = {
  '在职': 'green',
  '离职': 'red',
  '试用': 'orange',
};

const Employees: React.FC = () => {
  const [employees, setEmployees] = useState<Employee[]>([]);
  const [loading, setLoading] = useState(false);
  const [search, setSearch] = useState('');
  const [modalOpen, setModalOpen] = useState(false);
  const [editingEmployee, setEditingEmployee] = useState<Employee | null>(null);
  const [form] = Form.useForm<EmployeeInput>();

  const fetchData = useCallback(async () => {
    setLoading(true);
    try {
      const data = await getEmployees();
      setEmployees(data);
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      message.error('获取员工列表失败: ' + msg);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchData();
  }, [fetchData]);

  const handleAdd = () => {
    setEditingEmployee(null);
    form.resetFields();
    form.setFieldsValue({
      status: '在职',
      base_salary: 0,
      position_salary: 0,
      performance_salary: 0,
      social_insurance_base: 0,
      housing_fund_base: 0,
      special_deduction: 0,
    });
    setModalOpen(true);
  };

  const handleEdit = (record: Employee) => {
    setEditingEmployee(record);
    form.setFieldsValue(record);
    setModalOpen(true);
  };

  const handleDelete = async (id: number) => {
    try {
      await deleteEmployee(id);
      message.success('删除成功');
      fetchData();
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      message.error('删除失败: ' + msg);
    }
  };

  const handleImport = async () => {
    try {
      const path = await open({
        filters: [{ name: 'Excel', extensions: ['xlsx', 'xls'] }],
        multiple: false,
      });
      if (!path) return;
      const result = await importEmployeesExcel(path as string);
      if (result.success) {
        message.success(`导入成功：共 ${result.total} 条，成功 ${result.imported} 条，失败 ${result.failed} 条`);
      } else {
        message.error('导入失败: ' + result.errors.join('; '));
      }
      fetchData();
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      message.error('导入失败: ' + msg);
    }
  };

  const handleSubmit = async () => {
    try {
      const values = await form.validateFields();
      if (editingEmployee) {
        await updateEmployee(editingEmployee.id, values);
        message.success('更新成功');
      } else {
        await createEmployee(values);
        message.success('新增成功');
      }
      setModalOpen(false);
      fetchData();
    } catch (e: unknown) {
      if (e instanceof Error) {
        message.error('操作失败: ' + e.message);
      }
    }
  };

  const filteredData = employees.filter(
    (e) =>
      e.employee_no.includes(search) ||
      e.name.includes(search) ||
      e.department.includes(search)
  );

  const columns = [
    { title: '工号', dataIndex: 'employee_no', key: 'employee_no', width: 100, fixed: 'left' as const },
    { title: '姓名', dataIndex: 'name', key: 'name', width: 90 },
    { title: '部门', dataIndex: 'department', key: 'department', width: 100 },
    { title: '岗位', dataIndex: 'position', key: 'position', width: 100 },
    { title: '手机号', dataIndex: 'phone', key: 'phone', width: 130 },
    {
      title: '基本工资',
      dataIndex: 'base_salary',
      key: 'base_salary',
      width: 110,
      align: 'right' as const,
      render: (v: number) => v?.toLocaleString('zh-CN', { minimumFractionDigits: 2, maximumFractionDigits: 2 }),
    },
    {
      title: '岗位工资',
      dataIndex: 'position_salary',
      key: 'position_salary',
      width: 110,
      align: 'right' as const,
      render: (v: number) => v?.toLocaleString('zh-CN', { minimumFractionDigits: 2, maximumFractionDigits: 2 }),
    },
    {
      title: '绩效工资',
      dataIndex: 'performance_salary',
      key: 'performance_salary',
      width: 110,
      align: 'right' as const,
      render: (v: number) => v?.toLocaleString('zh-CN', { minimumFractionDigits: 2, maximumFractionDigits: 2 }),
    },
    {
      title: '状态',
      dataIndex: 'status',
      key: 'status',
      width: 80,
      render: (status: EmployeeStatus) => <Tag color={statusColorMap[status]}>{status}</Tag>,
    },
    { title: '入职日期', dataIndex: 'hire_date', key: 'hire_date', width: 110 },
    {
      title: '操作',
      key: 'action',
      width: 120,
      fixed: 'right' as const,
      render: (_: unknown, record: Employee) => (
        <Space>
          <Button type="link" icon={<EditOutlined />} onClick={() => handleEdit(record)} />
          <Popconfirm title="确认删除该员工?" onConfirm={() => handleDelete(record.id)} okText="确认" cancelText="取消">
            <Button type="link" danger icon={<DeleteOutlined />} />
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <div>
      <div className="page-header">
        <span className="page-title">员工管理</span>
        <div className="page-header-actions">
          <Input
            placeholder="搜索工号/姓名/部门"
            prefix={<SearchOutlined />}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            style={{ width: 240 }}
            allowClear
          />
          <Button type="primary" icon={<PlusOutlined />} onClick={handleAdd}>
            新增员工
          </Button>
          <Button icon={<ImportOutlined />} onClick={handleImport}>
            导入Excel
          </Button>
        </div>
      </div>

      <Table
        rowKey="id"
        columns={columns}
        dataSource={filteredData}
        loading={loading}
        pagination={{ pageSize: 20, showSizeChanger: true, showTotal: (t) => `共 ${t} 条` }}
        scroll={{ x: 1400 }}
        size="middle"
      />

      <Modal
        title={editingEmployee ? '编辑员工' : '新增员工'}
        open={modalOpen}
        onOk={handleSubmit}
        onCancel={() => setModalOpen(false)}
        width={720}
        destroyOnClose
        okText="保存"
        cancelText="取消"
      >
        <Form form={form} layout="vertical" style={{ maxHeight: '60vh', overflowY: 'auto', paddingRight: 8 }}>
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '0 16px' }}>
            <Form.Item name="employee_no" label="工号" rules={[{ required: true, message: '请输入工号' }]}>
              <Input placeholder="请输入工号" disabled={!!editingEmployee} />
            </Form.Item>
            <Form.Item name="name" label="姓名" rules={[{ required: true, message: '请输入姓名' }]}>
              <Input placeholder="请输入姓名" />
            </Form.Item>
            <Form.Item name="department" label="部门" rules={[{ required: true, message: '请输入部门' }]}>
              <Input placeholder="请输入部门" />
            </Form.Item>
            <Form.Item name="position" label="岗位" rules={[{ required: true, message: '请输入岗位' }]}>
              <Input placeholder="请输入岗位" />
            </Form.Item>
            <Form.Item name="id_card" label="身份证号">
              <Input placeholder="请输入身份证号" maxLength={18} />
            </Form.Item>
            <Form.Item name="phone" label="手机号">
              <Input placeholder="请输入手机号" maxLength={11} />
            </Form.Item>
            <Form.Item name="bank_account" label="银行卡号">
              <Input placeholder="请输入银行卡号" />
            </Form.Item>
            <Form.Item name="bank_name" label="开户行">
              <Input placeholder="请输入开户行" />
            </Form.Item>
            <Form.Item name="hire_date" label="入职日期">
              <Input placeholder="YYYY-MM-DD" />
            </Form.Item>
            <Form.Item name="status" label="员工状态">
              <Select
                options={[
                  { label: '在职', value: '在职' },
                  { label: '试用', value: '试用' },
                  { label: '离职', value: '离职' },
                ]}
              />
            </Form.Item>
            <Form.Item name="base_salary" label="基本工资" rules={[{ required: true, message: '请输入基本工资' }]}>
              <InputNumber min={0} precision={2} placeholder="0.00" style={{ width: '100%' }} />
            </Form.Item>
            <Form.Item name="position_salary" label="岗位工资" rules={[{ required: true, message: '请输入岗位工资' }]}>
              <InputNumber min={0} precision={2} placeholder="0.00" style={{ width: '100%' }} />
            </Form.Item>
            <Form.Item name="performance_salary" label="绩效工资" rules={[{ required: true, message: '请输入绩效工资' }]}>
              <InputNumber min={0} precision={2} placeholder="0.00" style={{ width: '100%' }} />
            </Form.Item>
            <Form.Item name="social_insurance_base" label="社保基数">
              <InputNumber min={0} precision={2} placeholder="0.00" style={{ width: '100%' }} />
            </Form.Item>
            <Form.Item name="housing_fund_base" label="公积金基数">
              <InputNumber min={0} precision={2} placeholder="0.00" style={{ width: '100%' }} />
            </Form.Item>
            <Form.Item name="special_deduction" label="专项附加扣除">
              <InputNumber min={0} precision={2} placeholder="0.00" style={{ width: '100%' }} />
            </Form.Item>
          </div>
          <Form.Item name="remark" label="备注">
            <Input.TextArea rows={2} placeholder="备注信息" />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  );
};

export default Employees;
