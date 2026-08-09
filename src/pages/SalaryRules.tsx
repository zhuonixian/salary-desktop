import { useState, useEffect, useCallback } from 'react';
import {
  Tabs, Form, InputNumber, Button, Table, message, Spin, Card, Popconfirm, Select, Input, Alert,
} from 'antd';
import { SaveOutlined, PlusOutlined, DeleteOutlined } from '@ant-design/icons';
import { useSearchParams } from 'react-router-dom';
import { getSalaryRule, saveSalaryRule, getTaxRules, saveTaxRules, getOcrSettings, saveOcrSettings } from '@/api';
import type { OcrSettingsInput, SalaryRule, TaxRule, TaxRuleInput } from '@/types';

const SalaryRules: React.FC = () => {
  const [searchParams, setSearchParams] = useSearchParams();
  const activeTab = searchParams.get('tab') ?? 'attendance';
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);

  // 考勤扣款规则 & 社保公积金
  const [ruleForm] = Form.useForm();
  const [ruleId, setRuleId] = useState<number>(0);
  const [systemForm] = Form.useForm<OcrSettingsInput>();

  // 个税税率表
  const [taxRules, setTaxRules] = useState<TaxRuleInput[]>([]);

  const fetchRule = useCallback(async () => {
    setLoading(true);
    try {
      const rule = await getSalaryRule();
      setRuleId(rule.id);
      ruleForm.setFieldsValue({
        late_penalty: rule.late_penalty,
        early_leave_penalty: rule.early_leave_penalty,
        personal_leave_rate: rule.personal_leave_rate,
        sick_leave_rate: rule.sick_leave_rate,
        absent_rate: rule.absent_rate,
        overtime_rate: rule.overtime_rate,
        social_insurance_rate: rule.social_insurance_rate,
        housing_fund_rate: rule.housing_fund_rate,
        tax_threshold: rule.tax_threshold,
      });
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      message.error('获取规则失败: ' + msg);
    } finally {
      setLoading(false);
    }
  }, [ruleForm]);

  const fetchTaxRules = useCallback(async () => {
    try {
      const rules = await getTaxRules();
      setTaxRules(
        rules.map((r: TaxRule) => ({
          level: r.level,
          min_amount: r.min_amount,
          max_amount: r.max_amount,
          tax_rate: r.tax_rate,
          quick_deduction: r.quick_deduction,
        }))
      );
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      message.error('获取税率表失败: ' + msg);
    }
  }, []);

  const fetchOcrSettings = useCallback(async () => {
    try {
      const settings = await getOcrSettings();
      systemForm.setFieldsValue(settings);
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      message.error('获取系统设置失败: ' + msg);
    }
  }, [systemForm]);

  useEffect(() => {
    fetchRule();
    fetchTaxRules();
    fetchOcrSettings();
  }, [fetchRule, fetchTaxRules, fetchOcrSettings]);

  const handleSaveRule = async () => {
    setSaving(true);
    try {
      const values = await ruleForm.validateFields();
      await saveSalaryRule({ id: ruleId, ...values, created_at: '', updated_at: '' } as SalaryRule);
      message.success('保存成功');
      fetchRule();
    } catch (e: unknown) {
      if (e instanceof Error) {
        message.error('保存失败: ' + e.message);
      }
    } finally {
      setSaving(false);
    }
  };

  const handleSaveTaxRules = async () => {
    setSaving(true);
    try {
      await saveTaxRules(taxRules);
      message.success('税率表保存成功');
      fetchTaxRules();
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      message.error('保存失败: ' + msg);
    } finally {
      setSaving(false);
    }
  };

  const handleSaveSystemSettings = async () => {
    setSaving(true);
    try {
      const values = await systemForm.validateFields();
      await saveOcrSettings({
        ...values,
        baidu_api_key: values.baidu_api_key?.trim(),
        baidu_secret_key: values.baidu_secret_key?.trim(),
      });
      message.success('系统设置保存成功');
      fetchOcrSettings();
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      message.error('保存失败: ' + msg);
    } finally {
      setSaving(false);
    }
  };

  const handleAddTaxRow = () => {
    setTaxRules([
      ...taxRules,
      {
        level: taxRules.length + 1,
        min_amount: 0,
        max_amount: 0,
        tax_rate: 0,
        quick_deduction: 0,
      },
    ]);
  };

  const handleRemoveTaxRow = (index: number) => {
    const newRules = taxRules.filter((_, i) => i !== index).map((r, i) => ({ ...r, level: i + 1 }));
    setTaxRules(newRules);
  };

  const handleTaxCellChange = (index: number, field: keyof TaxRuleInput, value: number) => {
    const newRules = [...taxRules];
    newRules[index] = { ...newRules[index], [field]: value };
    setTaxRules(newRules);
  };

  const taxColumns = [
    {
      title: '级数',
      dataIndex: 'level',
      key: 'level',
      width: 70,
      align: 'center' as const,
    },
    {
      title: '应纳税所得额下限',
      dataIndex: 'min_amount',
      key: 'min_amount',
      width: 180,
      render: (v: number, _: TaxRuleInput, idx: number) => (
        <InputNumber
          value={v}
          min={0}
          precision={2}
          style={{ width: '100%' }}
          onChange={(val) => handleTaxCellChange(idx, 'min_amount', val ?? 0)}
        />
      ),
    },
    {
      title: '应纳税所得额上限',
      dataIndex: 'max_amount',
      key: 'max_amount',
      width: 180,
      render: (v: number, _: TaxRuleInput, idx: number) => (
        <InputNumber
          value={v}
          min={0}
          precision={2}
          style={{ width: '100%' }}
          onChange={(val) => handleTaxCellChange(idx, 'max_amount', val ?? 0)}
        />
      ),
    },
    {
      title: '税率(%)',
      dataIndex: 'tax_rate',
      key: 'tax_rate',
      width: 120,
      render: (v: number, _: TaxRuleInput, idx: number) => (
        <InputNumber
          value={v}
          min={0}
          max={100}
          precision={2}
          style={{ width: '100%' }}
          addonAfter="%"
          onChange={(val) => handleTaxCellChange(idx, 'tax_rate', val ?? 0)}
        />
      ),
    },
    {
      title: '速算扣除数',
      dataIndex: 'quick_deduction',
      key: 'quick_deduction',
      width: 150,
      render: (v: number, _: TaxRuleInput, idx: number) => (
        <InputNumber
          value={v}
          min={0}
          precision={2}
          style={{ width: '100%' }}
          onChange={(val) => handleTaxCellChange(idx, 'quick_deduction', val ?? 0)}
        />
      ),
    },
    {
      title: '操作',
      key: 'action',
      width: 60,
      render: (_: unknown, __: TaxRuleInput, idx: number) => (
        <Popconfirm title="确认删除该级?" onConfirm={() => handleRemoveTaxRow(idx)} okText="确认" cancelText="取消">
          <Button type="link" danger icon={<DeleteOutlined />} size="small" />
        </Popconfirm>
      ),
    },
  ];

  const tabItems = [
    {
      key: 'attendance',
      label: '考勤扣款规则',
      children: (
        <Spin spinning={loading}>
          <Form form={ruleForm} layout="vertical" style={{ maxWidth: 600 }}>
            <Form.Item name="late_penalty" label="迟到扣款(元/次)" rules={[{ required: true }]}>
              <InputNumber min={0} precision={2} style={{ width: '100%' }} addonAfter="元" />
            </Form.Item>
            <Form.Item name="early_leave_penalty" label="早退扣款(元/次)" rules={[{ required: true }]}>
              <InputNumber min={0} precision={2} style={{ width: '100%' }} addonAfter="元" />
            </Form.Item>
            <Form.Item name="personal_leave_rate" label="事假扣款倍率(日工资倍数)" rules={[{ required: true }]}>
              <InputNumber min={0} precision={2} style={{ width: '100%' }} />
            </Form.Item>
            <Form.Item name="sick_leave_rate" label="病假扣款倍率(日工资倍数)" rules={[{ required: true }]}>
              <InputNumber min={0} precision={2} style={{ width: '100%' }} />
            </Form.Item>
            <Form.Item name="absent_rate" label="旷工扣款倍率(日工资倍数)" rules={[{ required: true }]}>
              <InputNumber min={0} precision={2} style={{ width: '100%' }} />
            </Form.Item>
            <Form.Item name="overtime_rate" label="加班工资倍率(日工资倍数)" rules={[{ required: true }]}>
              <InputNumber min={0} precision={2} style={{ width: '100%' }} />
            </Form.Item>
            <Form.Item>
              <Button type="primary" icon={<SaveOutlined />} onClick={handleSaveRule} loading={saving}>
                保存规则
              </Button>
            </Form.Item>
          </Form>
        </Spin>
      ),
    },
    {
      key: 'insurance',
      label: '社保公积金',
      children: (
        <Spin spinning={loading}>
          <Form form={ruleForm} layout="vertical" style={{ maxWidth: 600 }}>
            <Form.Item name="social_insurance_rate" label="社保个人缴纳比例(%)" rules={[{ required: true }]}>
              <InputNumber min={0} max={100} precision={2} style={{ width: '100%' }} addonAfter="%" />
            </Form.Item>
            <Form.Item name="housing_fund_rate" label="公积金缴纳比例(%)" rules={[{ required: true }]}>
              <InputNumber min={0} max={100} precision={2} style={{ width: '100%' }} addonAfter="%" />
            </Form.Item>
            <Form.Item name="tax_threshold" label="个税起征点(元)" rules={[{ required: true }]}>
              <InputNumber min={0} precision={2} style={{ width: '100%' }} addonAfter="元" />
            </Form.Item>
            <Form.Item>
              <Button type="primary" icon={<SaveOutlined />} onClick={handleSaveRule} loading={saving}>
                保存规则
              </Button>
            </Form.Item>
          </Form>
        </Spin>
      ),
    },
    {
      key: 'tax',
      label: '个税税率表',
      children: (
        <div>
          <div style={{ marginBottom: 16, display: 'flex', justifyContent: 'space-between' }}>
            <Button icon={<PlusOutlined />} onClick={handleAddTaxRow}>
              添加税率级数
            </Button>
            <Button type="primary" icon={<SaveOutlined />} onClick={handleSaveTaxRules} loading={saving}>
              保存税率表
            </Button>
          </div>
          <Table
            rowKey={(_, idx) => String(idx)}
            columns={taxColumns}
            dataSource={taxRules}
            pagination={false}
            size="middle"
            bordered
          />
        </div>
      ),
    },
    {
      key: 'system',
      label: '系统设置',
      children: (
        <Form
          form={systemForm}
          layout="vertical"
          initialValues={{ ocr_mode: 'online', ocr_provider: 'baidu' }}
          style={{ maxWidth: 680 }}
        >
          <Alert
            showIcon
            type="info"
            style={{ marginBottom: 16 }}
            title="OCR 接口配置为全局系统设置，OCR识别中心和打卡表识别会使用这里保存的配置。"
          />
          <Form.Item name="ocr_mode" label="默认识别模式" rules={[{ required: true }]}>
            <Select
              options={[
                { label: '在线识别', value: 'online' },
                { label: '本地识别', value: 'local' },
              ]}
            />
          </Form.Item>
          <Form.Item name="ocr_provider" label="在线 OCR 平台" rules={[{ required: true }]}>
            <Select
              options={[
                { label: '百度 OCR', value: 'baidu' },
              ]}
            />
          </Form.Item>
          <Form.Item name="baidu_api_key" label="百度 OCR API Key">
            <Input placeholder="输入百度智能云 API Key" />
          </Form.Item>
          <Form.Item name="baidu_secret_key" label="百度 OCR Secret Key">
            <Input.Password placeholder="输入百度智能云 Secret Key" />
          </Form.Item>
          <Alert
            type="warning"
            showIcon
            style={{ marginBottom: 16 }}
            title="目前在线识别默认使用百度 OCR；后续新增平台时会在“在线 OCR 平台”下拉框中扩展。"
          />
          <Form.Item>
            <Button type="primary" icon={<SaveOutlined />} onClick={handleSaveSystemSettings} loading={saving}>
              保存系统设置
            </Button>
          </Form.Item>
        </Form>
      ),
    },
  ];

  return (
    <div>
      <div className="page-header">
        <span className="page-title">系统设置</span>
      </div>

      <Card>
        <Tabs
          activeKey={activeTab}
          items={tabItems}
          onChange={(key) => setSearchParams(key === 'attendance' ? {} : { tab: key })}
        />
      </Card>
    </div>
  );
};

export default SalaryRules;
