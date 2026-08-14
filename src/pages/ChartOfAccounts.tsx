import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Alert,
  Button,
  DatePicker,
  Form,
  Input,
  InputNumber,
  Modal,
  Select,
  Space,
  Spin,
  Table,
  Tabs,
  Tag,
  Typography,
  message,
} from 'antd';
import type { ColumnsType } from 'antd/es/table';
import { PlusOutlined, ReloadOutlined, WalletOutlined } from '@ant-design/icons';
import dayjs from 'dayjs';
import type { Dayjs } from 'dayjs';
import {
  createGlAccount,
  getGlAccounts,
  getOpeningBalances,
  saveOpeningBalances,
  setGlAccountActive,
} from '@/api';
import type { GlAccount, GlAccountInput, OpeningBalanceRow } from '@/types';

const { Text } = Typography;

const CATEGORY_LABEL: Record<string, string> = {
  asset: '资产',
  liability: '负债',
  equity: '权益',
  cost: '成本',
  profit_loss: '损益',
};

const CFC_LABEL: Record<string, string> = {
  operating: '经营活动',
  investing: '投资活动',
  financing: '筹资活动',
  none: '不分类',
};

const DIRECTION_LABEL: Record<string, string> = {
  debit: '借方',
  credit: '贷方',
};

const CATEGORY_OPTIONS = Object.entries(CATEGORY_LABEL).map(([value, label]) => ({ value, label }));
const DIRECTION_OPTIONS = Object.entries(DIRECTION_LABEL).map(([value, label]) => ({ value, label }));
const CFC_OPTIONS = Object.entries(CFC_LABEL).map(([value, label]) => ({ value, label }));

const errText = (e: unknown): string => (e instanceof Error ? e.message : String(e));

const fmtAmount = (value: number): string =>
  (Number(value) || 0).toLocaleString('zh-CN', { minimumFractionDigits: 2, maximumFractionDigits: 2 });

// 期初余额编辑行：科目信息 + 借贷两侧金额
interface ObRow {
  code: string;
  name: string;
  category: GlAccount['category'];
  direction: GlAccount['direction'];
  debit: number;
  credit: number;
}

const ChartOfAccounts: React.FC = () => {
  const [accounts, setAccounts] = useState<GlAccount[]>([]);
  const [loading, setLoading] = useState(false);
  const [activeCategory, setActiveCategory] = useState<string>('all');

  const [addOpen, setAddOpen] = useState(false);
  const [savingAccount, setSavingAccount] = useState(false);
  const [form] = Form.useForm<GlAccountInput>();

  const [obOpen, setObOpen] = useState(false);
  const [obLoading, setObLoading] = useState(false);
  const [obSaving, setObSaving] = useState(false);
  const [obMonth, setObMonth] = useState<Dayjs>(dayjs());
  const [obRows, setObRows] = useState<ObRow[]>([]);

  const fetchAccounts = useCallback(async () => {
    setLoading(true);
    try {
      setAccounts(await getGlAccounts());
    } catch (e: unknown) {
      message.error('获取科目列表失败: ' + errText(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchAccounts();
  }, [fetchAccounts]);

  const filteredAccounts = useMemo(
    () => (activeCategory === 'all'
      ? accounts
      : accounts.filter((a) => a.category === activeCategory)),
    [accounts, activeCategory]
  );

  // 打开期初余额 Modal：加载已保存数据并与全部启用科目合并（未录制的科目金额为 0）
  const openOpeningBalances = async () => {
    setObOpen(true);
    setObLoading(true);
    try {
      const [state, list] = await Promise.all([getOpeningBalances(), getGlAccounts()]);
      if (state.month) {
        setObMonth(dayjs(state.month));
      }
      const saved = new Map(state.rows.map((r) => [r.account_code, r]));
      setObRows(
        list
          .filter((a) => a.is_active === 1)
          .map((a) => ({
            code: a.code,
            name: a.name,
            category: a.category,
            direction: a.direction,
            debit: saved.get(a.code)?.debit_amount ?? 0,
            credit: saved.get(a.code)?.credit_amount ?? 0,
          }))
      );
    } catch (e: unknown) {
      message.error('获取期初余额失败: ' + errText(e));
    } finally {
      setObLoading(false);
    }
  };

  const obDebit = useMemo(
    () => obRows.reduce((s, r) => s + (Number(r.debit) || 0), 0),
    [obRows]
  );
  const obCredit = useMemo(
    () => obRows.reduce((s, r) => s + (Number(r.credit) || 0), 0),
    [obRows]
  );
  const obDiff = obDebit - obCredit;
  const obBalanced = Math.abs(obDiff) < 0.005;

  const updateObRow = (code: string, field: 'debit' | 'credit', value: number | null) => {
    setObRows((rows) => rows.map((r) => (r.code === code ? { ...r, [field]: Number(value) || 0 } : r)));
  };

  const handleSaveOpeningBalances = async () => {
    if (!obBalanced) {
      message.error(`期初余额不平衡：借方合计 ${obDebit.toFixed(2)}，贷方合计 ${obCredit.toFixed(2)}，差额 ${obDiff.toFixed(2)}`);
      return;
    }
    const rows: OpeningBalanceRow[] = obRows
      .filter((r) => r.debit || r.credit)
      .map((r) => ({ account_code: r.code, debit_amount: r.debit, credit_amount: r.credit }));
    setObSaving(true);
    try {
      await saveOpeningBalances(obMonth.format('YYYY-MM'), rows);
      message.success(`已保存 ${obMonth.format('YYYY-MM')} 期初余额（${rows.length} 个科目）`);
      setObOpen(false);
    } catch (e: unknown) {
      message.error(errText(e) || '保存期初余额失败');
    } finally {
      setObSaving(false);
    }
  };

  const handleToggleActive = async (record: GlAccount) => {
    const target = record.is_active !== 1;
    try {
      await setGlAccountActive(record.code, target);
      message.success(`科目 ${record.code} 已${target ? '启用' : '停用'}`);
      await fetchAccounts();
    } catch (e: unknown) {
      message.error(errText(e) || '操作失败');
    }
  };

  const handleAddAccount = async () => {
    const values = await form.validateFields();
    setSavingAccount(true);
    try {
      const created = await createGlAccount({ ...values, cash_flow_category: values.cash_flow_category ?? 'none' });
      message.success(`科目 ${created.code} ${created.name} 已创建`);
      setAddOpen(false);
      form.resetFields();
      await fetchAccounts();
    } catch (e: unknown) {
      message.error(errText(e) || '新增科目失败');
    } finally {
      setSavingAccount(false);
    }
  };

  const columns: ColumnsType<GlAccount> = [
    { title: '编码', dataIndex: 'code', key: 'code', width: 110 },
    { title: '名称', dataIndex: 'name', key: 'name' },
    {
      title: '分类',
      dataIndex: 'category',
      key: 'category',
      width: 90,
      render: (v: string) => CATEGORY_LABEL[v] ?? v,
    },
    {
      title: '方向',
      dataIndex: 'direction',
      key: 'direction',
      width: 80,
      render: (v: string) => DIRECTION_LABEL[v] ?? v,
    },
    {
      title: '现金流量分类',
      dataIndex: 'cash_flow_category',
      key: 'cash_flow_category',
      width: 120,
      render: (v: string) => CFC_LABEL[v] ?? v,
    },
    {
      title: '状态',
      dataIndex: 'is_active',
      key: 'is_active',
      width: 80,
      render: (v: number) => (v === 1 ? <Tag color="green">启用</Tag> : <Tag>停用</Tag>),
    },
    {
      title: '操作',
      key: 'action',
      width: 90,
      render: (_, record: GlAccount) => (
        <Button
          type="link"
          size="small"
          danger={record.is_active === 1}
          disabled={savingAccount}
          onClick={() => handleToggleActive(record)}
        >
          {record.is_active === 1 ? '停用' : '启用'}
        </Button>
      ),
    },
  ];

  const tabItems = [
    { key: 'all', label: `全部 (${accounts.length})` },
    ...CATEGORY_OPTIONS.map(({ value, label }) => ({
      key: value,
      label: `${label} (${accounts.filter((a) => a.category === value).length})`,
    })),
  ];

  // 期初编辑列：借方科目只能填借方，贷方科目只能填贷方（与后端校验一致）
  const obColumns: ColumnsType<ObRow> = [
    { title: '编码', dataIndex: 'code', key: 'code', width: 110 },
    {
      title: '名称',
      dataIndex: 'name',
      key: 'name',
      render: (_: unknown, record: ObRow) => (
        <Space size={4}>
          <span>{record.name}</span>
          <Tag>{CATEGORY_LABEL[record.category] ?? record.category}</Tag>
        </Space>
      ),
    },
    {
      title: '借方金额',
      key: 'debit',
      width: 160,
      align: 'right' as const,
      render: (_: unknown, record: ObRow) => (
        <InputNumber
          value={record.debit}
          min={0}
          precision={2}
          disabled={record.direction !== 'debit'}
          onChange={(v) => updateObRow(record.code, 'debit', v)}
          style={{ width: '100%' }}
        />
      ),
    },
    {
      title: '贷方金额',
      key: 'credit',
      width: 160,
      align: 'right' as const,
      render: (_: unknown, record: ObRow) => (
        <InputNumber
          value={record.credit}
          min={0}
          precision={2}
          disabled={record.direction !== 'credit'}
          onChange={(v) => updateObRow(record.code, 'credit', v)}
          style={{ width: '100%' }}
        />
      ),
    },
  ];

  return (
    <div>
      <div className="page-header">
        <span className="page-title">科目表</span>
        <div className="page-header-actions">
          <Button type="primary" icon={<PlusOutlined />} onClick={() => setAddOpen(true)}>
            新增科目
          </Button>
          <Button icon={<WalletOutlined />} onClick={openOpeningBalances}>
            期初余额
          </Button>
          <Button icon={<ReloadOutlined />} onClick={fetchAccounts} loading={loading}>
            刷新
          </Button>
        </div>
      </div>

      <Tabs
        activeKey={activeCategory}
        onChange={setActiveCategory}
        items={tabItems}
        style={{ marginLeft: 4 }}
      />

      <Table<GlAccount>
        rowKey="code"
        columns={columns}
        dataSource={filteredAccounts}
        loading={loading}
        pagination={{ pageSize: 20, showSizeChanger: true, showTotal: (t) => `共 ${t} 条` }}
        size="middle"
      />

      <Modal
        title="新增科目"
        open={addOpen}
        onOk={handleAddAccount}
        onCancel={() => setAddOpen(false)}
        confirmLoading={savingAccount}
        okText="保存"
        cancelText="取消"
      >
        <Form form={form} layout="vertical" initialValues={{ cash_flow_category: 'none' }}>
          <Form.Item
            name="code"
            label="科目编码"
            rules={[
              { required: true, message: '请输入科目编码' },
              {
                validator: async (_, value: string) => {
                  const code = String(value ?? '').trim();
                  if (!code) return;
                  if (accounts.some((a) => a.code === code)) {
                    throw new Error(`科目编码 ${code} 已存在`);
                  }
                },
              },
            ]}
          >
            <Input placeholder="如 100201" maxLength={20} />
          </Form.Item>
          <Form.Item
            name="name"
            label="科目名称"
            rules={[{ required: true, message: '请输入科目名称' }]}
          >
            <Input placeholder="如 银行存款-工行" maxLength={50} />
          </Form.Item>
          <Form.Item
            name="category"
            label="科目分类"
            rules={[{ required: true, message: '请选择科目分类' }]}
          >
            <Select options={CATEGORY_OPTIONS} placeholder="选择分类" />
          </Form.Item>
          <Form.Item
            name="direction"
            label="余额方向"
            rules={[{ required: true, message: '请选择余额方向' }]}
          >
            <Select options={DIRECTION_OPTIONS} placeholder="选择方向" />
          </Form.Item>
          <Form.Item name="cash_flow_category" label="现金流量分类">
            <Select options={CFC_OPTIONS} />
          </Form.Item>
          <Form.Item name="remark" label="备注">
            <Input.TextArea rows={2} maxLength={200} placeholder="选填" />
          </Form.Item>
        </Form>
      </Modal>

      <Modal
        title="期初余额录入"
        open={obOpen}
        onOk={handleSaveOpeningBalances}
        onCancel={() => setObOpen(false)}
        confirmLoading={obSaving}
        okText="保存"
        cancelText="取消"
        okButtonProps={{ disabled: !obBalanced }}
        width={760}
      >
        <Spin spinning={obLoading}>
          <Alert
            type="info"
            showIcon
            style={{ marginBottom: 12 }}
            message="录入启用科目的期初余额"
            description="借方科目填借方金额，贷方科目填贷方金额；保存时借贷合计必须相等，未填金额的科目不会保存。保存会整体覆盖已有期初数据。"
          />
          <div style={{ marginBottom: 12 }}>
            <span style={{ marginRight: 8 }}>期初月份：</span>
            <DatePicker
              picker="month"
              value={obMonth}
              onChange={(d) => d && setObMonth(d)}
              allowClear={false}
              style={{ width: 160 }}
            />
          </div>
          <Table<ObRow>
            rowKey="code"
            columns={obColumns}
            dataSource={obRows}
            pagination={false}
            size="small"
            scroll={{ y: 360 }}
            summary={() => (
              <Table.Summary fixed>
                <Table.Summary.Row>
                  <Table.Summary.Cell index={0} colSpan={2}>
                    <Text strong>合计</Text>
                  </Table.Summary.Cell>
                  <Table.Summary.Cell index={2} align="right">
                    <Text strong>{fmtAmount(obDebit)}</Text>
                  </Table.Summary.Cell>
                  <Table.Summary.Cell index={3} align="right">
                    <Text strong>{fmtAmount(obCredit)}</Text>
                  </Table.Summary.Cell>
                </Table.Summary.Row>
                <Table.Summary.Row>
                  <Table.Summary.Cell index={0} colSpan={4}>
                    {obBalanced ? (
                      <Text type="success" strong>借贷平衡（差额 0.00）</Text>
                    ) : (
                      <Text type="danger" strong>
                        借贷不平衡：差额 {fmtAmount(obDiff)}，请调整后保存
                      </Text>
                    )}
                  </Table.Summary.Cell>
                </Table.Summary.Row>
              </Table.Summary>
            )}
          />
        </Spin>
      </Modal>
    </div>
  );
};

export default ChartOfAccounts;
