import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Button,
  Card,
  Col,
  DatePicker,
  Form,
  Input,
  InputNumber,
  Modal,
  Popconfirm,
  Row,
  Select,
  Space,
  Spin,
  Table,
  Tag,
  message,
} from 'antd';
import type { ColumnsType } from 'antd/es/table';
import {
  CalculatorOutlined,
  DownloadOutlined,
  PlusOutlined,
  ReloadOutlined,
} from '@ant-design/icons';
import dayjs, { type Dayjs } from 'dayjs';
import { save } from '@tauri-apps/plugin-dialog';
import {
  disposeFixedAsset,
  exportFixedAssetLedger,
  getFixedAssets,
  getFundAccounts,
  getGlAccounts,
  runAssetDepreciation,
  saveFixedAsset,
} from '@/api';
import { SensitiveStatistic } from '@/components/SensitiveStatistic';
import { SensitiveText } from '@/components/SensitiveText';
import {
  FIXED_ASSET_CATEGORY_BY_LABEL,
  FIXED_ASSET_CATEGORY_LABEL,
  type FixedAssetCategory,
  type FixedAssetLedgerRow,
  type FundAccount,
  type GlAccount,
} from '@/types';

const fmtMoney = (value?: number | null): string =>
  (Number(value) || 0).toLocaleString('zh-CN', { minimumFractionDigits: 2, maximumFractionDigits: 2 });

const errText = (e: unknown): string => (e instanceof Error ? e.message : String(e));

/** 资金科目编码（库存现金/银行存款/其他货币资金，与后端门禁一致） */
const isFundGlCode = (code?: string): boolean =>
  code === '1001' || code === '1002' || code === '1012';

interface AssetFormValues {
  name: string;
  category: FixedAssetCategory;
  purchase_date: Dayjs;
  belong_month: Dayjs;
  original_value: number;
  salvage_rate: number;
  useful_life_months: number;
  expense_account_code: string;
  counter_account_code?: string;
  counter_fund_account_id?: number;
  remark?: string;
}

interface DisposalFormValues {
  disposal_date: Dayjs;
  disposal_income: number;
  income_fund_account_id?: number;
  remark?: string;
}

/** 固定资产台账（第十三阶段 记账完整性）：登记/折旧计提/处置/导出 */
const FixedAssets: React.FC = () => {
  const [rows, setRows] = useState<FixedAssetLedgerRow[]>([]);
  const [loading, setLoading] = useState(false);
  const [keyword, setKeyword] = useState('');
  const [statusFilter, setStatusFilter] = useState<string | undefined>(undefined);

  // 基础资料（科目/资金账户下拉）
  const [glAccounts, setGlAccounts] = useState<GlAccount[]>([]);
  const [fundAccounts, setFundAccounts] = useState<FundAccount[]>([]);

  // 登记/编辑 Modal
  const [assetOpen, setAssetOpen] = useState(false);
  const [editTarget, setEditTarget] = useState<FixedAssetLedgerRow | null>(null);
  const [savingAsset, setSavingAsset] = useState(false);
  const [assetForm] = Form.useForm<AssetFormValues>();
  const watchCounterCode = Form.useWatch('counter_account_code', assetForm);

  // 计提折旧 Modal
  const [depOpen, setDepOpen] = useState(false);
  const [depMonth, setDepMonth] = useState<Dayjs>(dayjs());
  const [depRunning, setDepRunning] = useState(false);

  // 处置 Modal
  const [disposeTarget, setDisposeTarget] = useState<FixedAssetLedgerRow | null>(null);
  const [disposing, setDisposing] = useState(false);
  const [disposeForm] = Form.useForm<DisposalFormValues>();
  const watchDisposalIncome = Form.useWatch('disposal_income', disposeForm);

  const [exporting, setExporting] = useState(false);

  const fetchData = useCallback(async () => {
    setLoading(true);
    try {
      setRows(
        await getFixedAssets({
          keyword: keyword.trim() || undefined,
          status: statusFilter || undefined,
        }),
      );
    } catch (e: unknown) {
      message.error('查询固定资产台账失败: ' + errText(e));
    } finally {
      setLoading(false);
    }
  }, [keyword, statusFilter]);

  useEffect(() => {
    void fetchData();
  }, [fetchData]);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const [accounts, funds] = await Promise.all([getGlAccounts(), getFundAccounts({})]);
        if (cancelled) return;
        setGlAccounts(accounts.filter((a) => a.is_active === 1));
        setFundAccounts(funds.filter((f) => f.is_active));
      } catch {
        // 下拉基础资料加载失败不阻断列表：打开 Modal 时会再次提示
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // 折旧费用科目候选：5/6 开头（管理费用/制造费用/主营业务成本等）
  const expenseAccountOptions = useMemo(
    () =>
      glAccounts
        .filter((a) => a.code.startsWith('6') || a.code.startsWith('5'))
        .map((a) => ({ value: a.code, label: `${a.code} ${a.name}` })),
    [glAccounts],
  );

  const allAccountOptions = useMemo(
    () => glAccounts.map((a) => ({ value: a.code, label: `${a.code} ${a.name}` })),
    [glAccounts],
  );

  /** 资金账户候选：优先按科目挂接（gl_account_code）精确匹配，无挂接再按现金/银行类型兜底 */
  const fundOptionsFor = useCallback(
    (code?: string): { value: number; label: string }[] => {
      if (!code) return [];
      const byGl = fundAccounts.filter((f) => f.gl_account_code === code);
      const pool = byGl.length > 0 ? byGl : fundAccounts.filter((f) =>
        f.account_type === (code === '1001' ? 'cash' : 'bank'));
      return pool.map((f) => ({
        value: f.id,
        label: `${f.name}（${f.account_code}）`,
      }));
    },
    [fundAccounts],
  );

  // 汇总（前端聚合）
  const summary = useMemo(
    () => ({
      originalTotal: rows.reduce((s, r) => s + r.original_value, 0),
      accumulatedTotal: rows.reduce((s, r) => s + r.accumulated_depreciation, 0),
      netTotal: rows.reduce((s, r) => s + r.net_book_value, 0),
      monthlyTotal: rows
        .filter((r) => r.status === 'in_use')
        .reduce((s, r) => s + r.monthly_amount, 0),
    }),
    [rows],
  );

  const openAssetModal = (row?: FixedAssetLedgerRow) => {
    setEditTarget(row ?? null);
    assetForm.resetFields();
    if (row) {
      assetForm.setFieldsValue({
        name: row.name,
        category: FIXED_ASSET_CATEGORY_BY_LABEL[row.category_label] ?? 'other',
        purchase_date: dayjs(row.purchase_date),
        belong_month: dayjs(row.belong_month),
        original_value: row.original_value,
        salvage_rate: row.salvage_rate,
        useful_life_months: row.useful_life_months,
        expense_account_code: row.expense_account_code,
      });
    }
    setAssetOpen(true);
  };

  const handleSaveAsset = async () => {
    let values: AssetFormValues;
    try {
      values = await assetForm.validateFields();
    } catch {
      return;
    }
    const counterCode = values.counter_account_code?.trim() || null;
    if (isFundGlCode(counterCode ?? undefined) && !values.counter_fund_account_id) {
      message.warning('购置贷方为资金科目时必须选择资金账户');
      return;
    }
    setSavingAsset(true);
    try {
      const saved = await saveFixedAsset({
        id: editTarget?.asset_id ?? null,
        name: values.name.trim(),
        category: values.category,
        purchase_date: values.purchase_date.format('YYYY-MM-DD'),
        belong_month: values.belong_month.format('YYYY-MM'),
        original_value: Number(values.original_value),
        salvage_rate: Number(values.salvage_rate),
        useful_life_months: Number(values.useful_life_months),
        expense_account_code: values.expense_account_code,
        counter_account_code: counterCode,
        counter_fund_account_id: isFundGlCode(counterCode ?? undefined)
          ? Number(values.counter_fund_account_id)
          : null,
        remark: values.remark?.trim() || null,
      });
      message.success(`资产 ${saved.asset_no} ${editTarget ? '已更新' : '已登记'}`);
      setAssetOpen(false);
      void fetchData();
    } catch (e: unknown) {
      message.error(errText(e) || '保存资产失败');
    } finally {
      setSavingAsset(false);
    }
  };

  const handleRunDepreciation = async () => {
    const month = depMonth.format('YYYY-MM');
    setDepRunning(true);
    try {
      const result = await runAssetDepreciation(month);
      if (result.asset_count === 0) {
        message.info(`${month} 无可计提资产（本月已计提或均已提满/处置）`);
      } else {
        message.success(
          `${month} 已计提 ${result.asset_count} 项资产折旧，合计 ${fmtMoney(result.total_amount)} 元` +
            (result.voucher_no ? `，凭证号 ${result.voucher_no}` : ''),
        );
      }
      setDepOpen(false);
      void fetchData();
    } catch (e: unknown) {
      message.error('计提折旧失败: ' + errText(e));
    } finally {
      setDepRunning(false);
    }
  };

  const openDisposeModal = (row: FixedAssetLedgerRow) => {
    setDisposeTarget(row);
    disposeForm.resetFields();
    disposeForm.setFieldsValue({ disposal_date: dayjs(), disposal_income: 0 });
  };

  const handleDispose = async () => {
    if (!disposeTarget) return;
    let values: DisposalFormValues;
    try {
      values = await disposeForm.validateFields();
    } catch {
      return;
    }
    setDisposing(true);
    try {
      const record = await disposeFixedAsset({
        id: disposeTarget.asset_id,
        disposal_date: values.disposal_date.format('YYYY-MM-DD'),
        disposal_income: Number(values.disposal_income ?? 0),
        income_fund_account_id:
          Number(values.disposal_income ?? 0) > 0 && values.income_fund_account_id
            ? Number(values.income_fund_account_id)
            : null,
        remark: values.remark?.trim() || null,
      });
      message.success(`资产 ${record.asset_no} 已处置`);
      setDisposeTarget(null);
      void fetchData();
    } catch (e: unknown) {
      message.error('处置失败: ' + errText(e));
    } finally {
      setDisposing(false);
    }
  };

  const handleExport = async () => {
    const target = await save({
      defaultPath: `固定资产台账_${dayjs().format('YYYY-MM-DD')}.xlsx`,
      filters: [{ name: 'Excel', extensions: ['xlsx'] }],
    });
    if (!target) return;
    setExporting(true);
    try {
      await exportFixedAssetLedger({ keyword: keyword.trim() || undefined }, String(target));
      message.success('固定资产台账已导出');
    } catch (e: unknown) {
      message.error('导出失败: ' + errText(e));
    } finally {
      setExporting(false);
    }
  };

  const columns: ColumnsType<FixedAssetLedgerRow> = [
    { title: '编号', dataIndex: 'asset_no', key: 'asset_no', width: 150, fixed: 'left' },
    { title: '名称', dataIndex: 'name', key: 'name', width: 180, ellipsis: true },
    { title: '类别', dataIndex: 'category_label', key: 'category_label', width: 100 },
    {
      title: '购置日',
      dataIndex: 'purchase_date',
      key: 'purchase_date',
      width: 110,
      render: (v: string) => v || '-',
    },
    { title: '归属月', dataIndex: 'belong_month', key: 'belong_month', width: 90 },
    {
      title: '原值',
      dataIndex: 'original_value',
      key: 'original_value',
      width: 120,
      align: 'right',
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '残值率(%)',
      dataIndex: 'salvage_rate',
      key: 'salvage_rate',
      width: 95,
      align: 'right',
      render: (v: number) => `${(Number(v) * 100).toLocaleString('zh-CN', { maximumFractionDigits: 2 })}%`,
    },
    { title: '年限月', dataIndex: 'useful_life_months', key: 'useful_life_months', width: 85, align: 'right' },
    {
      title: '费用科目',
      dataIndex: 'expense_account_code',
      key: 'expense_account_code',
      width: 100,
    },
    {
      title: '累计折旧',
      dataIndex: 'accumulated_depreciation',
      key: 'accumulated_depreciation',
      width: 120,
      align: 'right',
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '净值',
      dataIndex: 'net_book_value',
      key: 'net_book_value',
      width: 120,
      align: 'right',
      render: (v: number) => <SensitiveText type="amount" value={v} />,
    },
    {
      title: '月折旧',
      dataIndex: 'monthly_amount',
      key: 'monthly_amount',
      width: 110,
      align: 'right',
      render: (v: number, r) =>
        r.status === 'in_use' ? <SensitiveText type="amount" value={v} /> : <span style={{ color: '#8c8c8c' }}>—</span>,
    },
    {
      title: '已提|剩余月',
      key: 'months',
      width: 110,
      align: 'right',
      render: (_, r) => `${r.depreciated_months}|${r.remaining_months}`,
    },
    {
      title: '状态',
      dataIndex: 'status',
      key: 'status',
      width: 90,
      render: (v: string) =>
        v === 'in_use' ? <Tag color="green">在用</Tag> : <Tag>已处置</Tag>,
    },
    {
      title: '操作',
      key: 'op',
      width: 140,
      fixed: 'right',
      render: (_, r) =>
        r.status === 'in_use' ? (
          <Space size={0}>
            <Button size="small" type="link" onClick={() => openAssetModal(r)}>
              编辑
            </Button>
            <Button size="small" type="link" danger onClick={() => openDisposeModal(r)}>
              处置
            </Button>
          </Space>
        ) : (
          <span style={{ color: '#8c8c8c' }}>—</span>
        ),
    },
  ];

  return (
    <div>
      <div className="page-header">
        <span className="page-title">固定资产</span>
        <div className="page-header-actions">
          <Input.Search
            placeholder="名称/编号"
            allowClear
            style={{ width: 180 }}
            onSearch={setKeyword}
          />
          <Select
            placeholder="资产状态"
            allowClear
            value={statusFilter}
            onChange={setStatusFilter}
            style={{ width: 120 }}
            options={[
              { value: 'in_use', label: '在用' },
              { value: 'disposed', label: '已处置' },
            ]}
          />
          <Button icon={<ReloadOutlined />} loading={loading} onClick={() => void fetchData()}>
            刷新
          </Button>
          <Button icon={<DownloadOutlined />} loading={exporting} onClick={() => void handleExport()}>
            导出台账
          </Button>
          <Button icon={<CalculatorOutlined />} onClick={() => setDepOpen(true)}>
            计提本月折旧
          </Button>
          <Button type="primary" icon={<PlusOutlined />} onClick={() => openAssetModal()}>
            登记资产
          </Button>
        </div>
      </div>

      <Spin spinning={loading}>
        <Row gutter={[16, 16]} className="mb-16">
          <Col xs={24} sm={12} lg={6}>
            <Card className="stat-card">
              <SensitiveStatistic title="资产原值合计" value={summary.originalTotal} />
            </Card>
          </Col>
          <Col xs={24} sm={12} lg={6}>
            <Card className="stat-card">
              <SensitiveStatistic title="累计折旧合计" value={summary.accumulatedTotal} />
            </Card>
          </Col>
          <Col xs={24} sm={12} lg={6}>
            <Card className="stat-card">
              <SensitiveStatistic title="净值合计" value={summary.netTotal} />
            </Card>
          </Col>
          <Col xs={24} sm={12} lg={6}>
            <Card className="stat-card">
              <SensitiveStatistic title="本月在用月折旧合计" value={summary.monthlyTotal} />
            </Card>
          </Col>
        </Row>

        <Card title="固定资产台账" extra={<span style={{ color: '#8c8c8c' }}>月折旧 = 原值 × (1 − 残值率) ÷ 年限月；已提满或已处置不再计提</span>}>
          <Table<FixedAssetLedgerRow>
            rowKey="asset_id"
            columns={columns}
            dataSource={rows}
            pagination={{ pageSize: 15, showSizeChanger: true, showTotal: (t) => `共 ${t} 条` }}
            scroll={{ x: 1760 }}
          />
        </Card>
      </Spin>

      {/* 登记/编辑 Modal */}
      <Modal
        title={editTarget ? `编辑资产 - ${editTarget.asset_no}` : '登记固定资产'}
        open={assetOpen}
        onOk={() => void handleSaveAsset()}
        onCancel={() => setAssetOpen(false)}
        confirmLoading={savingAsset}
        okText="保存"
        cancelText="取消"
        width={640}
      >
        <Form<AssetFormValues>
          form={assetForm}
          layout="vertical"
          initialValues={{
            category: 'electronics',
            salvage_rate: 0.05,
            expense_account_code: '6602',
          }}
        >
          <Row gutter={12}>
            <Col span={12}>
              <Form.Item
                name="name"
                label="资产名称"
                rules={[{ required: true, message: '请输入资产名称' }]}
              >
                <Input placeholder="如 财务部笔记本电脑" maxLength={100} />
              </Form.Item>
            </Col>
            <Col span={12}>
              <Form.Item
                name="category"
                label="资产类别"
                rules={[{ required: true, message: '请选择资产类别' }]}
              >
                <Select
                  placeholder="选择类别"
                  options={Object.entries(FIXED_ASSET_CATEGORY_LABEL).map(([value, label]) => ({
                    value,
                    label,
                  }))}
                />
              </Form.Item>
            </Col>
          </Row>
          <Row gutter={12}>
            <Col span={12}>
              <Form.Item
                name="purchase_date"
                label="购置日期"
                rules={[{ required: true, message: '请选择购置日期' }]}
                extra="变更购置日期时自动带出归属月"
              >
                <DatePicker
                  style={{ width: '100%' }}
                  onChange={(d) => {
                    if (d) assetForm.setFieldValue('belong_month', d);
                  }}
                />
              </Form.Item>
            </Col>
            <Col span={12}>
              <Form.Item
                name="belong_month"
                label="归属月"
                rules={[{ required: true, message: '请选择归属月' }]}
              >
                <DatePicker picker="month" style={{ width: '100%' }} allowClear={false} />
              </Form.Item>
            </Col>
          </Row>
          <Row gutter={12}>
            <Col span={12}>
              <Form.Item
                name="original_value"
                label="原值"
                rules={[{ required: true, message: '请输入原值' }]}
              >
                <InputNumber min={0.01} step={0.01} precision={2} style={{ width: '100%' }} placeholder="0.00" />
              </Form.Item>
            </Col>
            <Col span={12}>
              <Form.Item
                name="salvage_rate"
                label="残值率（小数，0.05 = 5%）"
                rules={[
                  { required: true, message: '请输入残值率' },
                  {
                    validator: async (_, value: number) => {
                      if (value == null) return;
                      if (value < 0 || value > 0.5) throw new Error('残值率应在 0 ~ 0.5 之间');
                    },
                  },
                ]}
              >
                <InputNumber min={0} max={0.5} step={0.01} precision={4} style={{ width: '100%' }} placeholder="0.05" />
              </Form.Item>
            </Col>
          </Row>
          <Row gutter={12}>
            <Col span={12}>
              <Form.Item
                name="useful_life_months"
                label="折旧年限（月）"
                rules={[{ required: true, message: '请输入折旧年限' }]}
              >
                <InputNumber min={1} max={600} precision={0} style={{ width: '100%' }} placeholder="如 36" />
              </Form.Item>
            </Col>
            <Col span={12}>
              <Form.Item
                name="expense_account_code"
                label="折旧费用科目"
                rules={[{ required: true, message: '请选择折旧费用科目' }]}
              >
                <Select
                  placeholder="选择费用科目"
                  showSearch
                  optionFilterProp="label"
                  options={expenseAccountOptions}
                />
              </Form.Item>
            </Col>
          </Row>
          <Row gutter={12}>
            <Col span={12}>
              <Form.Item
                name="counter_account_code"
                label="购置凭证贷方科目（可选）"
                extra="填写后登记时生成购置凭证（借 1601 固定资产）；编辑时留空不影响已有凭证"
              >
                <Select
                  placeholder="不生成购置凭证"
                  allowClear
                  showSearch
                  optionFilterProp="label"
                  options={allAccountOptions}
                />
              </Form.Item>
            </Col>
            <Col span={12}>
              {isFundGlCode(watchCounterCode) && (
                <Form.Item
                  name="counter_fund_account_id"
                  label="贷方资金账户"
                  rules={[{ required: true, message: '贷方为资金科目时必须选择资金账户' }]}
                >
                  <Select
                    placeholder="选择资金账户"
                    showSearch
                    optionFilterProp="label"
                    options={fundOptionsFor(watchCounterCode)}
                  />
                </Form.Item>
              )}
            </Col>
          </Row>
          <Form.Item name="remark" label="备注">
            <Input.TextArea rows={2} maxLength={200} placeholder="选填" />
          </Form.Item>
          {editTarget && editTarget.depreciated_months > 0 && (
            <div style={{ color: '#8c8c8c', fontSize: 12 }}>
              该资产已有折旧记录：原值/年限/残值率/归属月不可修改（如需调整请先处置后重新登记）。
            </div>
          )}
        </Form>
      </Modal>

      {/* 计提本月折旧 */}
      <Modal
        title="计提月度折旧"
        open={depOpen}
        onCancel={() => setDepOpen(false)}
        onOk={() => void handleRunDepreciation()}
        confirmLoading={depRunning}
        okText="确认计提"
        cancelText="取消"
      >
        <Space direction="vertical" size={8}>
          <Space>
            <span style={{ color: '#666' }}>计提月份：</span>
            <DatePicker
              picker="month"
              value={depMonth}
              onChange={(d) => d && setDepMonth(d)}
              allowClear={false}
              style={{ width: 160 }}
            />
          </Space>
          <div style={{ color: '#8c8c8c', fontSize: 12 }}>
            为当月在用且未提满的资产按「原值 × (1 − 残值率) ÷ 年限月」逐台计提折旧，并汇总生成一张
            折旧凭证（借费用科目 / 贷 1602 累计折旧）；已计提月份不会重复计提，月份须处于开放状态。
          </div>
        </Space>
      </Modal>

      {/* 处置 Modal（Popconfirm 二次确认） */}
      <Modal
        title={`处置资产 - ${disposeTarget ? disposeTarget.asset_no : ''}`}
        open={disposeTarget !== null}
        onCancel={() => setDisposeTarget(null)}
        footer={[
          <Button key="cancel" onClick={() => setDisposeTarget(null)}>
            取消
          </Button>,
          <Popconfirm
            key="ok"
            title="确认处置该资产？"
            description="处置后将停止计提折旧并生成处置凭证，不可恢复"
            okText="确认处置"
            cancelText="再想想"
            okButtonProps={{ danger: true }}
            onConfirm={() => void handleDispose()}
          >
            <Button type="primary" danger loading={disposing}>
              处置
            </Button>
          </Popconfirm>,
        ]}
      >
        {disposeTarget && (
          <div style={{ marginBottom: 12, color: '#5f6b7a' }}>
            {disposeTarget.name}｜原值 {fmtMoney(disposeTarget.original_value)}｜累计折旧{' '}
            {fmtMoney(disposeTarget.accumulated_depreciation)}｜净值 {fmtMoney(disposeTarget.net_book_value)}
          </div>
        )}
        <Form<DisposalFormValues> form={disposeForm} layout="vertical">
          <Row gutter={12}>
            <Col span={12}>
              <Form.Item
                name="disposal_date"
                label="处置日期"
                rules={[{ required: true, message: '请选择处置日期' }]}
              >
                <DatePicker style={{ width: '100%' }} allowClear={false} />
              </Form.Item>
            </Col>
            <Col span={12}>
              <Form.Item
                name="disposal_income"
                label="处置收入"
                rules={[{ required: true, message: '请输入处置收入（无收入填 0）' }]}
              >
                <InputNumber min={0} step={0.01} precision={2} style={{ width: '100%' }} placeholder="0.00" />
              </Form.Item>
            </Col>
          </Row>
          {Number(watchDisposalIncome ?? 0) > 0 && (
            <Form.Item
              name="income_fund_account_id"
              label="收款资金账户"
              rules={[{ required: true, message: '处置收入大于 0 时必须选择收款资金账户' }]}
            >
              <Select
                placeholder="选择收款资金账户"
                showSearch
                optionFilterProp="label"
                options={[...fundOptionsFor('1002'), ...fundOptionsFor('1001')]}
              />
            </Form.Item>
          )}
          <Form.Item name="remark" label="备注">
            <Input.TextArea rows={2} maxLength={200} placeholder="选填，如处置方式/买受人" />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  );
};

export default FixedAssets;
