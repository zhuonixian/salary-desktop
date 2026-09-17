import { useCallback, useEffect, useMemo, useState } from 'react';
import type { ReactNode } from 'react';
import {
  Alert,
  Button,
  Card,
  DatePicker,
  Descriptions,
  Drawer,
  Empty,
  Form,
  Input,
  InputNumber,
  Modal,
  Radio,
  Select,
  Space,
  Table,
  Tag,
  Typography,
  message,
} from 'antd';
import type { ColumnsType } from 'antd/es/table';
import { PlusOutlined, ReloadOutlined } from '@ant-design/icons';
import dayjs from 'dayjs';
import type { Dayjs } from 'dayjs';
import SensitiveText from '@/components/SensitiveText';
import { useBusinessMonth } from '@/contexts/BusinessMonthContext';
import {
  confirmCountSheet,
  createCountSheet,
  getCountSheetDetail,
  getCountSheets,
  getFundAccounts,
  getFundJournal,
  updateCountSheet,
  voidCountSheet,
} from '@/api';
import { CASH_COUNT_DENOMINATIONS, CASH_COUNT_STATUS_LABEL } from '@/types';
import type {
  CashCountDenomination,
  CashCountSheet,
  CashCountSheetDetail,
  FundAccount,
} from '@/types';

const { Title, Text } = Typography;
const { TextArea } = Input;

const errText = (e: unknown): string => (e instanceof Error ? e.message : String(e));

const fmtAmount = (value: number): string =>
  (Number(value) || 0).toLocaleString('zh-CN', { minimumFractionDigits: 2, maximumFractionDigits: 2 });

// 金额容差（与后端 AMOUNT_TOLERANCE 同值）
const AMOUNT_TOLERANCE = 0.005;

const STATUS_COLOR: Record<string, string> = {
  draft: 'processing',
  confirmed: 'success',
  void: '#999999',
};

const statusTag = (status: string): ReactNode => (
  <Tag color={STATUS_COLOR[status] ?? 'default'}>{CASH_COUNT_STATUS_LABEL[status] ?? status}</Tag>
);

// 业务月份内的缺省盘点日期（与票据台账页同模式）
const monthDate = (month: Dayjs): Dayjs => month.date(Math.min(dayjs().date(), month.daysInMonth()));

interface FormValues {
  fund_account_id?: number;
  count_date?: Dayjs;
  counted_amount?: number;
  difference_reason?: string;
  remark?: string;
}

// 面额模式张数本地态：面额 → 张数（不进 Form，八档固定网格）
type Quantities = Record<string, number | undefined>;

const CashCount: React.FC = () => {
  const { month } = useBusinessMonth();

  const [sheets, setSheets] = useState<CashCountSheet[]>([]);
  const [loading, setLoading] = useState(false);
  const [accounts, setAccounts] = useState<FundAccount[]>([]);
  const [statusFilter, setStatusFilter] = useState<string | undefined>(undefined);
  const [monthFilter, setMonthFilter] = useState<Dayjs | null>(month);

  // 新建/编辑弹窗
  const [editorOpen, setEditorOpen] = useState(false);
  const [editing, setEditing] = useState<CashCountSheet | null>(null);
  const [saving, setSaving] = useState(false);
  const [form] = Form.useForm<FormValues>();
  const [mode, setMode] = useState<'quick' | 'denomination'>('quick');
  const [quantities, setQuantities] = useState<Quantities>({});
  // 参考账面余额（资金日记账同源派生，仅作录入参考；确认时点以后端快照为准）
  const [refBalance, setRefBalance] = useState<number | null>(null);

  // 作废弹窗
  const [voiding, setVoiding] = useState<CashCountSheet | null>(null);
  const [voidSaving, setVoidSaving] = useState(false);
  const [voidForm] = Form.useForm<{ reason: string }>();

  // 详情抽屉
  const [detail, setDetail] = useState<CashCountSheetDetail | null>(null);
  const [detailOpen, setDetailOpen] = useState(false);
  const [detailLoading, setDetailLoading] = useState(false);

  const cashAccounts = useMemo(
    () => accounts.filter((a) => a.account_type === 'cash' && a.is_active),
    [accounts],
  );
  const accountName = (id: number): string =>
    accounts.find((a) => a.id === id)?.name ?? `账户ID=${id}`;

  const fetchSheets = useCallback(async () => {
    setLoading(true);
    try {
      setSheets(
        await getCountSheets({
          status: statusFilter,
          belong_month: monthFilter ? monthFilter.format('YYYY-MM') : undefined,
        }),
      );
    } catch (e: unknown) {
      message.error('获取盘点单失败: ' + errText(e));
    } finally {
      setLoading(false);
    }
  }, [statusFilter, monthFilter]);

  useEffect(() => {
    fetchSheets();
  }, [fetchSheets]);

  useEffect(() => {
    // 顶部业务月份切换时同步月份筛选
    setMonthFilter(month);
  }, [month]);

  useEffect(() => {
    getFundAccounts()
      .then(setAccounts)
      .catch(() => undefined);
  }, []);

  // 选中账户后取该账户当前账面余额（日记账同源），供差异实时计算
  const fetchRefBalance = useCallback(async (accountId?: number) => {
    if (!accountId) {
      setRefBalance(null);
      return;
    }
    try {
      const journal = await getFundJournal({ fund_account_id: accountId });
      setRefBalance(journal.closing_balance);
    } catch {
      setRefBalance(null);
    }
  }, []);

  const denomTotal = useMemo(
    () =>
      CASH_COUNT_DENOMINATIONS.reduce(
        (sum, d) => sum + d * (quantities[String(d)] ?? 0),
        0,
      ),
    [quantities],
  );

  const countedAmount =
    mode === 'quick' ? Number(form.getFieldValue('counted_amount')) || 0 : denomTotal;
  // 参考差异（正=盘盈 / 负=盘亏）；参考余额取不到时不拦提交，以后端确认为准
  const refDifference =
    refBalance === null ? null : Number((countedAmount - refBalance).toFixed(2));

  const openCreate = () => {
    setEditing(null);
    setMode('quick');
    setQuantities({});
    setRefBalance(null);
    form.resetFields();
    form.setFieldsValue({ count_date: monthDate(monthFilter ?? month) });
    setEditorOpen(true);
  };

  const openEdit = async (sheet: CashCountSheet) => {
    setEditing(sheet);
    setEditorOpen(true);
    setDetailLoading(true);
    try {
      const d = await getCountSheetDetail(sheet.id);
      setMode(d.denominations.length > 0 ? 'denomination' : 'quick');
      const q: Quantities = {};
      for (const item of d.denominations) q[String(item.denomination)] = item.quantity;
      setQuantities(q);
      form.setFieldsValue({
        fund_account_id: sheet.fund_account_id,
        count_date: dayjs(sheet.count_date),
        counted_amount: sheet.counted_amount,
        difference_reason: sheet.difference_reason ?? undefined,
        remark: sheet.remark ?? undefined,
      });
      await fetchRefBalance(sheet.fund_account_id);
    } catch (e: unknown) {
      message.error('获取盘点单详情失败: ' + errText(e));
      setEditorOpen(false);
    } finally {
      setDetailLoading(false);
    }
  };

  const handleEditorOk = async () => {
    let values: FormValues;
    try {
      values = await form.validateFields();
    } catch {
      return;
    }
    if (!values.fund_account_id || !values.count_date) return;
    const counted = mode === 'quick' ? Number(values.counted_amount) || 0 : denomTotal;
    const denominations =
      mode === 'denomination'
        ? CASH_COUNT_DENOMINATIONS.filter((d) => (quantities[String(d)] ?? 0) > 0).map((d) => ({
            denomination: d,
            quantity: quantities[String(d)] ?? 0,
          }))
        : null;
    setSaving(true);
    try {
      const payload = {
        count_date: values.count_date.format('YYYY-MM-DD'),
        fund_account_id: values.fund_account_id,
        counted_amount: counted,
        difference_reason: values.difference_reason?.trim() || null,
        remark: values.remark?.trim() || null,
        denominations,
      };
      if (editing) {
        await updateCountSheet(editing.id, payload);
        message.success('盘点单已修改');
      } else {
        await createCountSheet(payload);
        message.success('盘点单已保存为草稿');
      }
      setEditorOpen(false);
      fetchSheets();
    } catch (e: unknown) {
      message.error('保存失败: ' + errText(e));
    } finally {
      setSaving(false);
    }
  };

  const handleConfirm = (sheet: CashCountSheet) => {
    const diff = sheet.difference;
    const diffText =
      Math.abs(diff) <= AMOUNT_TOLERANCE
        ? '差异为 0，确认后不生成凭证，仅留盘点记录。'
        : diff < 0
          ? `盘亏 ${fmtAmount(Math.abs(diff))}，确认后生成凭证：借 1901 待处理财产损溢 / 贷现金账户。`
          : `盘盈 ${fmtAmount(diff)}，确认后生成凭证：借现金账户 / 贷 1901 待处理财产损溢。`;
    Modal.confirm({
      title: `确认盘点单 #${sheet.id}（${sheet.count_date}）`,
      content: `账面 ${fmtAmount(sheet.book_balance)} / 实存 ${fmtAmount(sheet.counted_amount)}。${diffText}确认后盘点单不可再修改。`,
      okText: '确认',
      cancelText: '取消',
      onOk: async () => {
        try {
          await confirmCountSheet(sheet.id);
          message.success('盘点单已确认');
          fetchSheets();
        } catch (e: unknown) {
          message.error('确认失败: ' + errText(e));
        }
      },
    });
  };

  const handleVoidOk = async () => {
    if (!voiding) return;
    let values: { reason: string };
    try {
      values = await voidForm.validateFields();
    } catch {
      return;
    }
    setVoidSaving(true);
    try {
      await voidCountSheet(voiding.id, values.reason.trim());
      message.success(
        voiding.voucher_id
          ? '盘点单已作废（差异凭证走红字冲正，净影响 0）'
          : '盘点单已作废',
      );
      setVoiding(null);
      fetchSheets();
    } catch (e: unknown) {
      message.error('作废失败: ' + errText(e));
    } finally {
      setVoidSaving(false);
    }
  };

  const openDetail = async (sheet: CashCountSheet) => {
    setDetailOpen(true);
    setDetailLoading(true);
    try {
      setDetail(await getCountSheetDetail(sheet.id));
    } catch (e: unknown) {
      message.error('获取盘点单详情失败: ' + errText(e));
    } finally {
      setDetailLoading(false);
    }
  };

  const differenceText = (value: number): ReactNode => {
    if (Math.abs(value) <= AMOUNT_TOLERANCE) {
      return <Text type="secondary">账实相符</Text>;
    }
    return value < 0 ? (
      <Text type="danger" strong>
        盘亏 {fmtAmount(Math.abs(value))}
      </Text>
    ) : (
      <Text type="warning" strong>
        盘盈 {fmtAmount(value)}
      </Text>
    );
  };

  const columns: ColumnsType<CashCountSheet> = [
    { title: '盘点日期', dataIndex: 'count_date', key: 'count_date', width: 110 },
    {
      title: '盘点账户',
      dataIndex: 'fund_account_id',
      key: 'fund_account_id',
      width: 150,
      render: (v: number) => accountName(v),
    },
    {
      title: '账面余额',
      dataIndex: 'book_balance',
      key: 'book_balance',
      width: 120,
      align: 'right',
      render: (v: number) => <SensitiveText type="amount" value={fmtAmount(v)} />,
    },
    {
      title: '实存金额',
      dataIndex: 'counted_amount',
      key: 'counted_amount',
      width: 120,
      align: 'right',
      render: (v: number) => <SensitiveText type="amount" value={fmtAmount(v)} />,
    },
    {
      title: '差异',
      dataIndex: 'difference',
      key: 'difference',
      width: 130,
      render: (v: number) => differenceText(v),
    },
    {
      title: '差异原因',
      dataIndex: 'difference_reason',
      key: 'difference_reason',
      width: 180,
      ellipsis: true,
      render: (v?: string | null) => v ?? '-',
    },
    { title: '状态', dataIndex: 'status', key: 'status', width: 90, render: statusTag },
    {
      title: '凭证号',
      dataIndex: 'voucher_id',
      key: 'voucher_id',
      width: 90,
      render: (v: number | null) => (v ? <Tag color="geekblue">{v}</Tag> : '-'),
    },
    {
      title: '盘点人',
      dataIndex: 'created_by',
      key: 'created_by',
      width: 100,
      render: (v?: string | null) => v ?? '-',
    },
    {
      title: '操作',
      key: 'action',
      width: 190,
      fixed: 'right',
      render: (_, record) => (
        <Space size={0} wrap>
          <Button type="link" size="small" onClick={() => void openDetail(record)}>
            详情
          </Button>
          {record.status === 'draft' && (
            <>
              <Button type="link" size="small" onClick={() => handleConfirm(record)}>
                确认
              </Button>
              <Button type="link" size="small" onClick={() => void openEdit(record)}>
                编辑
              </Button>
              <Button type="link" size="small" danger onClick={() => {
                voidForm.resetFields();
                setVoiding(record);
              }}>
                作废
              </Button>
            </>
          )}
          {record.status === 'confirmed' && (
            <Button
              type="link"
              size="small"
              danger
              onClick={() => {
                voidForm.resetFields();
                setVoiding(record);
              }}
            >
              {record.voucher_id ? '作废（红字冲正）' : '作废'}
            </Button>
          )}
        </Space>
      ),
    },
  ];

  const denomColumns: ColumnsType<CashCountDenomination> = [
    { title: '面额', dataIndex: 'denomination', key: 'denomination', width: 100, align: 'right' },
    { title: '张数', dataIndex: 'quantity', key: 'quantity', width: 100, align: 'right' },
    {
      title: '小计',
      dataIndex: 'subtotal',
      key: 'subtotal',
      width: 120,
      align: 'right',
      render: (v: number) => <SensitiveText type="amount" value={fmtAmount(v)} />,
    },
  ];

  return (
    <Card>
      <div className="page-header" style={{ marginBottom: 8 }}>
        <Title level={4} style={{ marginTop: 0, marginBottom: 0 }}>
          现金盘点
        </Title>
      </div>
      <Alert
        type="info"
        showIcon
        style={{ marginBottom: 16 }}
        message="盘点流程：新建盘点单（限现金类账户，账面余额自动快照）→ 录实存（快速填总额或面额明细自动合计）→ 差异≠0 填原因 → 确认。盘亏生成 借 1901 待处理财产损溢 / 贷现金 凭证，盘盈反向，差异 0 免凭证。已确认盘点单不可修改，作废时差异凭证走红字冲正（净影响 0）。"
      />

      <Space wrap style={{ marginBottom: 16 }}>
        <DatePicker
          picker="month"
          value={monthFilter}
          onChange={(value) => setMonthFilter(value)}
          allowClear
          placeholder="全部月份"
          style={{ width: 140 }}
        />
        <Select
          style={{ width: 120 }}
          allowClear
          placeholder="状态"
          value={statusFilter}
          onChange={setStatusFilter}
          options={Object.entries(CASH_COUNT_STATUS_LABEL).map(([value, label]) => ({ value, label }))}
        />
        <Button type="primary" icon={<PlusOutlined />} onClick={openCreate}>
          新建盘点单
        </Button>
        <Button icon={<ReloadOutlined />} onClick={fetchSheets}>
          刷新
        </Button>
      </Space>

      <Table
        rowKey="id"
        columns={columns}
        dataSource={sheets}
        loading={loading}
        size="middle"
        scroll={{ x: 1240 }}
        pagination={{ pageSize: 20, showTotal: (t) => `共 ${t} 条` }}
      />

      {/* 新建/编辑弹窗：账户限现金类；快速模式填总额 / 面额模式八档张数实时合计 */}
      <Modal
        title={editing ? `编辑盘点单 #${editing.id}` : '新建现金盘点单'}
        open={editorOpen}
        onOk={handleEditorOk}
        confirmLoading={saving || detailLoading}
        okText={editing ? '保存' : '保存草稿'}
        onCancel={() => setEditorOpen(false)}
        width={640}
        destroyOnHidden
      >
        <Form form={form} layout="vertical">
          <Space size="middle" style={{ display: 'flex' }}>
            <Form.Item
              name="fund_account_id"
              label="盘点账户"
              rules={[{ required: true, message: '请选择现金账户' }]}
              extra="仅现金类账户可盘点"
            >
              <Select
                style={{ width: 220 }}
                showSearch
                optionFilterProp="label"
                placeholder="选择现金类账户"
                options={cashAccounts.map((a) => ({ value: a.id, label: `${a.name}（${a.account_code}）` }))}
                onChange={(value?: number) => void fetchRefBalance(value)}
              />
            </Form.Item>
            <Form.Item
              name="count_date"
              label="盘点日期"
              rules={[{ required: true, message: '请选择盘点日期' }]}
              extra="盘点月须未月结"
            >
              <DatePicker style={{ width: 140 }} />
            </Form.Item>
          </Space>

          <Form.Item label="实存录入方式">
            <Radio.Group
              value={mode}
              onChange={(e) => {
                setMode(e.target.value as 'quick' | 'denomination');
                if (e.target.value === 'quick') setQuantities({});
              }}
              optionType="button"
              buttonStyle="solid"
              options={[
                { value: 'quick', label: '快速模式（填总额）' },
                { value: 'denomination', label: '面额模式（按张数）' },
              ]}
            />
          </Form.Item>

          {mode === 'quick' ? (
            <Form.Item
              name="counted_amount"
              label="实存总额"
              rules={[
                { required: true, message: '请输入实存总额' },
                { type: 'number', min: 0, message: '实存总额不能为负' },
              ]}
              extra="清点后的现金总额（无现金填 0）"
            >
              <InputNumber min={0} step={0.01} style={{ width: 200 }} precision={2} />
            </Form.Item>
          ) : (
            <>
              <Table
                rowKey="denomination"
                size="small"
                pagination={false}
                style={{ marginBottom: 12 }}
                dataSource={CASH_COUNT_DENOMINATIONS.map((d) => ({ denomination: d }))}
                columns={[
                  {
                    title: '面额',
                    dataIndex: 'denomination',
                    key: 'denomination',
                    width: 100,
                    align: 'right',
                    render: (v: number) => `¥ ${v}`,
                  },
                  {
                    title: '张数',
                    key: 'quantity',
                    width: 160,
                    render: (_, record) => (
                      <InputNumber
                        min={0}
                        precision={0}
                        style={{ width: 120 }}
                        value={quantities[String(record.denomination)] ?? 0}
                        onChange={(value) =>
                          setQuantities((prev) => ({
                            ...prev,
                            [String(record.denomination)]: value ?? 0,
                          }))
                        }
                      />
                    ),
                  },
                  {
                    title: '小计',
                    key: 'subtotal',
                    align: 'right',
                    render: (_, record) => (
                      <SensitiveText
                        type="amount"
                        value={fmtAmount(record.denomination * (quantities[String(record.denomination)] ?? 0))}
                      />
                    ),
                  },
                ]}
              />
              <Alert
                type="info"
                showIcon
                style={{ marginBottom: 12 }}
                message={
                  <>
                    面额合计：
                    <SensitiveText type="amount" value={fmtAmount(denomTotal)} />
                    （实存金额自动取面额合计）
                  </>
                }
              />
            </>
          )}

          {/* 差异醒目提示：参考余额取自资金日记账同源派生，确认时点以后端快照为准 */}
          {refBalance !== null && (
            <Alert
              type={refDifference !== null && Math.abs(refDifference) <= AMOUNT_TOLERANCE ? 'success' : 'warning'}
              showIcon
              style={{ marginBottom: 12 }}
              message={
                refDifference !== null && Math.abs(refDifference) <= AMOUNT_TOLERANCE ? (
                  '账实相符：实存与参考账面余额一致'
                ) : (
                  <>
                    参考账面余额 <SensitiveText type="amount" value={fmtAmount(refBalance ?? 0)} />
                    ，差异{' '}
                    <Text type={refDifference !== null && refDifference < 0 ? 'danger' : 'warning'} strong>
                      {refDifference !== null && refDifference < 0
                        ? `盘亏 ${fmtAmount(Math.abs(refDifference))}`
                        : `盘盈 ${fmtAmount(refDifference ?? 0)}`}
                    </Text>
                    （差异≠0 时确认将生成盘盈亏凭证）
                  </>
                )
              }
            />
          )}

          <Form.Item
            name="difference_reason"
            label="差异原因"
            dependencies={['counted_amount']}
            rules={[
              ({ getFieldValue }) => ({
                validator: (_, value: string) => {
                  const counted =
                    mode === 'quick'
                      ? Number(getFieldValue('counted_amount')) || 0
                      : denomTotal;
                  const diff = refBalance === null ? null : Number((counted - refBalance).toFixed(2));
                  if (diff !== null && Math.abs(diff) > AMOUNT_TOLERANCE && !value?.trim()) {
                    return Promise.reject(new Error('差异不为 0 时必须填写差异原因'));
                  }
                  return Promise.resolve();
                },
              }),
            ]}
          >
            <TextArea rows={2} maxLength={200} placeholder="差异≠0 时必填（如 找零垫付未入账）" />
          </Form.Item>
          <Form.Item name="remark" label="备注">
            <TextArea rows={2} maxLength={200} placeholder="备注（可选）" />
          </Form.Item>
        </Form>
      </Modal>

      {/* 作废弹窗：存在差异凭证的确认单必须填原因（红字冲正留痕） */}
      <Modal
        title={`作废盘点单 #${voiding?.id ?? ''}`}
        open={voiding !== null}
        onOk={handleVoidOk}
        confirmLoading={voidSaving}
        okText="作废"
        okButtonProps={{ danger: true }}
        onCancel={() => setVoiding(null)}
        destroyOnHidden
      >
        {voiding && (
          <>
            <Alert
              type={voiding.voucher_id ? 'warning' : 'info'}
              showIcon
              style={{ marginBottom: 12 }}
              message={
                voiding.voucher_id
                  ? '该盘点单已确认且存在差异凭证，作废将生成借贷互换的红字冲正凭证（原凭证保留，净影响 0），原因必填。'
                  : '草稿或无差异凭证的确认单直接作废，不产生凭证。'
              }
            />
            <Form form={voidForm} layout="vertical">
              <Form.Item
                name="reason"
                label="作废原因"
                rules={
                  voiding.voucher_id
                    ? [{ required: true, message: '存在差异凭证时作废必须填写原因' }]
                    : undefined
                }
              >
                <TextArea rows={3} maxLength={200} placeholder={voiding.voucher_id ? '请填写冲正原因（必填）' : '作废原因（可选）'} />
              </Form.Item>
            </Form>
          </>
        )}
      </Modal>

      {/* 详情抽屉：盘点信息 + 面额明细（含差异凭证号） */}
      <Drawer
        title={detail ? `盘点单详情 #${detail.sheet.id}` : '盘点单详情'}
        open={detailOpen}
        width={640}
        onClose={() => setDetailOpen(false)}
        destroyOnHidden
      >
        {detail === null ? (
          <Empty description={detailLoading ? '加载中…' : '暂无数据'} />
        ) : (
          <>
            <Descriptions
              size="small"
              column={2}
              bordered
              items={[
                { key: 'date', label: '盘点日期', children: detail.sheet.count_date },
                { key: 'status', label: '状态', children: statusTag(detail.sheet.status) },
                { key: 'account', label: '盘点账户', children: accountName(detail.sheet.fund_account_id) },
                {
                  key: 'voucher',
                  label: '差异凭证号',
                  children: detail.sheet.voucher_id ? (
                    <Tag color="geekblue">{detail.sheet.voucher_id}</Tag>
                  ) : (
                    '-'
                  ),
                },
                {
                  key: 'book',
                  label: '账面余额',
                  children: <SensitiveText type="amount" value={fmtAmount(detail.sheet.book_balance)} />,
                },
                {
                  key: 'counted',
                  label: '实存金额',
                  children: <SensitiveText type="amount" value={fmtAmount(detail.sheet.counted_amount)} />,
                },
                {
                  key: 'difference',
                  label: '差异',
                  children: differenceText(detail.sheet.difference),
                },
                {
                  key: 'reason',
                  label: '差异原因',
                  children: detail.sheet.difference_reason ?? '-',
                },
                {
                  key: 'creator',
                  label: '盘点人',
                  span: 2,
                  children: detail.sheet.created_by
                    ? `${detail.sheet.created_by} · ${dayjs(detail.sheet.created_at).format('YYYY-MM-DD HH:mm')}`
                    : '-',
                },
                { key: 'remark', label: '备注', children: detail.sheet.remark ?? '-', span: 2 },
              ]}
            />

            <Title level={5} style={{ marginTop: 24 }}>
              面额明细
            </Title>
            {detail.denominations.length === 0 ? (
              <Text type="secondary">快速模式（无面额明细）</Text>
            ) : (
              <>
                <Table
                  rowKey="denomination"
                  size="small"
                  pagination={false}
                  columns={denomColumns}
                  dataSource={detail.denominations}
                />
                <div style={{ marginTop: 8, textAlign: 'right' }}>
                  <Text strong>
                    合计：
                    <SensitiveText
                      type="amount"
                      value={fmtAmount(detail.denominations.reduce((s, d) => s + d.subtotal, 0))}
                    />
                  </Text>
                </div>
              </>
            )}
          </>
        )}
      </Drawer>
    </Card>
  );
};

export default CashCount;
