import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Alert,
  Button,
  Card,
  DatePicker,
  Form,
  Input,
  InputNumber,
  Modal,
  Popconfirm,
  Select,
  Space,
  Switch,
  Table,
  Tabs,
  Tag,
  Tooltip,
  Typography,
  message,
} from 'antd';
import type { ColumnsType } from 'antd/es/table';
import { PlusOutlined, ReloadOutlined } from '@ant-design/icons';
import dayjs from 'dayjs';
import type { Dayjs } from 'dayjs';
import SensitiveText from '@/components/SensitiveText';
import { useOperator } from '@/contexts/OperatorContext';
import {
  getBusinessPartners,
  getFundAccounts,
  getGlAccounts,
  saveBusinessPartner,
  saveFundAccount,
  saveOperatorProfile,
  setBusinessPartnerActive,
  setFundAccountActive,
  setOperatorProfileActive,
} from '@/api';
import {
  FUND_ACCOUNT_TYPE_LABEL,
  OPERATOR_ROLE_LABEL,
  PARTNER_STATUS_LABEL,
  PARTNER_TYPE_LABEL,
} from '@/types';
import type {
  BusinessPartner,
  BusinessPartnerInput,
  FundAccount,
  FundAccountInput,
  OperatorProfile,
  OperatorProfileInput,
} from '@/types';

const { Title, Text } = Typography;

const errText = (e: unknown): string => (e instanceof Error ? e.message : String(e));

const fmtAmount = (value: number): string =>
  (Number(value) || 0).toLocaleString('zh-CN', { minimumFractionDigits: 2, maximumFractionDigits: 2 });

// 资金科目白名单与后端 db::STAGE7_FUND_GL_CODES 一致（1001/1002/1012）。
const FUND_GL_OPTIONS = [
  { value: '1001', label: '1001 库存现金' },
  { value: '1002', label: '1002 银行存款' },
  { value: '1012', label: '1012 其他货币资金' },
];

const FUND_TYPE_TO_GL: Record<string, string> = { bank: '1002', cash: '1001', third_party: '1012' };

const FUND_TYPE_OPTIONS = Object.entries(FUND_ACCOUNT_TYPE_LABEL).map(([value, label]) => ({
  value,
  label,
}));

const PARTNER_TYPE_OPTIONS = Object.entries(PARTNER_TYPE_LABEL).map(([value, label]) => ({
  value,
  label,
}));

const PARTNER_STATUS_OPTIONS = Object.entries(PARTNER_STATUS_LABEL).map(([value, label]) => ({
  value,
  label,
}));

const OPERATOR_ROLE_OPTIONS = Object.entries(OPERATOR_ROLE_LABEL).map(([value, label]) => ({
  value,
  label,
}));

// 提示文案：操作人是本地署名，不是多用户权限（brief 明确要求）。
const OPERATOR_NOTICE =
  '资金账户、往来单位与操作人为本地基础资料；操作人仅用于给操作日志署名（本地署名），不是多用户权限体系。';

// ==================== 资金账户 ====================

interface FundAccountFormValues {
  account_code: string;
  name: string;
  account_type: string;
  gl_account_code: string;
  bank_name?: string;
  account_no?: string;
  opening_date?: Dayjs | null;
  opening_balance?: number;
  is_default: boolean;
  is_active: boolean;
  remark?: string;
}

const FundAccountTab: React.FC = () => {
  const [accounts, setAccounts] = useState<FundAccount[]>([]);
  const [loading, setLoading] = useState(false);
  const [accountType, setAccountType] = useState<string | undefined>(undefined);
  const [keyword, setKeyword] = useState('');

  const [modalOpen, setModalOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const [editing, setEditing] = useState<FundAccount | null>(null);
  const [form] = Form.useForm<FundAccountFormValues>();

  const fetchAccounts = useCallback(async () => {
    setLoading(true);
    try {
      setAccounts(
        await getFundAccounts({
          account_type: accountType,
          keyword: keyword.trim() || undefined,
        }),
      );
    } catch (e: unknown) {
      message.error('获取资金账户失败: ' + errText(e));
    } finally {
      setLoading(false);
    }
  }, [accountType, keyword]);

  useEffect(() => {
    fetchAccounts();
  }, [fetchAccounts]);

  const openCreate = () => {
    setEditing(null);
    form.resetFields();
    form.setFieldsValue({
      account_type: 'bank',
      gl_account_code: '1002',
      is_default: false,
      is_active: true,
    });
    setModalOpen(true);
  };

  const openEdit = (record: FundAccount) => {
    setEditing(record);
    form.resetFields();
    form.setFieldsValue({
      account_code: record.account_code,
      name: record.name,
      account_type: record.account_type,
      gl_account_code: record.gl_account_code,
      bank_name: record.bank_name ?? undefined,
      account_no: record.account_no ?? undefined,
      opening_date: record.opening_date ? dayjs(record.opening_date) : undefined,
      opening_balance: record.opening_balance,
      is_default: record.is_default,
      is_active: record.is_active,
      remark: record.remark ?? undefined,
    });
    setModalOpen(true);
  };

  const handleSave = async () => {
    const values = await form.validateFields();
    setSaving(true);
    try {
      const data: FundAccountInput = {
        id: editing?.id,
        account_code: values.account_code.trim(),
        name: values.name.trim(),
        account_type: values.account_type,
        gl_account_code: values.gl_account_code,
        // 空串=清空（后端 patch 语义），表单所见即所存。
        bank_name: values.bank_name?.trim() ?? '',
        account_no: values.account_no?.trim() ?? '',
        opening_date: values.opening_date ? values.opening_date.format('YYYY-MM-DD') : '',
        opening_balance: values.opening_balance ?? 0,
        is_default: values.is_default,
        is_active: values.is_active,
        remark: values.remark?.trim() ?? '',
      };
      await saveFundAccount(data);
      message.success('资金账户已保存');
      setModalOpen(false);
      fetchAccounts();
    } catch (e: unknown) {
      message.error('保存失败: ' + errText(e));
    } finally {
      setSaving(false);
    }
  };

  const handleSetActive = async (record: FundAccount, active: boolean) => {
    try {
      await setFundAccountActive(record.id, active);
      message.success(active ? '已启用' : '已停用');
      fetchAccounts();
    } catch (e: unknown) {
      message.error('操作失败: ' + errText(e));
    }
  };

  // 快捷设为默认：利用后端 patch 语义，仅提交标识字段与 is_default，其余字段保留原值。
  const handleSetDefault = async (record: FundAccount) => {
    try {
      await saveFundAccount({
        id: record.id,
        account_code: record.account_code,
        name: record.name,
        account_type: record.account_type,
        gl_account_code: record.gl_account_code,
        is_default: true,
      });
      message.success(`已将 ${record.name} 设为${FUND_ACCOUNT_TYPE_LABEL[record.account_type] ?? ''}类默认账户`);
      fetchAccounts();
    } catch (e: unknown) {
      message.error('操作失败: ' + errText(e));
    }
  };

  const columns: ColumnsType<FundAccount> = [
    { title: '编码', dataIndex: 'account_code', key: 'account_code', width: 110 },
    {
      title: '名称',
      dataIndex: 'name',
      key: 'name',
      width: 180,
      render: (value: string, record) => (
        <Space size={4}>
          <span>{value}</span>
          {record.is_default && <Tag color="gold">默认</Tag>}
        </Space>
      ),
    },
    {
      title: '类型',
      dataIndex: 'account_type',
      key: 'account_type',
      width: 110,
      render: (value: string) => <Tag color="blue">{FUND_ACCOUNT_TYPE_LABEL[value] ?? value}</Tag>,
    },
    { title: '银行/机构', dataIndex: 'bank_name', key: 'bank_name', width: 130, render: (v?: string | null) => v ?? '-' },
    {
      title: '账号',
      dataIndex: 'account_no',
      key: 'account_no',
      width: 190,
      render: (value?: string | null) =>
        value ? <SensitiveText type="bank_card" value={value} /> : '-',
    },
    { title: '挂接科目', dataIndex: 'gl_account_code', key: 'gl_account_code', width: 90 },
    {
      title: '期初余额',
      dataIndex: 'opening_balance',
      key: 'opening_balance',
      width: 130,
      align: 'right',
      render: (value: number) => <SensitiveText type="amount" value={fmtAmount(value)} />,
    },
    { title: '启用日期', dataIndex: 'opening_date', key: 'opening_date', width: 110, render: (v?: string | null) => v ?? '-' },
    {
      title: '状态',
      dataIndex: 'is_active',
      key: 'is_active',
      width: 80,
      render: (value: boolean) =>
        value ? <Tag color="green">启用</Tag> : <Tag color="default">停用</Tag>,
    },
    { title: '备注', dataIndex: 'remark', key: 'remark', ellipsis: true, render: (v?: string | null) => v ?? '-' },
    {
      title: '操作',
      key: 'action',
      width: 200,
      fixed: 'right',
      render: (_, record) => (
        <Space size={0}>
          <Button type="link" size="small" onClick={() => openEdit(record)}>
            编辑
          </Button>
          {!record.is_default && (
            <Popconfirm
              title={`确认将「${record.name}」设为该类型默认账户？原默认账户将自动取消。`}
              onConfirm={() => handleSetDefault(record)}
            >
              <Button type="link" size="small">
                设为默认
              </Button>
            </Popconfirm>
          )}
          {record.is_active ? (
            <Popconfirm title="确认停用该资金账户？" onConfirm={() => handleSetActive(record, false)}>
              <Button type="link" size="small" danger>
                停用
              </Button>
            </Popconfirm>
          ) : (
            <Button type="link" size="small" onClick={() => handleSetActive(record, true)}>
              启用
            </Button>
          )}
        </Space>
      ),
    },
  ];

  return (
    <>
      <Space wrap style={{ marginBottom: 16 }}>
        <Select
          style={{ width: 150 }}
          allowClear
          placeholder="账户类型"
          value={accountType}
          onChange={setAccountType}
          options={FUND_TYPE_OPTIONS}
        />
        <Input.Search
          style={{ width: 260 }}
          allowClear
          placeholder="搜索编码/名称/银行/账号"
          value={keyword}
          onChange={(e) => setKeyword(e.target.value)}
          onSearch={fetchAccounts}
        />
        <Button type="primary" icon={<PlusOutlined />} onClick={openCreate}>
          新增账户
        </Button>
        <Button icon={<ReloadOutlined />} onClick={fetchAccounts}>
          刷新
        </Button>
      </Space>
      <Table
        rowKey="id"
        columns={columns}
        dataSource={accounts}
        loading={loading}
        size="middle"
        scroll={{ x: 1280 }}
        pagination={{ pageSize: 20, showTotal: (t) => `共 ${t} 条` }}
      />

      <Modal
        title={editing ? '编辑资金账户' : '新增资金账户'}
        open={modalOpen}
        onOk={handleSave}
        confirmLoading={saving}
        onCancel={() => setModalOpen(false)}
        destroyOnHidden
      >
        <Form form={form} layout="vertical">
          <Space size="middle" style={{ display: 'flex' }}>
            <Form.Item
              name="account_code"
              label="账户编码"
              rules={[{ required: true, message: '请输入账户编码' }]}
            >
              <Input placeholder="如 BANK-001" style={{ width: 180 }} maxLength={32} />
            </Form.Item>
            <Form.Item
              name="name"
              label="账户名称"
              rules={[{ required: true, message: '请输入账户名称' }]}
            >
              <Input placeholder="如 基本存款账户" style={{ width: 200 }} maxLength={64} />
            </Form.Item>
          </Space>
          <Space size="middle" style={{ display: 'flex' }}>
            <Form.Item
              name="account_type"
              label="账户类型"
              rules={[{ required: true, message: '请选择账户类型' }]}
            >
              <Select
                style={{ width: 150 }}
                options={FUND_TYPE_OPTIONS}
                onChange={(value) => {
                  const gl = FUND_TYPE_TO_GL[value];
                  if (gl) form.setFieldsValue({ gl_account_code: gl });
                }}
              />
            </Form.Item>
            <Form.Item
              name="gl_account_code"
              label="挂接总账科目"
              rules={[{ required: true, message: '请选择挂接科目' }]}
              extra="只能挂接资金科目；账户被凭证/流水引用后不可再修改类型"
            >
              <Select style={{ width: 180 }} options={FUND_GL_OPTIONS} />
            </Form.Item>
          </Space>
          <Space size="middle" style={{ display: 'flex' }}>
            <Form.Item name="bank_name" label="银行/机构">
              <Input placeholder="现金账户可留空" style={{ width: 180 }} maxLength={64} />
            </Form.Item>
            <Form.Item name="account_no" label="账号">
              <Input placeholder="选填，全局唯一" style={{ width: 200 }} maxLength={64} />
            </Form.Item>
          </Space>
          <Space size="middle" style={{ display: 'flex' }}>
            <Form.Item name="opening_date" label="启用日期">
              <DatePicker style={{ width: 160 }} placeholder="YYYY-MM-DD" />
            </Form.Item>
            <Form.Item name="opening_balance" label="期初余额" initialValue={0}>
              <InputNumber min={0} step={0.01} style={{ width: 160 }} />
            </Form.Item>
          </Space>
          <Space size="middle" style={{ display: 'flex' }}>
            <Form.Item name="is_default" label="设为该类型默认账户" valuePropName="checked">
              <Switch />
            </Form.Item>
            <Form.Item name="is_active" label="启用" valuePropName="checked">
              <Switch />
            </Form.Item>
          </Space>
          <Form.Item name="remark" label="备注">
            <Input.TextArea rows={2} placeholder="备注（可选）" maxLength={200} />
          </Form.Item>
        </Form>
      </Modal>
    </>
  );
};

// ==================== 往来单位 ====================

interface PartnerFormValues {
  partner_code: string;
  name: string;
  partner_type: string;
  status: string;
  tax_id?: string;
  contact_person?: string;
  phone?: string;
  bank_name?: string;
  bank_account?: string;
  gl_account_code?: string;
  remark?: string;
}

const PartnerTab: React.FC = () => {
  const [partners, setPartners] = useState<BusinessPartner[]>([]);
  const [loading, setLoading] = useState(false);
  const [partnerType, setPartnerType] = useState<string | undefined>(undefined);
  const [status, setStatus] = useState<string | undefined>(undefined);
  const [keyword, setKeyword] = useState('');
  const [glOptions, setGlOptions] = useState<{ value: string; label: string }[]>([]);

  const [modalOpen, setModalOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const [editing, setEditing] = useState<BusinessPartner | null>(null);
  const [form] = Form.useForm<PartnerFormValues>();

  const fetchPartners = useCallback(async () => {
    setLoading(true);
    try {
      setPartners(
        await getBusinessPartners({
          partner_type: partnerType,
          status,
          keyword: keyword.trim() || undefined,
        }),
      );
    } catch (e: unknown) {
      message.error('获取往来单位失败: ' + errText(e));
    } finally {
      setLoading(false);
    }
  }, [partnerType, status, keyword]);

  useEffect(() => {
    fetchPartners();
  }, [fetchPartners]);

  useEffect(() => {
    // 默认科目下拉：加载科目表（可选字段，加载失败不阻断页面）。
    getGlAccounts()
      .then((list) =>
        setGlOptions(
          list
            .filter((acc) => acc.is_active)
            .map((acc) => ({ value: acc.code, label: `${acc.code} ${acc.name}` })),
        ),
      )
      .catch(() => setGlOptions([]));
  }, []);

  const openCreate = () => {
    setEditing(null);
    form.resetFields();
    form.setFieldsValue({ partner_type: 'supplier', status: 'active' });
    setModalOpen(true);
  };

  const openEdit = (record: BusinessPartner) => {
    setEditing(record);
    form.resetFields();
    form.setFieldsValue({
      partner_code: record.partner_code,
      name: record.name,
      partner_type: record.partner_type,
      status: record.status,
      tax_id: record.tax_id ?? undefined,
      contact_person: record.contact_person ?? undefined,
      phone: record.phone ?? undefined,
      bank_name: record.bank_name ?? undefined,
      bank_account: record.bank_account ?? undefined,
      gl_account_code: record.gl_account_code ?? undefined,
      remark: record.remark ?? undefined,
    });
    setModalOpen(true);
  };

  const handleSave = async () => {
    const values = await form.validateFields();
    setSaving(true);
    try {
      const data: BusinessPartnerInput = {
        id: editing?.id,
        partner_code: values.partner_code.trim(),
        name: values.name.trim(),
        partner_type: values.partner_type,
        status: values.status,
        tax_id: values.tax_id?.trim() ?? '',
        contact_person: values.contact_person?.trim() ?? '',
        phone: values.phone?.trim() ?? '',
        bank_name: values.bank_name?.trim() ?? '',
        bank_account: values.bank_account?.trim() ?? '',
        gl_account_code: values.gl_account_code ?? '',
        remark: values.remark?.trim() ?? '',
      };
      await saveBusinessPartner(data);
      message.success('往来单位已保存');
      setModalOpen(false);
      fetchPartners();
    } catch (e: unknown) {
      message.error('保存失败: ' + errText(e));
    } finally {
      setSaving(false);
    }
  };

  const handleSetActive = async (record: BusinessPartner, active: boolean) => {
    try {
      await setBusinessPartnerActive(record.id, active);
      message.success(active ? '已启用' : '已停用');
      fetchPartners();
    } catch (e: unknown) {
      message.error('操作失败: ' + errText(e));
    }
  };

  const columns: ColumnsType<BusinessPartner> = [
    { title: '编码', dataIndex: 'partner_code', key: 'partner_code', width: 110 },
    { title: '名称', dataIndex: 'name', key: 'name', width: 200 },
    {
      title: '类型',
      dataIndex: 'partner_type',
      key: 'partner_type',
      width: 90,
      render: (value: string) => <Tag color="blue">{PARTNER_TYPE_LABEL[value] ?? value}</Tag>,
    },
    { title: '税号', dataIndex: 'tax_id', key: 'tax_id', width: 160, render: (v?: string | null) => v ?? '-' },
    { title: '联系人', dataIndex: 'contact_person', key: 'contact_person', width: 100, render: (v?: string | null) => v ?? '-' },
    { title: '电话', dataIndex: 'phone', key: 'phone', width: 130, render: (v?: string | null) => v ?? '-' },
    { title: '开户行', dataIndex: 'bank_name', key: 'bank_name', width: 130, render: (v?: string | null) => v ?? '-' },
    {
      title: '银行账号',
      dataIndex: 'bank_account',
      key: 'bank_account',
      width: 190,
      render: (value?: string | null) =>
        value ? <SensitiveText type="bank_card" value={value} /> : '-',
    },
    { title: '默认科目', dataIndex: 'gl_account_code', key: 'gl_account_code', width: 90, render: (v?: string | null) => v ?? '-' },
    {
      title: '状态',
      dataIndex: 'status',
      key: 'status',
      width: 80,
      render: (value: string) =>
        value === 'active' ? <Tag color="green">启用</Tag> : <Tag color="default">停用</Tag>,
    },
    { title: '备注', dataIndex: 'remark', key: 'remark', ellipsis: true, render: (v?: string | null) => v ?? '-' },
    {
      title: '操作',
      key: 'action',
      width: 140,
      fixed: 'right',
      render: (_, record) => (
        <Space size={0}>
          <Button type="link" size="small" onClick={() => openEdit(record)}>
            编辑
          </Button>
          {record.status === 'active' ? (
            <Popconfirm title="确认停用该往来单位？" onConfirm={() => handleSetActive(record, false)}>
              <Button type="link" size="small" danger>
                停用
              </Button>
            </Popconfirm>
          ) : (
            <Button type="link" size="small" onClick={() => handleSetActive(record, true)}>
              启用
            </Button>
          )}
        </Space>
      ),
    },
  ];

  return (
    <>
      <Space wrap style={{ marginBottom: 16 }}>
        <Select
          style={{ width: 120 }}
          allowClear
          placeholder="单位类型"
          value={partnerType}
          onChange={setPartnerType}
          options={PARTNER_TYPE_OPTIONS}
        />
        <Select
          style={{ width: 110 }}
          allowClear
          placeholder="状态"
          value={status}
          onChange={setStatus}
          options={PARTNER_STATUS_OPTIONS}
        />
        <Input.Search
          style={{ width: 260 }}
          allowClear
          placeholder="搜索编码/名称/税号/联系人"
          value={keyword}
          onChange={(e) => setKeyword(e.target.value)}
          onSearch={fetchPartners}
        />
        <Button type="primary" icon={<PlusOutlined />} onClick={openCreate}>
          新增单位
        </Button>
        <Button icon={<ReloadOutlined />} onClick={fetchPartners}>
          刷新
        </Button>
      </Space>
      <Table
        rowKey="id"
        columns={columns}
        dataSource={partners}
        loading={loading}
        size="middle"
        scroll={{ x: 1280 }}
        pagination={{ pageSize: 20, showTotal: (t) => `共 ${t} 条` }}
      />

      <Modal
        title={editing ? '编辑往来单位' : '新增往来单位'}
        open={modalOpen}
        onOk={handleSave}
        confirmLoading={saving}
        onCancel={() => setModalOpen(false)}
        destroyOnHidden
      >
        <Form form={form} layout="vertical">
          <Space size="middle" style={{ display: 'flex' }}>
            <Form.Item
              name="partner_code"
              label="单位编码"
              rules={[{ required: true, message: '请输入单位编码' }]}
            >
              <Input placeholder="如 GYS-001" style={{ width: 180 }} maxLength={32} />
            </Form.Item>
            <Form.Item
              name="name"
              label="单位名称"
              rules={[{ required: true, message: '请输入单位名称' }]}
              extra="同一名称 + 税号组合唯一"
            >
              <Input placeholder="单位全称" style={{ width: 200 }} maxLength={64} />
            </Form.Item>
          </Space>
          <Space size="middle" style={{ display: 'flex' }}>
            <Form.Item
              name="partner_type"
              label="单位类型"
              rules={[{ required: true, message: '请选择单位类型' }]}
            >
              <Select style={{ width: 120 }} options={PARTNER_TYPE_OPTIONS} />
            </Form.Item>
            <Form.Item name="status" label="状态">
              <Select style={{ width: 110 }} options={PARTNER_STATUS_OPTIONS} />
            </Form.Item>
            <Form.Item name="tax_id" label="税号">
              <Input placeholder="统一社会信用代码" style={{ width: 200 }} maxLength={32} />
            </Form.Item>
          </Space>
          <Space size="middle" style={{ display: 'flex' }}>
            <Form.Item name="contact_person" label="联系人">
              <Input style={{ width: 140 }} maxLength={32} />
            </Form.Item>
            <Form.Item name="phone" label="电话">
              <Input style={{ width: 170 }} maxLength={20} />
            </Form.Item>
          </Space>
          <Space size="middle" style={{ display: 'flex' }}>
            <Form.Item name="bank_name" label="开户行">
              <Input style={{ width: 180 }} maxLength={64} />
            </Form.Item>
            <Form.Item name="bank_account" label="银行账号">
              <Input style={{ width: 200 }} maxLength={64} />
            </Form.Item>
          </Space>
          <Form.Item
            name="gl_account_code"
            label="默认往来科目"
            extra="选填；填写后收付款单可默认带出该科目"
          >
            <Select
              style={{ width: 240 }}
              allowClear
              showSearch
              optionFilterProp="label"
              placeholder="从科目表选择"
              options={glOptions}
            />
          </Form.Item>
          <Form.Item name="remark" label="备注">
            <Input.TextArea rows={2} placeholder="备注（可选）" maxLength={200} />
          </Form.Item>
        </Form>
      </Modal>
    </>
  );
};

// ==================== 操作人 ====================

interface OperatorFormValues {
  name: string;
  role: string;
  is_active: boolean;
  remark?: string;
}

const OperatorTab: React.FC = () => {
  const { operators, operator: current, reload, selectOperator } = useOperator();
  const [statusFilter, setStatusFilter] = useState<string | undefined>(undefined);
  const [keyword, setKeyword] = useState('');

  const [modalOpen, setModalOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const [editing, setEditing] = useState<OperatorProfile | null>(null);
  const [form] = Form.useForm<OperatorFormValues>();

  const filtered = useMemo(() => {
    const kw = keyword.trim();
    return operators.filter((item) => {
      if (statusFilter === 'active' && !item.is_active) return false;
      if (statusFilter === 'inactive' && item.is_active) return false;
      if (kw && !`${item.name}${item.role}`.toLowerCase().includes(kw.toLowerCase())) return false;
      return true;
    });
  }, [operators, statusFilter, keyword]);

  const openCreate = () => {
    setEditing(null);
    form.resetFields();
    form.setFieldsValue({ role: 'cashier', is_active: true });
    setModalOpen(true);
  };

  const openEdit = (record: OperatorProfile) => {
    setEditing(record);
    form.resetFields();
    form.setFieldsValue({
      name: record.name,
      role: record.role,
      is_active: record.is_active,
      remark: record.remark ?? undefined,
    });
    setModalOpen(true);
  };

  const handleSave = async () => {
    const values = await form.validateFields();
    setSaving(true);
    try {
      const data: OperatorProfileInput = {
        id: editing?.id,
        name: values.name.trim(),
        role: values.role,
        is_active: values.is_active,
        remark: values.remark?.trim() ?? '',
      };
      await saveOperatorProfile(data);
      message.success('操作人已保存');
      setModalOpen(false);
      // 停用当前操作人时后端会清空会话，统一刷新当前操作人与列表。
      await reload();
    } catch (e: unknown) {
      message.error('保存失败: ' + errText(e));
    } finally {
      setSaving(false);
    }
  };

  const handleSetActive = async (record: OperatorProfile, active: boolean) => {
    try {
      await setOperatorProfileActive(record.id, active);
      message.success(active ? '已启用' : '已停用');
      await reload();
    } catch (e: unknown) {
      message.error('操作失败: ' + errText(e));
    }
  };

  const handleMakeCurrent = async (record: OperatorProfile) => {
    try {
      await selectOperator(record.id);
      message.success(`当前操作人已切换为 ${record.name}`);
    } catch (e: unknown) {
      message.error('切换失败: ' + errText(e));
    }
  };

  const columns: ColumnsType<OperatorProfile> = [
    { title: '姓名', dataIndex: 'name', key: 'name', width: 160 },
    {
      title: '岗位角色',
      dataIndex: 'role',
      key: 'role',
      width: 110,
      render: (value: string) => <Tag color="blue">{OPERATOR_ROLE_LABEL[value] ?? value}</Tag>,
    },
    {
      title: '状态',
      dataIndex: 'is_active',
      key: 'is_active',
      width: 80,
      render: (value: boolean) =>
        value ? <Tag color="green">启用</Tag> : <Tag color="default">停用</Tag>,
    },
    {
      title: '当前操作人',
      key: 'current',
      width: 110,
      render: (_, record) =>
        current?.id === record.id ? <Tag color="processing">当前</Tag> : <Text type="secondary">-</Text>,
    },
    { title: '备注', dataIndex: 'remark', key: 'remark', ellipsis: true, render: (v?: string | null) => v ?? '-' },
    { title: '更新时间', dataIndex: 'updated_at', key: 'updated_at', width: 170, render: (v?: string | null) => (v ? dayjs(v).format('YYYY-MM-DD HH:mm') : '-') },
    {
      title: '操作',
      key: 'action',
      width: 210,
      fixed: 'right',
      render: (_, record) => (
        <Space size={0}>
          <Button type="link" size="small" onClick={() => openEdit(record)}>
            编辑
          </Button>
          <Tooltip title={record.is_active ? undefined : '停用的操作人不能设为当前操作人'}>
            <Button
              type="link"
              size="small"
              disabled={!record.is_active || current?.id === record.id}
              onClick={() => handleMakeCurrent(record)}
            >
              设为当前
            </Button>
          </Tooltip>
          {record.is_active ? (
            <Popconfirm title="确认停用该操作人？停用当前操作人后需重新选择。" onConfirm={() => handleSetActive(record, false)}>
              <Button type="link" size="small" danger>
                停用
              </Button>
            </Popconfirm>
          ) : (
            <Button type="link" size="small" onClick={() => handleSetActive(record, true)}>
              启用
            </Button>
          )}
        </Space>
      ),
    },
  ];

  return (
    <>
      <Alert
        type="warning"
        showIcon
        style={{ marginBottom: 16 }}
        message="操作人仅用于本地操作留痕（本地署名），不是多用户权限体系；至少保留一名启用操作人。"
      />
      <Space wrap style={{ marginBottom: 16 }}>
        <Select
          style={{ width: 110 }}
          allowClear
          placeholder="状态"
          value={statusFilter}
          onChange={setStatusFilter}
          options={[
            { value: 'active', label: '启用' },
            { value: 'inactive', label: '停用' },
          ]}
        />
        <Input.Search
          style={{ width: 220 }}
          allowClear
          placeholder="搜索姓名"
          value={keyword}
          onChange={(e) => setKeyword(e.target.value)}
        />
        <Button type="primary" icon={<PlusOutlined />} onClick={openCreate}>
          新增操作人
        </Button>
      </Space>
      <Table
        rowKey="id"
        columns={columns}
        dataSource={filtered}
        size="middle"
        pagination={{ pageSize: 20, showTotal: (t) => `共 ${t} 条` }}
      />

      <Modal
        title={editing ? '编辑操作人' : '新增操作人'}
        open={modalOpen}
        onOk={handleSave}
        confirmLoading={saving}
        onCancel={() => setModalOpen(false)}
        destroyOnHidden
      >
        <Form form={form} layout="vertical">
          <Space size="middle" style={{ display: 'flex' }}>
            <Form.Item
              name="name"
              label="姓名"
              rules={[{ required: true, message: '请输入姓名' }]}
            >
              <Input placeholder="操作人姓名" style={{ width: 180 }} maxLength={32} />
            </Form.Item>
            <Form.Item
              name="role"
              label="岗位角色"
              rules={[{ required: true, message: '请选择岗位角色' }]}
              extra="角色仅作留痕标注，不影响任何功能权限"
            >
              <Select style={{ width: 140 }} options={OPERATOR_ROLE_OPTIONS} />
            </Form.Item>
          </Space>
          <Form.Item name="is_active" label="启用" valuePropName="checked" extra="至少保留一名启用操作人">
            <Switch />
          </Form.Item>
          <Form.Item name="remark" label="备注">
            <Input.TextArea rows={2} placeholder="备注（可选）" maxLength={200} />
          </Form.Item>
        </Form>
      </Modal>
    </>
  );
};

// ==================== 页面入口 ====================

const FundAccounts: React.FC = () => (
  <Card>
    <Title level={4} style={{ marginTop: 0 }}>
      资金账户与基础资料
    </Title>
    <Alert type="info" showIcon style={{ marginBottom: 16 }} message={OPERATOR_NOTICE} />
    <Tabs
      defaultActiveKey="fund"
      destroyOnHidden
      items={[
        { key: 'fund', label: '资金账户', children: <FundAccountTab /> },
        { key: 'partner', label: '往来单位', children: <PartnerTab /> },
        { key: 'operator', label: '操作人', children: <OperatorTab /> },
      ]}
    />
  </Card>
);

export default FundAccounts;
