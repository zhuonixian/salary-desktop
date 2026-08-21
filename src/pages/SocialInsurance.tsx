import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Button,
  Card,
  Form,
  Input,
  InputNumber,
  Modal,
  Popconfirm,
  Select,
  Space,
  Switch,
  Table,
  Typography,
  message,
} from 'antd';
import type { ColumnsType } from 'antd/es/table';
import { PlusOutlined, ReloadOutlined } from '@ant-design/icons';
import {
  copySocialProfiles,
  deleteSocialProfile,
  getSocialBaseLimits,
  getSocialProfiles,
  saveSocialProfile,
  setSocialBaseLimits,
} from '@/api';
import type { SocialInsuranceProfile, SocialInsuranceProfileInput } from '@/types';

const { Title } = Typography;

const errText = (e: unknown): string => (e instanceof Error ? e.message : String(e));

const fmtAmount = (value: number): string =>
  (Number(value) || 0).toLocaleString('zh-CN', { minimumFractionDigits: 2, maximumFractionDigits: 2 });

const RATE_PLACEHOLDER = '0.24';

const currentYear = new Date().getFullYear();
// 年度下拉：当年 ~ 2028（含历史 2024 起，方便补录）
const YEAR_OPTIONS = Array.from({ length: currentYear - 2024 + 5 }, (_, i) => 2024 + i).map(
  (y) => ({ value: y, label: `${y} 年` }),
);

interface ProfileFormValues {
  employee_no: string;
  profile_year: number;
  ss_base?: number;
  hf_base?: number;
  ss_employer_rate?: number;
  ss_personal_rate?: number;
  hf_employer_rate?: number;
  hf_personal_rate?: number;
  remark?: string;
}

interface CopyFormValues {
  from_year: number;
  to_year: number;
  factor: number;
  apply_clamp: boolean;
}

interface LimitsFormValues {
  ss_min: number;
  ss_max: number;
  hf_min: number;
  hf_max: number;
}

const SocialInsurance: React.FC = () => {
  const [profiles, setProfiles] = useState<SocialInsuranceProfile[]>([]);
  const [loading, setLoading] = useState(false);
  const [year, setYear] = useState<number>(currentYear);

  const [profileOpen, setProfileOpen] = useState(false);
  const [profileSaving, setProfileSaving] = useState(false);
  const [editing, setEditing] = useState<SocialInsuranceProfile | null>(null);
  const [form] = Form.useForm<ProfileFormValues>();

  const [copyOpen, setCopyOpen] = useState(false);
  const [copySaving, setCopySaving] = useState(false);
  const [copyForm] = Form.useForm<CopyFormValues>();

  const [limitsOpen, setLimitsOpen] = useState(false);
  const [limitsSaving, setLimitsSaving] = useState(false);
  const [limitsForm] = Form.useForm<LimitsFormValues>();

  const fetchProfiles = useCallback(async (y: number) => {
    setLoading(true);
    try {
      setProfiles(await getSocialProfiles(y));
    } catch (e: unknown) {
      message.error('获取社保台账失败: ' + errText(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchProfiles(year);
  }, [fetchProfiles, year]);

  const openCreate = () => {
    setEditing(null);
    form.resetFields();
    form.setFieldsValue({ profile_year: year });
    setProfileOpen(true);
  };

  const openEdit = (record: SocialInsuranceProfile) => {
    setEditing(record);
    form.resetFields();
    form.setFieldsValue({
      employee_no: record.employee_no,
      profile_year: record.profile_year,
      ss_base: record.ss_base,
      hf_base: record.hf_base,
      ss_employer_rate: record.ss_employer_rate,
      ss_personal_rate: record.ss_personal_rate,
      hf_employer_rate: record.hf_employer_rate,
      hf_personal_rate: record.hf_personal_rate,
      remark: record.remark ?? undefined,
    });
    setProfileOpen(true);
  };

  const handleSaveProfile = async () => {
    const values = await form.validateFields();
    setProfileSaving(true);
    try {
      const data: SocialInsuranceProfileInput = {
        id: editing?.id,
        employee_no: values.employee_no.trim(),
        profile_year: values.profile_year,
        ss_base: values.ss_base ?? 0,
        hf_base: values.hf_base ?? 0,
        ss_employer_rate: values.ss_employer_rate ?? 0,
        ss_personal_rate: values.ss_personal_rate ?? 0,
        hf_employer_rate: values.hf_employer_rate ?? 0,
        hf_personal_rate: values.hf_personal_rate ?? 0,
        remark: values.remark?.trim() || undefined,
      };
      await saveSocialProfile(data);
      message.success('社保台账已保存');
      setProfileOpen(false);
      if (data.profile_year === year) {
        fetchProfiles(year);
      }
    } catch (e: unknown) {
      message.error('保存失败: ' + errText(e));
    } finally {
      setProfileSaving(false);
    }
  };

  const handleDelete = async (id: number) => {
    try {
      await deleteSocialProfile(id);
      message.success('已删除');
      fetchProfiles(year);
    } catch (e: unknown) {
      message.error('删除失败: ' + errText(e));
    }
  };

  const openCopy = () => {
    copyForm.resetFields();
    copyForm.setFieldsValue({
      from_year: year - 1,
      to_year: year,
      factor: 1.0,
      apply_clamp: true,
    });
    setCopyOpen(true);
  };

  const handleCopy = async () => {
    const values = await copyForm.validateFields();
    setCopySaving(true);
    try {
      const n = await copySocialProfiles(values.from_year, values.to_year, values.factor, values.apply_clamp);
      message.success(`年度调基完成，共复制 ${n} 条台账`);
      setCopyOpen(false);
      setYear(values.to_year);
      fetchProfiles(values.to_year);
    } catch (e: unknown) {
      message.error('调基失败: ' + errText(e));
    } finally {
      setCopySaving(false);
    }
  };

  const openLimits = async () => {
    setLimitsOpen(true);
    try {
      const [ssMin, ssMax, hfMin, hfMax] = await getSocialBaseLimits();
      limitsForm.setFieldsValue({ ss_min: ssMin, ss_max: ssMax, hf_min: hfMin, hf_max: hfMax });
    } catch (e: unknown) {
      message.error('获取基数上下限失败: ' + errText(e));
      limitsForm.setFieldsValue({ ss_min: 0, ss_max: 0, hf_min: 0, hf_max: 0 });
    }
  };

  const handleSaveLimits = async () => {
    const values = await limitsForm.validateFields();
    setLimitsSaving(true);
    try {
      await setSocialBaseLimits(values.ss_min, values.ss_max, values.hf_min, values.hf_max);
      message.success('基数上下限已保存');
      setLimitsOpen(false);
    } catch (e: unknown) {
      message.error('保存失败: ' + errText(e));
    } finally {
      setLimitsSaving(false);
    }
  };

  const columns: ColumnsType<SocialInsuranceProfile> = useMemo(
    () => [
      { title: '工号', dataIndex: 'employee_no', key: 'employee_no', width: 110 },
      { title: '社保基数', dataIndex: 'ss_base', key: 'ss_base', align: 'right', render: fmtAmount },
      { title: '公积金基数', dataIndex: 'hf_base', key: 'hf_base', align: 'right', render: fmtAmount },
      { title: '社保单位率', dataIndex: 'ss_employer_rate', key: 'ss_employer_rate', align: 'right' },
      { title: '社保个人率', dataIndex: 'ss_personal_rate', key: 'ss_personal_rate', align: 'right' },
      { title: '公积金单位率', dataIndex: 'hf_employer_rate', key: 'hf_employer_rate', align: 'right' },
      { title: '公积金个人率', dataIndex: 'hf_personal_rate', key: 'hf_personal_rate', align: 'right' },
      { title: '备注', dataIndex: 'remark', key: 'remark', ellipsis: true },
      {
        title: '操作',
        key: 'action',
        width: 140,
        render: (_, record) => (
          <Space>
            <Button type="link" size="small" onClick={() => openEdit(record)}>
              编辑
            </Button>
            <Popconfirm title="确认删除该台账记录？" onConfirm={() => handleDelete(record.id)}>
              <Button type="link" size="small" danger>
                删除
              </Button>
            </Popconfirm>
          </Space>
        ),
      },
    ],
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [profiles],
  );

  return (
    <Card>
      <Title level={4} style={{ marginTop: 0 }}>
        社保台账
      </Title>
      <Space wrap style={{ marginBottom: 16 }}>
        <Select
          value={year}
          onChange={setYear}
          options={YEAR_OPTIONS}
          style={{ width: 120 }}
        />
        <Button type="primary" icon={<PlusOutlined />} onClick={openCreate}>
          新增台账
        </Button>
        <Button onClick={openCopy}>年度调基</Button>
        <Button onClick={openLimits}>基数上下限</Button>
        <Button icon={<ReloadOutlined />} onClick={() => fetchProfiles(year)}>
          刷新
        </Button>
      </Space>
      <Table
        rowKey="id"
        columns={columns}
        dataSource={profiles}
        loading={loading}
        size="middle"
        pagination={{ pageSize: 20, showTotal: (t) => `共 ${t} 条` }}
      />

      <Modal
        title={editing ? '编辑社保台账' : '新增社保台账'}
        open={profileOpen}
        onOk={handleSaveProfile}
        confirmLoading={profileSaving}
        onCancel={() => setProfileOpen(false)}
        destroyOnHidden
      >
        <Form form={form} layout="vertical">
          <Form.Item
            name="employee_no"
            label="工号"
            rules={[{ required: true, message: '请输入工号' }]}
          >
            <Input disabled={!!editing} placeholder="员工工号" />
          </Form.Item>
          <Form.Item
            name="profile_year"
            label="年度"
            rules={[{ required: true, message: '请选择年度' }]}
          >
            <Select options={YEAR_OPTIONS} style={{ width: 120 }} />
          </Form.Item>
          <Space size="middle" style={{ display: 'flex' }}>
            <Form.Item name="ss_base" label="社保基数" initialValue={0}>
              <InputNumber min={0} style={{ width: 160 }} />
            </Form.Item>
            <Form.Item name="hf_base" label="公积金基数" initialValue={0}>
              <InputNumber min={0} style={{ width: 160 }} />
            </Form.Item>
          </Space>
          <Space size="middle" style={{ display: 'flex' }}>
            <Form.Item name="ss_employer_rate" label="社保单位率（小数）" initialValue={0}>
              <InputNumber min={0} max={1} step={0.001} placeholder={RATE_PLACEHOLDER} style={{ width: 160 }} />
            </Form.Item>
            <Form.Item name="ss_personal_rate" label="社保个人率（小数）" initialValue={0}>
              <InputNumber min={0} max={1} step={0.001} placeholder={RATE_PLACEHOLDER} style={{ width: 160 }} />
            </Form.Item>
          </Space>
          <Space size="middle" style={{ display: 'flex' }}>
            <Form.Item name="hf_employer_rate" label="公积金单位率（小数）" initialValue={0}>
              <InputNumber min={0} max={1} step={0.001} placeholder={RATE_PLACEHOLDER} style={{ width: 160 }} />
            </Form.Item>
            <Form.Item name="hf_personal_rate" label="公积金个人率（小数）" initialValue={0}>
              <InputNumber min={0} max={1} step={0.001} placeholder={RATE_PLACEHOLDER} style={{ width: 160 }} />
            </Form.Item>
          </Space>
          <Form.Item name="remark" label="备注">
            <Input.TextArea rows={2} placeholder="备注（可选）" />
          </Form.Item>
        </Form>
      </Modal>

      <Modal
        title="年度调基"
        open={copyOpen}
        onOk={handleCopy}
        confirmLoading={copySaving}
        onCancel={() => setCopyOpen(false)}
        destroyOnHidden
      >
        <Form form={copyForm} layout="vertical">
          <Space size="middle" style={{ display: 'flex' }}>
            <Form.Item name="from_year" label="源年度" rules={[{ required: true }]}>
              <Select options={YEAR_OPTIONS} style={{ width: 120 }} />
            </Form.Item>
            <Form.Item name="to_year" label="目标年度" rules={[{ required: true }]}>
              <Select options={YEAR_OPTIONS} style={{ width: 120 }} />
            </Form.Item>
          </Space>
          <Form.Item
            name="factor"
            label="调基系数"
            rules={[{ required: true, message: '请输入调基系数' }]}
            extra="新基数 = 原基数 × 系数"
          >
            <InputNumber min={0} step={0.01} style={{ width: 160 }} />
          </Form.Item>
          <Form.Item name="apply_clamp" label="按上下限截断" valuePropName="checked">
            <Switch />
          </Form.Item>
        </Form>
      </Modal>

      <Modal
        title="基数上下限"
        open={limitsOpen}
        onOk={handleSaveLimits}
        confirmLoading={limitsSaving}
        onCancel={() => setLimitsOpen(false)}
        destroyOnHidden
      >
        <Form form={limitsForm} layout="vertical">
          <Space size="middle" style={{ display: 'flex' }}>
            <Form.Item name="ss_min" label="社保基数下限">
              <InputNumber min={0} style={{ width: 160 }} />
            </Form.Item>
            <Form.Item name="ss_max" label="社保基数上限">
              <InputNumber min={0} style={{ width: 160 }} />
            </Form.Item>
          </Space>
          <Space size="middle" style={{ display: 'flex' }}>
            <Form.Item name="hf_min" label="公积金基数下限">
              <InputNumber min={0} style={{ width: 160 }} />
            </Form.Item>
            <Form.Item name="hf_max" label="公积金基数上限">
              <InputNumber min={0} style={{ width: 160 }} />
            </Form.Item>
          </Space>
          <Typography.Text type="secondary">0 表示不限制</Typography.Text>
        </Form>
      </Modal>
    </Card>
  );
};

export default SocialInsurance;
