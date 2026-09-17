import { useCallback, useEffect, useState } from 'react';
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
  Select,
  Space,
  Table,
  Tabs,
  Tag,
  Timeline,
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
  confirmCollection,
  discountInstrument,
  endorseInstrument,
  getBusinessPartners,
  getFundAccounts,
  getGlAccounts,
  getInstrumentDetail,
  getNegotiableInstruments,
  getOperatorProfiles,
  listApprovalEvents,
  registerInstrument,
  reverseInstrumentFlow,
  settleIssuedInstrument,
  startCollection,
  voidInstrument,
} from '@/api';
import {
  APPROVAL_ACTION_LABEL,
  INSTRUMENT_ACTION_LABEL,
  INSTRUMENT_DIRECTION_LABEL,
  INSTRUMENT_STATUS_LABEL,
  INSTRUMENT_TYPE_LABEL,
} from '@/types';
import type {
  ApprovalEvent,
  BusinessPartner,
  FundAccount,
  GlAccount,
  InstrumentDetail,
  InstrumentEndorsement,
  NegotiableInstrument,
  OperatorProfile,
} from '@/types';

const { Title, Text } = Typography;
const { TextArea } = Input;

const errText = (e: unknown): string => (e instanceof Error ? e.message : String(e));

const fmtAmount = (value: number): string =>
  (Number(value) || 0).toLocaleString('zh-CN', { minimumFractionDigits: 2, maximumFractionDigits: 2 });

// 状态颜色（与后端 notes.rs status_label 中文一致）
const STATUS_COLOR: Record<string, string> = {
  holding: 'processing',
  endorsed_out: 'purple',
  discounted: 'geekblue',
  collecting: 'gold',
  collected: 'success',
  issued_outstanding: 'warning',
  paid: 'success',
  void: '#999999',
};

const STATUS_OPTIONS = Object.entries(INSTRUMENT_STATUS_LABEL).map(([value, label]) => ({
  value,
  label,
}));

const TYPE_OPTIONS = Object.entries(INSTRUMENT_TYPE_LABEL).map(([value, label]) => ({
  value,
  label,
}));

// 方向标签（全部 Tab 表格列展示用）
const directionTag = (direction: string): ReactNode => (
  <Tag color={direction === 'received' ? 'blue' : 'orange'}>
    {INSTRUMENT_DIRECTION_LABEL[direction] ?? direction}
  </Tag>
);

const statusTag = (status: string): ReactNode => (
  <Tag color={STATUS_COLOR[status] ?? 'default'}>{INSTRUMENT_STATUS_LABEL[status] ?? status}</Tag>
);

// 可红字冲正的流转后状态（撤销最后一次流转；作废为纯终态不可冲正）
const REVERSABLE_STATUSES = ['endorsed_out', 'discounted', 'collected', 'paid'];

// 冲正恢复态说明（与后端 reverse_instrument_flow 恢复口径一致）
const reverseTargetLabel = (inst: NegotiableInstrument): string => {
  if (inst.instrument_type === 'check') return '已作废（登记即终态的纠错，同号可重新登记）';
  if (inst.status === 'paid') return '已开出未兑付（可重新兑付或作废）';
  return '持有（可改道背书/贴现/托收或作废）';
};

interface SharedOptions {
  accounts: FundAccount[];
  partners: BusinessPartner[];
  glOptions: { value: string; label: string }[];
  operators: OperatorProfile[];
}

// ==================== 日期/账户公共小件 ====================

// 业务月份内的缺省操作日期（与收付款单页同模式，操作月须未月结由后端强校验）
const monthDate = (month: Dayjs): Dayjs => month.date(Math.min(dayjs().date(), month.daysInMonth()));

const accountSelectOptions = (accounts: FundAccount[]) =>
  accounts
    .filter((a) => a.is_active)
    .map((a) => ({ value: a.id, label: `${a.name}（${a.account_code}）` }));

const accountName = (accounts: FundAccount[], id: number | null): string =>
  id === null ? '-' : accounts.find((a) => a.id === id)?.name ?? `账户ID=${id}`;

interface RegisterFormValues {
  instrument_type: string;
  direction: string;
  instrument_no: string;
  face_amount?: number;
  issue_date: Dayjs;
  due_date: Dayjs;
  drawer?: string;
  acceptor?: string;
  payee?: string;
  partner_id?: number;
  fund_account_id?: number;
  counter_account_code?: string;
  remark?: string;
}

interface EndorseFormValues {
  endorsee: string;
  endorse_date: Dayjs;
  purpose?: string;
  counter_account_code?: string;
}

interface DiscountFormValues {
  discount_date: Dayjs;
  proceeds?: number;
  fund_account_id?: number;
}

interface OperateFormValues {
  operate_date: Dayjs;
  fund_account_id?: number;
}

interface ReverseFormValues {
  reverse_date: Dayjs;
  reason: string;
}

// ==================== 票据 Tab ====================

const InstrumentTab: React.FC<{ direction?: string; shared: SharedOptions }> = ({
  direction,
  shared,
}) => {
  const { month } = useBusinessMonth();

  const [instruments, setInstruments] = useState<NegotiableInstrument[]>([]);
  const [loading, setLoading] = useState(false);
  const [statusFilter, setStatusFilter] = useState<string | undefined>(undefined);
  const [typeFilter, setTypeFilter] = useState<string | undefined>(undefined);
  const [keyword, setKeyword] = useState('');

  const [registerOpen, setRegisterOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const [registerForm] = Form.useForm<RegisterFormValues>();
  const watchRegType = Form.useWatch('instrument_type', registerForm);
  const watchRegDir = Form.useWatch('direction', registerForm);
  const regIsCheck = watchRegType === 'check';
  const regIsAcceptance = watchRegType === 'bank_acceptance' || watchRegType === 'commercial_acceptance';

  const [endorseInst, setEndorseInst] = useState<NegotiableInstrument | null>(null);
  const [endorseSaving, setEndorseSaving] = useState(false);
  const [endorseForm] = Form.useForm<EndorseFormValues>();

  const [discountInst, setDiscountInst] = useState<NegotiableInstrument | null>(null);
  const [discountSaving, setDiscountSaving] = useState(false);
  const [discountForm] = Form.useForm<DiscountFormValues>();
  const watchProceeds = Form.useWatch('proceeds', discountForm);
  const discountFee = Math.max(
    0,
    (discountInst?.face_amount ?? 0) - (Number(watchProceeds) || 0),
  );

  const [collectInst, setCollectInst] = useState<NegotiableInstrument | null>(null);
  const [collectSaving, setCollectSaving] = useState(false);
  const [collectForm] = Form.useForm<OperateFormValues>();

  const [confirmInst, setConfirmInst] = useState<NegotiableInstrument | null>(null);
  const [confirmSaving, setConfirmSaving] = useState(false);
  const [confirmForm] = Form.useForm<OperateFormValues>();

  const [settleInst, setSettleInst] = useState<NegotiableInstrument | null>(null);
  const [settleSaving, setSettleSaving] = useState(false);
  const [settleForm] = Form.useForm<OperateFormValues>();

  const [voidInst, setVoidInst] = useState<NegotiableInstrument | null>(null);
  const [voidSaving, setVoidSaving] = useState(false);
  const [voidForm] = Form.useForm<{ reason: string }>();

  const [reverseInst, setReverseInst] = useState<NegotiableInstrument | null>(null);
  const [reverseSaving, setReverseSaving] = useState(false);
  const [reverseForm] = Form.useForm<ReverseFormValues>();

  const [detail, setDetail] = useState<InstrumentDetail | null>(null);
  const [detailOpen, setDetailOpen] = useState(false);
  const [detailLoading, setDetailLoading] = useState(false);
  const [events, setEvents] = useState<ApprovalEvent[]>([]);

  const fetchInstruments = useCallback(async () => {
    setLoading(true);
    try {
      setInstruments(
        await getNegotiableInstruments({
          direction,
          instrument_type: typeFilter,
          status: statusFilter,
          belong_month: month.format('YYYY-MM'),
          keyword: keyword.trim() || undefined,
        }),
      );
    } catch (e: unknown) {
      message.error('获取票据失败: ' + errText(e));
    } finally {
      setLoading(false);
    }
  }, [direction, typeFilter, statusFilter, month, keyword]);

  useEffect(() => {
    fetchInstruments();
  }, [fetchInstruments]);

  const fetchDetail = useCallback(async (id: number) => {
    setDetailLoading(true);
    try {
      setDetail(await getInstrumentDetail(id));
      setEvents(await listApprovalEvents('negotiable_instrument', id));
    } catch (e: unknown) {
      message.error('获取票据详情失败: ' + errText(e));
    } finally {
      setDetailLoading(false);
    }
  }, []);

  const openDetail = async (inst: NegotiableInstrument) => {
    setDetailOpen(true);
    await fetchDetail(inst.id);
  };

  // 流转成功后的统一收尾：刷新列表；详情抽屉开着且是同一票据时同步刷新
  const afterTransition = async (instId?: number) => {
    await fetchInstruments();
    if (detailOpen && instId) await fetchDetail(instId);
  };

  const runAction = async (fn: () => Promise<unknown>, success: string, instId?: number) => {
    try {
      await fn();
      message.success(success);
      await afterTransition(instId);
      return true;
    } catch (e: unknown) {
      message.error('操作失败: ' + errText(e));
      return false;
    }
  };

  // ---------- 登记弹窗 ----------

  const openRegister = () => {
    registerForm.resetFields();
    registerForm.setFieldsValue({
      instrument_type: 'bank_acceptance',
      direction: direction ?? 'received',
      issue_date: monthDate(month),
      due_date: monthDate(month).add(3, 'month'),
    });
    setRegisterOpen(true);
  };

  const handleRegisterOk = async () => {
    let values: RegisterFormValues;
    try {
      values = await registerForm.validateFields();
    } catch {
      return;
    }
    setSaving(true);
    try {
      const isCheck = values.instrument_type === 'check';
      const isAcceptance =
        values.instrument_type === 'bank_acceptance' ||
        values.instrument_type === 'commercial_acceptance';
      await registerInstrument({
        instrument_type: values.instrument_type,
        direction: values.direction,
        instrument_no: values.instrument_no.trim(),
        face_amount: values.face_amount ?? 0,
        issue_date: values.issue_date.format('YYYY-MM-DD'),
        due_date: values.due_date.format('YYYY-MM-DD'),
        drawer: values.drawer?.trim() || null,
        acceptor: isAcceptance ? values.acceptor?.trim() || null : null,
        payee: values.payee?.trim() || null,
        partner_id: values.partner_id ?? null,
        // 支票登记即结算必选账户；承兑类可选（作后续托收/贴现/兑付缺省）
        fund_account_id: values.fund_account_id ?? null,
        counter_account_code: values.counter_account_code ?? null,
        remark: values.remark?.trim() || null,
      });
      message.success(
        isCheck
          ? '票据已登记（支票登记即结算）'
          : values.direction === 'received'
            ? '承兑已登记（持有）'
            : '承兑已登记（已开出未兑付）',
      );
      setRegisterOpen(false);
      fetchInstruments();
    } catch (e: unknown) {
      message.error('登记失败: ' + errText(e));
    } finally {
      setSaving(false);
    }
  };

  // ---------- 背书 / 贴现 / 托收 / 到账 / 兑付 ----------

  const openEndorse = (inst: NegotiableInstrument) => {
    endorseForm.resetFields();
    endorseForm.setFieldsValue({ endorse_date: monthDate(month) });
    setEndorseInst(inst);
  };

  const handleEndorseOk = async () => {
    if (!endorseInst) return;
    let values: EndorseFormValues;
    try {
      values = await endorseForm.validateFields();
    } catch {
      return;
    }
    setEndorseSaving(true);
    try {
      await endorseInstrument({
        instrument_id: endorseInst.id,
        endorsee: values.endorsee.trim(),
        endorse_date: values.endorse_date.format('YYYY-MM-DD'),
        // 本期约定全额背书：金额固定等于票面
        amount: endorseInst.face_amount,
        purpose: values.purpose?.trim() || null,
        counter_account_code: values.counter_account_code ?? null,
      });
      message.success('背书转出成功');
      setEndorseInst(null);
      await afterTransition(endorseInst.id);
    } catch (e: unknown) {
      message.error('背书失败: ' + errText(e));
    } finally {
      setEndorseSaving(false);
    }
  };

  const openDiscount = (inst: NegotiableInstrument) => {
    discountForm.resetFields();
    discountForm.setFieldsValue({
      discount_date: monthDate(month),
      proceeds: inst.face_amount,
      fund_account_id: inst.fund_account_id ?? undefined,
    });
    setDiscountInst(inst);
  };

  const handleDiscountOk = async () => {
    if (!discountInst) return;
    let values: DiscountFormValues;
    try {
      values = await discountForm.validateFields();
    } catch {
      return;
    }
    setDiscountSaving(true);
    try {
      await discountInstrument({
        instrument_id: discountInst.id,
        discount_date: values.discount_date.format('YYYY-MM-DD'),
        proceeds: values.proceeds ?? 0,
        fund_account_id: values.fund_account_id ?? null,
      });
      message.success('贴现成功');
      setDiscountInst(null);
      await afterTransition(discountInst.id);
    } catch (e: unknown) {
      message.error('贴现失败: ' + errText(e));
    } finally {
      setDiscountSaving(false);
    }
  };

  const openCollect = (inst: NegotiableInstrument) => {
    collectForm.resetFields();
    collectForm.setFieldsValue({ operate_date: monthDate(month) });
    setCollectInst(inst);
  };

  const handleCollectOk = async () => {
    if (!collectInst) return;
    let values: OperateFormValues;
    try {
      values = await collectForm.validateFields();
    } catch {
      return;
    }
    setCollectSaving(true);
    const ok = await runAction(
      () => startCollection(collectInst.id, values.operate_date.format('YYYY-MM-DD')),
      '托收已发起（在途不记账）',
      collectInst.id,
    );
    setCollectSaving(false);
    if (ok) setCollectInst(null);
  };

  const openConfirm = (inst: NegotiableInstrument) => {
    confirmForm.resetFields();
    confirmForm.setFieldsValue({
      operate_date: monthDate(month),
      fund_account_id: inst.fund_account_id ?? undefined,
    });
    setConfirmInst(inst);
  };

  const handleConfirmOk = async () => {
    if (!confirmInst) return;
    let values: OperateFormValues;
    try {
      values = await confirmForm.validateFields();
    } catch {
      return;
    }
    setConfirmSaving(true);
    const ok = await runAction(
      () =>
        confirmCollection({
          instrument_id: confirmInst.id,
          received_date: values.operate_date.format('YYYY-MM-DD'),
          fund_account_id: values.fund_account_id ?? null,
        }),
      '到账确认成功',
      confirmInst.id,
    );
    setConfirmSaving(false);
    if (ok) setConfirmInst(null);
  };

  const openSettle = (inst: NegotiableInstrument) => {
    settleForm.resetFields();
    settleForm.setFieldsValue({
      operate_date: monthDate(month),
      fund_account_id: inst.fund_account_id ?? undefined,
    });
    setSettleInst(inst);
  };

  const handleSettleOk = async () => {
    if (!settleInst) return;
    let values: OperateFormValues;
    try {
      values = await settleForm.validateFields();
    } catch {
      return;
    }
    setSettleSaving(true);
    const ok = await runAction(
      () =>
        settleIssuedInstrument({
          instrument_id: settleInst.id,
          settle_date: values.operate_date.format('YYYY-MM-DD'),
          fund_account_id: values.fund_account_id ?? null,
        }),
      '兑付成功',
      settleInst.id,
    );
    setSettleSaving(false);
    if (ok) setSettleInst(null);
  };

  // ---------- 作废 / 冲正 ----------

  const openVoid = (inst: NegotiableInstrument) => {
    voidForm.resetFields();
    setVoidInst(inst);
  };

  const handleVoidOk = async () => {
    if (!voidInst) return;
    let values: { reason: string };
    try {
      values = await voidForm.validateFields();
    } catch {
      return;
    }
    setVoidSaving(true);
    const ok = await runAction(
      () => voidInstrument(voidInst.id, values.reason.trim()),
      '票据已作废（登记凭证同步作废）',
      voidInst.id,
    );
    setVoidSaving(false);
    if (ok) setVoidInst(null);
  };

  const openReverse = (inst: NegotiableInstrument) => {
    reverseForm.resetFields();
    reverseForm.setFieldsValue({ reverse_date: monthDate(month) });
    setReverseInst(inst);
  };

  const handleReverseOk = async () => {
    if (!reverseInst) return;
    let values: ReverseFormValues;
    try {
      values = await reverseForm.validateFields();
    } catch {
      return;
    }
    setReverseSaving(true);
    const ok = await runAction(
      () =>
        reverseInstrumentFlow({
          instrument_id: reverseInst.id,
          reverse_date: values.reverse_date.format('YYYY-MM-DD'),
          reason: values.reason.trim(),
        }),
      '红字冲正成功（原凭证保留，反向凭证并存，净影响 0）',
      reverseInst.id,
    );
    setReverseSaving(false);
    if (ok) setReverseInst(null);
  };

  // ---------- 表格 ----------

  const columns: ColumnsType<NegotiableInstrument> = [
    { title: '票据号码', dataIndex: 'instrument_no', key: 'instrument_no', width: 150 },
    {
      title: '类型',
      dataIndex: 'instrument_type',
      key: 'instrument_type',
      width: 120,
      render: (value: string) => (
        <Tag color="blue">{INSTRUMENT_TYPE_LABEL[value] ?? value}</Tag>
      ),
    },
    ...(direction
      ? []
      : [
          {
            title: '方向',
            dataIndex: 'direction',
            key: 'direction',
            width: 80,
            render: (value: string) => directionTag(value),
          } as ColumnsType<NegotiableInstrument>[number],
        ]),
    {
      title: '票面金额',
      dataIndex: 'face_amount',
      key: 'face_amount',
      width: 130,
      align: 'right',
      render: (value: number) => <SensitiveText type="amount" value={fmtAmount(value)} />,
    },
    { title: '出票日', dataIndex: 'issue_date', key: 'issue_date', width: 100 },
    { title: '到期日', dataIndex: 'due_date', key: 'due_date', width: 100 },
    {
      title: '出票人',
      dataIndex: 'drawer',
      key: 'drawer',
      width: 130,
      ellipsis: true,
      render: (v?: string | null) => v ?? '-',
    },
    {
      title: '承兑人/收款人',
      key: 'acceptor_payee',
      width: 150,
      ellipsis: true,
      render: (_, record) =>
        [record.acceptor, record.payee].filter(Boolean).join(' / ') || '-',
    },
    {
      title: '挂接账户',
      dataIndex: 'fund_account_id',
      key: 'fund_account_id',
      width: 130,
      ellipsis: true,
      render: (v: number | null) => accountName(shared.accounts, v),
    },
    { title: '状态', dataIndex: 'status', key: 'status', width: 110, render: statusTag },
    {
      title: '操作',
      key: 'action',
      width: 230,
      fixed: 'right',
      render: (_, record) => (
        <Space size={0} wrap>
          <Button type="link" size="small" onClick={() => void openDetail(record)}>
            详情
          </Button>
          {record.status === 'holding' && (
            <>
              <Button type="link" size="small" onClick={() => openEndorse(record)}>
                背书
              </Button>
              <Button type="link" size="small" onClick={() => openDiscount(record)}>
                贴现
              </Button>
              <Button type="link" size="small" onClick={() => openCollect(record)}>
                托收
              </Button>
              <Button type="link" size="small" danger onClick={() => openVoid(record)}>
                作废
              </Button>
            </>
          )}
          {record.status === 'collecting' && (
            <Button type="link" size="small" onClick={() => openConfirm(record)}>
              到账确认
            </Button>
          )}
          {record.status === 'issued_outstanding' && (
            <>
              <Button type="link" size="small" onClick={() => openSettle(record)}>
                兑付
              </Button>
              <Button type="link" size="small" danger onClick={() => openVoid(record)}>
                作废
              </Button>
            </>
          )}
          {REVERSABLE_STATUSES.includes(record.status) && (
            <Button type="link" size="small" danger onClick={() => openReverse(record)}>
              冲正
            </Button>
          )}
        </Space>
      ),
    },
  ];

  const accOptions = accountSelectOptions(shared.accounts);

  return (
    <>
      <Space wrap style={{ marginBottom: 16 }}>
        <Select
          style={{ width: 140 }}
          allowClear
          placeholder="状态"
          value={statusFilter}
          onChange={setStatusFilter}
          options={STATUS_OPTIONS}
        />
        <Select
          style={{ width: 150 }}
          allowClear
          placeholder="票据类型"
          value={typeFilter}
          onChange={setTypeFilter}
          options={TYPE_OPTIONS}
        />
        <Input.Search
          style={{ width: 240 }}
          allowClear
          placeholder="搜索票据号/出票人/收款人"
          value={keyword}
          onChange={(e) => setKeyword(e.target.value)}
          onSearch={fetchInstruments}
        />
        <Button type="primary" icon={<PlusOutlined />} onClick={openRegister}>
          登记票据
        </Button>
        <Button icon={<ReloadOutlined />} onClick={fetchInstruments}>
          刷新
        </Button>
      </Space>

      <Table
        rowKey="id"
        columns={columns}
        dataSource={instruments}
        loading={loading}
        size="middle"
        scroll={{ x: 1360 }}
        pagination={{ pageSize: 20, showTotal: (t) => `共 ${t} 条` }}
      />

      {/* 登记弹窗：按类型+方向动态约束（承兑类收方向免账户、支票必选账户） */}
      <Modal
        title="登记票据"
        open={registerOpen}
        onOk={handleRegisterOk}
        confirmLoading={saving}
        okText="登记"
        onCancel={() => setRegisterOpen(false)}
        width={680}
        destroyOnHidden
      >
        <Form form={registerForm} layout="vertical">
          <Space size="middle" style={{ display: 'flex' }}>
            <Form.Item
              name="instrument_type"
              label="票据类型"
              rules={[{ required: true, message: '请选择票据类型' }]}
              extra="支票登记即结算（收到→已到账 / 开出→已兑付）"
            >
              <Select
                style={{ width: 160 }}
                options={TYPE_OPTIONS}
                onChange={() => {
                  // 切类型/方向时清空动态字段；支票到期日默认=出票日
                  registerForm.setFieldsValue({ fund_account_id: undefined, acceptor: undefined });
                  if (registerForm.getFieldValue('instrument_type') === 'check') {
                    const issue = registerForm.getFieldValue('issue_date') as Dayjs | undefined;
                    if (issue) registerForm.setFieldsValue({ due_date: issue });
                  }
                }}
              />
            </Form.Item>
            <Form.Item
              name="direction"
              label="方向"
              rules={[{ required: true, message: '请选择方向' }]}
              extra="收到＝我方持有票据；开出＝我方对外承兑/付款"
            >
              <Select
                style={{ width: 120 }}
                options={Object.entries(INSTRUMENT_DIRECTION_LABEL).map(([value, label]) => ({
                  value,
                  label,
                }))}
              />
            </Form.Item>
            <Form.Item
              name="instrument_no"
              label="票据号码"
              rules={[{ required: true, message: '请输入票据号码' }]}
            >
              <Input style={{ width: 180 }} maxLength={50} placeholder="同类型同号唯一" />
            </Form.Item>
          </Space>
          <Space size="middle" style={{ display: 'flex' }}>
            <Form.Item
              name="face_amount"
              label="票面金额"
              rules={[
                { required: true, message: '请输入票面金额' },
                { type: 'number', min: 0.01, message: '票面金额必须为正数' },
              ]}
            >
              <InputNumber min={0.01} step={0.01} style={{ width: 160 }} precision={2} />
            </Form.Item>
            <Form.Item
              name="issue_date"
              label="出票日"
              rules={[{ required: true, message: '请选择出票日' }]}
              extra="登记月＝出票日所在月，月结后禁止补录"
            >
              <DatePicker
                style={{ width: 140 }}
                onChange={(value) => {
                  // 支票到期日默认随出票日
                  if (value && registerForm.getFieldValue('instrument_type') === 'check') {
                    registerForm.setFieldsValue({ due_date: value });
                  }
                }}
              />
            </Form.Item>
            <Form.Item
              name="due_date"
              label="到期日"
              rules={[
                { required: true, message: '请选择到期日' },
                ({ getFieldValue }) => ({
                  validator: (_, value: Dayjs | undefined) =>
                    value && getFieldValue('issue_date') && value.isBefore(getFieldValue('issue_date'), 'day')
                      ? Promise.reject(new Error('到期日不能早于出票日'))
                      : Promise.resolve(),
                }),
              ]}
            >
              <DatePicker style={{ width: 140 }} />
            </Form.Item>
          </Space>
          <Space size="middle" style={{ display: 'flex' }}>
            <Form.Item name="drawer" label="出票人">
              <Input style={{ width: 160 }} maxLength={50} />
            </Form.Item>
            {regIsAcceptance && (
              <Form.Item name="acceptor" label="承兑人">
                <Input style={{ width: 160 }} maxLength={50} placeholder="承兑汇票必填" />
              </Form.Item>
            )}
            <Form.Item name="payee" label="收款人">
              <Input style={{ width: 160 }} maxLength={50} />
            </Form.Item>
          </Space>
          <Space size="middle" style={{ display: 'flex' }}>
            <Form.Item
              name="fund_account_id"
              label={
                regIsCheck
                  ? watchRegDir === 'issued'
                    ? '出账资金账户'
                    : '入账资金账户'
                  : '挂接资金账户'
              }
              rules={regIsCheck ? [{ required: true, message: '支票登记即结算，必须选择资金账户' }] : undefined}
              extra={
                regIsCheck
                  ? undefined
                  : '可选；作为后续托收/贴现/兑付的缺省账户'
              }
            >
              <Select style={{ width: 240 }} allowClear showSearch optionFilterProp="label" options={accOptions} />
            </Form.Item>
            <Form.Item
              name="partner_id"
              label="关联往来单位"
            >
              <Select
                style={{ width: 220 }}
                allowClear
                showSearch
                optionFilterProp="label"
                options={shared.partners.map((p) => ({ value: p.id, label: p.name }))}
              />
            </Form.Item>
            <Form.Item
              name="counter_account_code"
              label="对方科目"
              extra="缺省按类型与方向推导（收到 1122 应收账款 / 开出 2202 应付账款）"
            >
              <Select
                style={{ width: 220 }}
                allowClear
                showSearch
                optionFilterProp="label"
                placeholder="从科目表选择"
                options={shared.glOptions}
              />
            </Form.Item>
          </Space>
          <Form.Item name="remark" label="备注">
            <TextArea rows={2} placeholder="备注（可选）" maxLength={200} />
          </Form.Item>
        </Form>
      </Modal>

      {/* 背书弹窗：本期全额背书，金额自动等于票面 */}
      <Modal
        title={`背书转出：${endorseInst?.instrument_no ?? ''}`}
        open={endorseInst !== null}
        onOk={handleEndorseOk}
        confirmLoading={endorseSaving}
        okText="背书转出"
        onCancel={() => setEndorseInst(null)}
        destroyOnHidden
      >
        {endorseInst && (
          <>
            <Alert
              type="info"
              showIcon
              style={{ marginBottom: 12 }}
              message={`本期仅支持全额背书：背书金额固定等于票面 ${fmtAmount(endorseInst.face_amount)}，背书后票据转为「已背书转出」。`}
            />
            <Form form={endorseForm} layout="vertical">
              <Space size="middle" style={{ display: 'flex' }}>
                <Form.Item
                  name="endorsee"
                  label="被背书人"
                  rules={[{ required: true, message: '请输入被背书人' }]}
                >
                  <Input style={{ width: 200 }} maxLength={50} placeholder="如 供应商名称" />
                </Form.Item>
                <Form.Item
                  name="endorse_date"
                  label="背书日期"
                  rules={[{ required: true, message: '请选择背书日期' }]}
                  extra="背书月份须未月结"
                >
                  <DatePicker style={{ width: 140 }} />
                </Form.Item>
              </Space>
              <Space size="middle" style={{ display: 'flex' }}>
                <Form.Item name="purpose" label="事由">
                  <Input style={{ width: 240 }} maxLength={100} placeholder="如 付货款/转让（可选）" />
                </Form.Item>
                <Form.Item
                  name="counter_account_code"
                  label="对方科目"
                  extra="缺省 2202 应付账款"
                >
                  <Select
                    style={{ width: 220 }}
                    allowClear
                    showSearch
                    optionFilterProp="label"
                    placeholder="从科目表选择"
                    options={shared.glOptions}
                  />
                </Form.Item>
              </Space>
            </Form>
          </>
        )}
      </Modal>

      {/* 贴现弹窗：实收默认=票面可改，实时显示贴现息（财务费用） */}
      <Modal
        title={`贴现：${discountInst?.instrument_no ?? ''}`}
        open={discountInst !== null}
        onOk={handleDiscountOk}
        confirmLoading={discountSaving}
        okText="贴现"
        onCancel={() => setDiscountInst(null)}
        destroyOnHidden
      >
        {discountInst && (
          <>
            <Descriptions size="small" column={2} style={{ marginBottom: 12 }}>
              <Descriptions.Item label="票面金额">
                <SensitiveText type="amount" value={fmtAmount(discountInst.face_amount)} />
              </Descriptions.Item>
              <Descriptions.Item label="贴现息（财务费用）">
                <SensitiveText type="amount" value={fmtAmount(discountFee)} />
              </Descriptions.Item>
            </Descriptions>
            <Form form={discountForm} layout="vertical">
              <Space size="middle" style={{ display: 'flex' }}>
                <Form.Item
                  name="discount_date"
                  label="贴现日期"
                  rules={[{ required: true, message: '请选择贴现日期' }]}
                  extra="贴现月份须未月结"
                >
                  <DatePicker style={{ width: 140 }} />
                </Form.Item>
                <Form.Item
                  name="proceeds"
                  label="实收金额"
                  rules={[
                    { required: true, message: '请输入实收金额' },
                    { type: 'number', min: 0.01, message: '实收金额必须为正数' },
                  ]}
                  extra="不得大于票面；贴现息＝票面−实收"
                >
                  <InputNumber
                    min={0.01}
                    max={discountInst.face_amount}
                    step={0.01}
                    style={{ width: 160 }}
                    precision={2}
                  />
                </Form.Item>
                <Form.Item
                  name="fund_account_id"
                  label="入账资金账户"
                  rules={[{ required: true, message: '贴现必须选择入账资金账户' }]}
                >
                  <Select style={{ width: 220 }} showSearch optionFilterProp="label" options={accOptions} />
                </Form.Item>
              </Space>
            </Form>
          </>
        )}
      </Modal>

      {/* 托收弹窗：在途不记账，仅事件留痕 */}
      <Modal
        title={`发起托收：${collectInst?.instrument_no ?? ''}`}
        open={collectInst !== null}
        onOk={handleCollectOk}
        confirmLoading={collectSaving}
        okText="发起托收"
        onCancel={() => setCollectInst(null)}
        destroyOnHidden
      >
        {collectInst && (
          <>
            <Alert
              type="info"
              showIcon
              style={{ marginBottom: 12 }}
              message="托收在途不记账、不生成凭证，票据转为「托收中」；到账后请在「到账确认」完成入账。"
            />
            <Form form={collectForm} layout="vertical">
              <Form.Item
                name="operate_date"
                label="托收日期"
                rules={[{ required: true, message: '请选择托收日期' }]}
              >
                <DatePicker style={{ width: 140 }} />
              </Form.Item>
            </Form>
          </>
        )}
      </Modal>

      {/* 到账确认弹窗 */}
      <Modal
        title={`托收到账确认：${confirmInst?.instrument_no ?? ''}`}
        open={confirmInst !== null}
        onOk={handleConfirmOk}
        confirmLoading={confirmSaving}
        okText="确认到账"
        onCancel={() => setConfirmInst(null)}
        destroyOnHidden
      >
        {confirmInst && (
          <>
            <Descriptions size="small" column={1} style={{ marginBottom: 12 }}>
              <Descriptions.Item label="票面金额">
                <SensitiveText type="amount" value={fmtAmount(confirmInst.face_amount)} />
              </Descriptions.Item>
            </Descriptions>
            <Form form={confirmForm} layout="vertical">
              <Space size="middle" style={{ display: 'flex' }}>
                <Form.Item
                  name="operate_date"
                  label="到账日期"
                  rules={[{ required: true, message: '请选择到账日期' }]}
                  extra="到账月份须未月结"
                >
                  <DatePicker style={{ width: 140 }} />
                </Form.Item>
                <Form.Item
                  name="fund_account_id"
                  label="入账资金账户"
                  rules={[{ required: true, message: '到账确认必须选择入账资金账户' }]}
                >
                  <Select style={{ width: 220 }} showSearch optionFilterProp="label" options={accOptions} />
                </Form.Item>
              </Space>
            </Form>
          </>
        )}
      </Modal>

      {/* 兑付弹窗（开出承兑） */}
      <Modal
        title={`兑付开出票据：${settleInst?.instrument_no ?? ''}`}
        open={settleInst !== null}
        onOk={handleSettleOk}
        confirmLoading={settleSaving}
        okText="兑付"
        onCancel={() => setSettleInst(null)}
        destroyOnHidden
      >
        {settleInst && (
          <>
            <Descriptions size="small" column={1} style={{ marginBottom: 12 }}>
              <Descriptions.Item label="票面金额">
                <SensitiveText type="amount" value={fmtAmount(settleInst.face_amount)} />
              </Descriptions.Item>
            </Descriptions>
            <Form form={settleForm} layout="vertical">
              <Space size="middle" style={{ display: 'flex' }}>
                <Form.Item
                  name="operate_date"
                  label="兑付日期"
                  rules={[{ required: true, message: '请选择兑付日期' }]}
                  extra="兑付月份须未月结"
                >
                  <DatePicker style={{ width: 140 }} />
                </Form.Item>
                <Form.Item
                  name="fund_account_id"
                  label="出账资金账户"
                  rules={[{ required: true, message: '兑付必须选择出账资金账户' }]}
                >
                  <Select style={{ width: 220 }} showSearch optionFilterProp="label" options={accOptions} />
                </Form.Item>
              </Space>
            </Form>
          </>
        )}
      </Modal>

      {/* 作废弹窗：原因必填，仅未流转票据；登记凭证同步置 void */}
      <Modal
        title={`作废票据：${voidInst?.instrument_no ?? ''}`}
        open={voidInst !== null}
        onOk={handleVoidOk}
        confirmLoading={voidSaving}
        okText="作废"
        okButtonProps={{ danger: true }}
        onCancel={() => setVoidInst(null)}
        destroyOnHidden
      >
        {voidInst && (
          <>
            <Alert
              type="warning"
              showIcon
              style={{ marginBottom: 12 }}
              message="仅未流转票据（持有/已开出未兑付）可作废；作废后登记凭证同步作废、同号可重新登记。已流转票据请使用「冲正」。"
            />
            <Form form={voidForm} layout="vertical">
              <Form.Item
                name="reason"
                label="作废原因"
                rules={[{ required: true, message: '作废必须填写原因' }]}
              >
                <TextArea rows={3} maxLength={200} placeholder="请填写作废原因（必填）" />
              </Form.Item>
            </Form>
          </>
        )}
      </Modal>

      {/* 红字冲正弹窗：原因必填；恢复到上次流转前状态 */}
      <Modal
        title={`红字冲正：${reverseInst?.instrument_no ?? ''}`}
        open={reverseInst !== null}
        onOk={handleReverseOk}
        confirmLoading={reverseSaving}
        okText="冲正"
        okButtonProps={{ danger: true }}
        onCancel={() => setReverseInst(null)}
        destroyOnHidden
      >
        {reverseInst && (
          <>
            <Alert
              type="warning"
              showIcon
              style={{ marginBottom: 12 }}
              message={`将生成借贷互换的反向凭证（原凭证保留、净影响 0），票据恢复到「${reverseTargetLabel(
                reverseInst,
              )}」。登记月与冲正月份均须未月结。`}
            />
            <Form form={reverseForm} layout="vertical">
              <Space size="middle" style={{ display: 'flex' }}>
                <Form.Item
                  name="reverse_date"
                  label="冲正日期"
                  rules={[{ required: true, message: '请选择冲正日期' }]}
                >
                  <DatePicker style={{ width: 140 }} />
                </Form.Item>
              </Space>
              <Form.Item
                name="reason"
                label="冲正原因"
                rules={[{ required: true, message: '冲正必须填写原因' }]}
              >
                <TextArea rows={3} maxLength={200} placeholder="请填写冲正原因（必填）" />
              </Form.Item>
            </Form>
          </>
        )}
      </Modal>

      {/* 详情抽屉：票面信息 + 背书链时间线 + 状态轨迹（含关联凭证号） */}
      <Drawer
        title={detail ? `票据详情 ${detail.instrument.instrument_no}` : '票据详情'}
        open={detailOpen}
        width={680}
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
                { key: 'no', label: '票据号码', children: detail.instrument.instrument_no },
                {
                  key: 'type',
                  label: '类型',
                  children:
                    INSTRUMENT_TYPE_LABEL[detail.instrument.instrument_type] ??
                    detail.instrument.instrument_type,
                },
                {
                  key: 'direction',
                  label: '方向',
                  children: directionTag(detail.instrument.direction),
                },
                { key: 'status', label: '状态', children: statusTag(detail.instrument.status) },
                {
                  key: 'amount',
                  label: '票面金额',
                  children: (
                    <SensitiveText type="amount" value={fmtAmount(detail.instrument.face_amount)} />
                  ),
                },
                { key: 'voucher', label: '登记凭证号', children: detail.instrument.voucher_id ?? '-' },
                { key: 'issue', label: '出票日', children: detail.instrument.issue_date },
                { key: 'due', label: '到期日', children: detail.instrument.due_date },
                { key: 'drawer', label: '出票人', children: detail.instrument.drawer ?? '-' },
                { key: 'acceptor', label: '承兑人', children: detail.instrument.acceptor ?? '-' },
                { key: 'payee', label: '收款人', children: detail.instrument.payee ?? '-' },
                {
                  key: 'account',
                  label: '挂接账户',
                  children: accountName(shared.accounts, detail.instrument.fund_account_id),
                },
                {
                  key: 'partner',
                  label: '往来单位',
                  children:
                    shared.partners.find((p) => p.id === detail.instrument.partner_id)?.name ??
                    (detail.instrument.partner_id !== null
                      ? `往来单位ID=${detail.instrument.partner_id}`
                      : '-'),
                },
                {
                  key: 'counter',
                  label: '对方科目',
                  children: detail.instrument.counter_account_code ?? '-',
                },
                {
                  key: 'creator',
                  label: '登记人',
                  span: 2,
                  children:
                    detail.instrument.created_by
                      ? `${detail.instrument.created_by} · ${dayjs(detail.instrument.created_at).format('YYYY-MM-DD HH:mm')}`
                      : '-',
                },
                { key: 'remark', label: '备注', children: detail.instrument.remark ?? '-', span: 2 },
              ]}
            />

            <Title level={5} style={{ marginTop: 24 }}>
              背书链
            </Title>
            {detail.endorsements.length === 0 ? (
              <Text type="secondary">尚无背书记录</Text>
            ) : (
              <Timeline
                items={detail.endorsements.map((e: InstrumentEndorsement) => ({
                  color: 'blue',
                  content: (
                    <div key={e.id}>
                      <div>
                        <Text strong>
                          第 {e.endorse_order} 手 → 被背书人 {e.endorsee}
                        </Text>
                        <Text type="secondary">
                          {' '}
                          <SensitiveText type="amount" value={fmtAmount(e.amount)} />
                        </Text>
                      </div>
                      <Text type="secondary">
                        {e.endorse_date}
                        {e.purpose ? ` · 事由：${e.purpose}` : ''} · 凭证号 {e.voucher_id} ·{' '}
                        {e.created_by ?? '-'}
                      </Text>
                    </div>
                  ),
                }))}
              />
            )}

            <Title level={5} style={{ marginTop: 24 }}>
              状态轨迹
            </Title>
            {events.length === 0 ? (
              <Text type="secondary">尚无流转记录（登记后未发生背书/贴现/托收/兑付等操作）</Text>
            ) : (
              <Timeline
                items={events.map((e: ApprovalEvent) => ({
                  color: e.action === 'void' || e.action === 'reverse' ? 'red' : 'green',
                  content: (
                    <div key={e.id}>
                      <div>
                        <Text strong>
                          {INSTRUMENT_ACTION_LABEL[e.action] ??
                            APPROVAL_ACTION_LABEL[e.action] ??
                            e.action}
                        </Text>
                        {(e.from_status || e.to_status) && (
                          <Text type="secondary">
                            {' '}
                            {e.from_status
                              ? INSTRUMENT_STATUS_LABEL[e.from_status] ?? e.from_status
                              : '—'}{' '}
                            →{' '}
                            {e.to_status ? INSTRUMENT_STATUS_LABEL[e.to_status] ?? e.to_status : '—'}
                          </Text>
                        )}
                      </div>
                      <Text type="secondary">
                        {shared.operators.find((o) => o.id === e.operator_id)?.name ??
                          `操作人ID=${e.operator_id ?? '-'}`}{' '}
                        · {dayjs(e.created_at).format('YYYY-MM-DD HH:mm')}
                      </Text>
                      {e.comment && <div>意见：{e.comment}</div>}
                    </div>
                  ),
                }))}
              />
            )}
          </>
        )}
      </Drawer>
    </>
  );
};

// ==================== 页面入口 ====================

const NotesInstruments: React.FC = () => {
  const [shared, setShared] = useState<SharedOptions>({
    accounts: [],
    partners: [],
    glOptions: [],
    operators: [],
  });

  useEffect(() => {
    // 基础资料下拉一次性加载；单项失败不互相阻断
    getFundAccounts()
      .then((accounts) => setShared((prev) => ({ ...prev, accounts })))
      .catch(() => undefined);
    getBusinessPartners()
      .then((partners) => setShared((prev) => ({ ...prev, partners })))
      .catch(() => undefined);
    getOperatorProfiles()
      .then((operators) => setShared((prev) => ({ ...prev, operators })))
      .catch(() => undefined);
    getGlAccounts()
      .then((list: GlAccount[]) =>
        setShared((prev) => ({
          ...prev,
          glOptions: list
            .filter((acc) => acc.is_active)
            .map((acc) => ({ value: acc.code, label: `${acc.code} ${acc.name}` })),
        })),
      )
      .catch(() => undefined);
  }, []);

  return (
    <Card>
      <div className="page-header" style={{ marginBottom: 8 }}>
        <Title level={4} style={{ marginTop: 0, marginBottom: 0 }}>
          票据台账
        </Title>
      </div>
      <Alert
        type="info"
        showIcon
        style={{ marginBottom: 16 }}
        message="票据流程：登记（承兑→持有/已开出，支票登记即结算）→ 背书/贴现/托收到账 或 兑付；所有流转自动生成凭证并留痕。已流转票据纠错走「冲正」（原凭证保留、反向凭证并存），未流转票据可直接「作废」。"
      />
      <Tabs
        defaultActiveKey="received"
        destroyOnHidden
        items={[
          { key: 'received', label: '收到票据', children: <InstrumentTab direction="received" shared={shared} /> },
          { key: 'issued', label: '开出票据', children: <InstrumentTab direction="issued" shared={shared} /> },
          { key: 'all', label: '全部票据', children: <InstrumentTab shared={shared} /> },
        ]}
      />
    </Card>
  );
};

export default NotesInstruments;
