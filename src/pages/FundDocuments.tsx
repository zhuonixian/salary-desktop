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
  Image,
  Input,
  InputNumber,
  List,
  Modal,
  Popconfirm,
  Select,
  Space,
  Switch,
  Table,
  Tabs,
  Tag,
  Timeline,
  Tooltip,
  Typography,
  message,
} from 'antd';
import type { ColumnsType } from 'antd/es/table';
import { PlusOutlined, ReloadOutlined, SettingOutlined, UploadOutlined } from '@ant-design/icons';
import { convertFileSrc } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import dayjs from 'dayjs';
import type { Dayjs } from 'dayjs';
import SensitiveText from '@/components/SensitiveText';
import { useBusinessMonth } from '@/contexts/BusinessMonthContext';
import { useOperator } from '@/contexts/OperatorContext';
import {
  addBusinessAttachment,
  approveFundDocument,
  createFundDocument,
  deleteBusinessAttachment,
  getBusinessPartners,
  getDecryptedAttachmentUrl,
  getEmployees,
  getFundAccounts,
  getFundDocumentDetail,
  getFundDocuments,
  getGlAccounts,
  getMakerCheckerEnabled,
  getOperatorProfiles,
  listBusinessAttachments,
  rejectFundDocument,
  reverseFundDocument,
  settleFundDocument,
  setMakerCheckerEnabled,
  submitFundDocument,
  updateFundDocument,
  voidFundDocument,
  withdrawFundDocument,
} from '@/api';
import {
  APPROVAL_ACTION_LABEL,
  FUND_DOCUMENT_STATUS_LABEL,
  FUND_DOCUMENT_TYPE_LABEL,
} from '@/types';
import type {
  ApprovalEvent,
  BusinessAttachment,
  BusinessPartner,
  Employee,
  FundAccount,
  FundDocument,
  FundDocumentDetail,
  FundDocumentInput,
  GlAccount,
  OperatorProfile,
} from '@/types';

const { Title, Text } = Typography;
const { TextArea } = Input;

const errText = (e: unknown): string => (e instanceof Error ? e.message : String(e));

const fmtAmount = (value: number): string =>
  (Number(value) || 0).toLocaleString('zh-CN', { minimumFractionDigits: 2, maximumFractionDigits: 2 });

// 状态颜色（与后端 cashier.rs fund_status_label 中文一致）
const STATUS_COLOR: Record<string, string> = {
  draft: 'default',
  submitted: 'processing',
  approved: 'success',
  rejected: 'warning',
  batched: 'gold',
  settled: 'geekblue',
  void: '#999999',
  reversed: 'purple',
};

// 本页可手工创建的单据类型（员工借款/借款核销/冲正分别在借款页与冲正入口产生）
const CREATABLE_TYPE_OPTIONS = [
  { value: 'receipt', label: '收款单' },
  { value: 'payment', label: '付款单' },
  { value: 'transfer', label: '内部转账单' },
];

const STATUS_OPTIONS = Object.entries(FUND_DOCUMENT_STATUS_LABEL).map(([value, label]) => ({
  value,
  label,
}));

const TYPE_OPTIONS = Object.entries(FUND_DOCUMENT_TYPE_LABEL).map(([value, label]) => ({
  value,
  label,
}));

// 付款/借款单审批后须经付款批次（spec 5.1）；其余类型可直接结算
const BATCHABLE_TYPES = ['payment', 'advance'];
// 附件可变更状态（与后端 FUND_DOCUMENT_ATTACHMENT_EDITABLE_STATUSES 一致）
const ATTACHMENT_EDITABLE_STATUSES = ['draft', 'rejected', 'void'];

interface SharedOptions {
  partners: BusinessPartner[];
  employees: Employee[];
  accounts: FundAccount[];
  glOptions: { value: string; label: string }[];
  operators: OperatorProfile[];
}

const statusTag = (status: string): ReactNode => (
  <Tag color={STATUS_COLOR[status] ?? 'default'}>
    {FUND_DOCUMENT_STATUS_LABEL[status] ?? status}
  </Tag>
);

// ==================== 附件预览 ====================

const AttachmentPreviewModal: React.FC<{
  attachment: BusinessAttachment | null;
  onClose: () => void;
}> = ({ attachment, onClose }) => {
  const [url, setUrl] = useState('');
  const [note, setNote] = useState('');

  useEffect(() => {
    let cancelled = false;
    setUrl('');
    setNote('');
    if (!attachment) return undefined;
    getDecryptedAttachmentUrl(attachment.id)
      .then((path) => {
        if (cancelled) return;
        if (path) {
          setUrl(convertFileSrc(path));
        } else {
          setNote('当前环境暂不支持附件预览（浏览器预览模式或文件缺失）');
        }
      })
      .catch((e: unknown) => {
        if (!cancelled) setNote('预览加载失败: ' + errText(e));
      });
    return () => {
      cancelled = true;
    };
  }, [attachment]);

  return (
    <Modal
      title={`附件预览：${attachment?.file_name ?? ''}`}
      open={Boolean(attachment)}
      onCancel={onClose}
      footer={null}
      width={720}
      destroyOnHidden
    >
      {note ? (
        <Empty description={note} />
      ) : !url ? (
        <Empty description="加载中…" />
      ) : url.toLowerCase().endsWith('.pdf') ? (
        <iframe src={url} style={{ width: '100%', height: 480 }} title="附件预览" />
      ) : (
        <Image src={url} alt={attachment?.file_name ?? '附件'} style={{ width: '100%' }} />
      )}
    </Modal>
  );
};

// ==================== 单据列表 Tab ====================

interface FundDocFormValues {
  document_type: string;
  belong_month: Dayjs;
  document_date: Dayjs;
  amount?: number;
  summary: string;
  department?: string;
  expense_type?: string;
  counterparty_kind: 'partner' | 'employee';
  partner_id?: number;
  employee_id?: number;
  source_account_id?: number;
  target_account_id?: number;
  counter_account_code?: string;
  remark?: string;
}

type CommentAction = 'approve' | 'reject' | 'void';

const COMMENT_META: Record<CommentAction, { title: string; placeholder: string; okText: string }> = {
  approve: {
    title: '审批通过',
    placeholder: '请输入审批意见（必填）',
    okText: '通过',
  },
  reject: {
    title: '驳回',
    placeholder: '请填写驳回意见（必填），提交人可按意见修改后重新提交',
    okText: '驳回',
  },
  void: {
    title: '作废单据',
    placeholder: '请填写作废原因（必填）',
    okText: '作废',
  },
};

const DocumentTab: React.FC<{ docType?: string; shared: SharedOptions; makerChecker: boolean }> = ({
  docType,
  shared,
  makerChecker,
}) => {
  const { month } = useBusinessMonth();
  const { operator } = useOperator();

  const [docs, setDocs] = useState<FundDocument[]>([]);
  const [loading, setLoading] = useState(false);
  const [statusFilter, setStatusFilter] = useState<string | undefined>(undefined);
  const [typeFilter, setTypeFilter] = useState<string | undefined>(undefined);
  // 往来对象筛选值带前缀区分来源：p-{partner_id} / e-{employee_id}
  const [counterpartyFilter, setCounterpartyFilter] = useState<string | undefined>(undefined);
  const [accountFilter, setAccountFilter] = useState<number | undefined>(undefined);
  const [keyword, setKeyword] = useState('');

  const [formOpen, setFormOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const [editing, setEditing] = useState<FundDocument | null>(null);
  const [form] = Form.useForm<FundDocFormValues>();
  const watchBelongMonth = Form.useWatch('belong_month', form);
  const watchDocType = Form.useWatch('document_type', form);
  const watchCounterpartyKind = Form.useWatch('counterparty_kind', form);

  const [commentAction, setCommentAction] = useState<{
    action: CommentAction;
    doc: FundDocument;
  } | null>(null);
  const [commentSaving, setCommentSaving] = useState(false);
  const [commentForm] = Form.useForm<{ comment: string }>();

  const [reverseDoc, setReverseDoc] = useState<FundDocument | null>(null);
  const [reverseSaving, setReverseSaving] = useState(false);
  const [reverseForm] = Form.useForm<{
    belong_month: Dayjs;
    document_date: Dayjs;
    comment: string;
  }>();

  const [detail, setDetail] = useState<FundDocumentDetail | null>(null);
  const [detailOpen, setDetailOpen] = useState(false);
  const [detailLoading, setDetailLoading] = useState(false);
  const [attachments, setAttachments] = useState<BusinessAttachment[]>([]);
  const [previewAtt, setPreviewAtt] = useState<BusinessAttachment | null>(null);

  const accountName = useCallback(
    (id: number | null): string =>
      id === null ? '' : shared.accounts.find((a) => a.id === id)?.name ?? `账户ID=${id}`,
    [shared.accounts],
  );

  const counterpartyLabel = useCallback(
    (doc: FundDocument): string => {
      if (doc.partner_id !== null) {
        return shared.partners.find((p) => p.id === doc.partner_id)?.name ?? `往来单位ID=${doc.partner_id}`;
      }
      if (doc.employee_id !== null) {
        return shared.employees.find((e) => e.id === doc.employee_id)?.name ?? `员工ID=${doc.employee_id}`;
      }
      return '-';
    },
    [shared.partners, shared.employees],
  );

  const operatorName = useCallback(
    (id: number | null): string =>
      id === null ? '未知操作人' : shared.operators.find((o) => o.id === id)?.name ?? `操作人ID=${id}`,
    [shared.operators],
  );

  const fetchDocs = useCallback(async () => {
    setLoading(true);
    try {
      setDocs(
        await getFundDocuments({
          belong_month: month.format('YYYY-MM'),
          document_type: docType ?? typeFilter,
          status: statusFilter,
          partner_id: counterpartyFilter?.startsWith('p-')
            ? Number(counterpartyFilter.slice(2))
            : undefined,
          employee_id: counterpartyFilter?.startsWith('e-')
            ? Number(counterpartyFilter.slice(2))
            : undefined,
          account_id: accountFilter,
          keyword: keyword.trim() || undefined,
        }),
      );
    } catch (e: unknown) {
      message.error('获取资金单据失败: ' + errText(e));
    } finally {
      setLoading(false);
    }
  }, [month, docType, typeFilter, statusFilter, counterpartyFilter, accountFilter, keyword]);

  useEffect(() => {
    fetchDocs();
  }, [fetchDocs]);

  const fetchDetail = useCallback(async (id: number) => {
    setDetailLoading(true);
    try {
      setDetail(await getFundDocumentDetail(id));
    } catch (e: unknown) {
      message.error('获取单据详情失败: ' + errText(e));
    } finally {
      setDetailLoading(false);
    }
  }, []);

  const loadAttachments = useCallback(async (entityId: number) => {
    try {
      setAttachments(await listBusinessAttachments('fund_document', entityId));
    } catch {
      setAttachments([]);
    }
  }, []);

  const openDetail = async (doc: FundDocument) => {
    setDetailOpen(true);
    await fetchDetail(doc.id);
    await loadAttachments(doc.id);
  };

  // 状态流转成功后的统一收尾：刷新列表；详情抽屉开着且是同一单据时同步刷新
  const afterTransition = async (docId?: number) => {
    await fetchDocs();
    if (detailOpen && docId) {
      await fetchDetail(docId);
    }
  };

  // ---------- 表单（新建/编辑） ----------

  const openCreate = () => {
    setEditing(null);
    form.resetFields();
    form.setFieldsValue({
      document_type: docType ?? 'receipt',
      belong_month: month,
      document_date: month.date(Math.min(dayjs().date(), month.daysInMonth())),
      counterparty_kind: 'partner',
    });
    setFormOpen(true);
  };

  const openEdit = (doc: FundDocument) => {
    setEditing(doc);
    form.resetFields();
    form.setFieldsValue({
      document_type: doc.document_type,
      belong_month: dayjs(doc.belong_month),
      document_date: dayjs(doc.document_date),
      amount: doc.amount,
      summary: doc.summary,
      department: doc.department ?? undefined,
      expense_type: doc.expense_type ?? undefined,
      counterparty_kind: doc.employee_id !== null ? 'employee' : 'partner',
      partner_id: doc.partner_id ?? undefined,
      employee_id: doc.employee_id ?? undefined,
      source_account_id: doc.source_account_id ?? undefined,
      target_account_id: doc.target_account_id ?? undefined,
      counter_account_code: doc.counter_account_code ?? undefined,
      remark: doc.remark ?? undefined,
    });
    setFormOpen(true);
  };

  const needsCounterparty = watchDocType === 'receipt' || watchDocType === 'payment';

  const handleFormSave = async () => {
    let values: FundDocFormValues;
    try {
      values = await form.validateFields();
    } catch {
      return; // 校验失败信息由表单展示
    }
    setSaving(true);
    try {
      const isTransfer = values.document_type === 'transfer';
      const data: FundDocumentInput = {
        id: editing?.id,
        document_type: values.document_type,
        belong_month: values.belong_month.format('YYYY-MM'),
        document_date: values.document_date.format('YYYY-MM-DD'),
        amount: values.amount ?? 0,
        summary: values.summary.trim(),
        department: values.department?.trim() ?? '',
        expense_type: values.expense_type?.trim() ?? '',
        partner_id: needsCounterparty && values.counterparty_kind === 'partner' ? values.partner_id ?? null : null,
        employee_id: needsCounterparty && values.counterparty_kind === 'employee' ? values.employee_id ?? null : null,
        source_account_id: isTransfer || values.document_type === 'payment' ? values.source_account_id ?? null : null,
        target_account_id: isTransfer || values.document_type === 'receipt' ? values.target_account_id ?? null : null,
        counter_account_code: values.counter_account_code ?? '',
        remark: values.remark?.trim() ?? '',
      };
      if (editing) {
        await updateFundDocument(data);
        message.success('单据已保存');
      } else {
        await createFundDocument(data);
        message.success('单据已创建（草稿）');
      }
      setFormOpen(false);
      fetchDocs();
    } catch (e: unknown) {
      message.error('保存失败: ' + errText(e));
    } finally {
      setSaving(false);
    }
  };

  // ---------- 状态机操作 ----------

  const runAction = async (fn: () => Promise<FundDocument>, success: string, docId?: number) => {
    try {
      await fn();
      message.success(success);
      await afterTransition(docId);
    } catch (e: unknown) {
      message.error('操作失败: ' + errText(e));
    }
  };

  const handleCommentOk = async () => {
    if (!commentAction) return;
    let values: { comment: string };
    try {
      values = await commentForm.validateFields();
    } catch {
      return;
    }
    setCommentSaving(true);
    const { action, doc } = commentAction;
    try {
      if (action === 'approve') {
        await approveFundDocument(doc.id, values.comment.trim());
      } else if (action === 'reject') {
        await rejectFundDocument(doc.id, values.comment.trim());
      } else {
        await voidFundDocument(doc.id, values.comment.trim());
      }
      message.success(`${COMMENT_META[action].title}成功`);
      setCommentAction(null);
      await afterTransition(doc.id);
    } catch (e: unknown) {
      message.error('操作失败: ' + errText(e));
    } finally {
      setCommentSaving(false);
    }
  };

  const handleReverseOk = async () => {
    if (!reverseDoc) return;
    let values: { belong_month: Dayjs; document_date: Dayjs; comment: string };
    try {
      values = await reverseForm.validateFields();
    } catch {
      return;
    }
    setReverseSaving(true);
    try {
      await reverseFundDocument({
        document_id: reverseDoc.id,
        belong_month: values.belong_month.format('YYYY-MM'),
        document_date: values.document_date.format('YYYY-MM-DD'),
        comment: values.comment.trim(),
      });
      message.success('冲正单已生成并结算，原单已置为已冲正');
      setReverseDoc(null);
      await afterTransition(reverseDoc.id);
    } catch (e: unknown) {
      message.error('冲正失败: ' + errText(e));
    } finally {
      setReverseSaving(false);
    }
  };

  // ---------- 附件 ----------

  const detailEditable =
    detail !== null && ATTACHMENT_EDITABLE_STATUSES.includes(detail.document.status);

  const handleUploadAttachment = async () => {
    if (!detail) return;
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: '附件文件', extensions: ['pdf', 'png', 'jpg', 'jpeg', 'bmp'] }],
      });
      if (!selected) return;
      const filePath = selected as string;
      const fileName = filePath.split(/[\\/]/).pop() ?? 'attachment.bin';
      await addBusinessAttachment({
        entity_type: 'fund_document',
        entity_id: detail.document.id,
        file_name: fileName,
        file_path: filePath,
        belong_month: detail.document.belong_month,
      });
      message.success('附件已上传');
      await loadAttachments(detail.document.id);
    } catch (e: unknown) {
      message.error('上传附件失败: ' + errText(e));
    }
  };

  const handleDeleteAttachment = async (att: BusinessAttachment) => {
    try {
      await deleteBusinessAttachment(att.id);
      message.success('附件已删除');
      if (detail) await loadAttachments(detail.document.id);
    } catch (e: unknown) {
      message.error('删除附件失败: ' + errText(e));
    }
  };

  // ---------- 表格 ----------

  const columns: ColumnsType<FundDocument> = [
    { title: '单号', dataIndex: 'document_no', key: 'document_no', width: 170 },
    {
      title: '类型',
      dataIndex: 'document_type',
      key: 'document_type',
      width: 100,
      render: (value: string) => (
        <Tag color="blue">{FUND_DOCUMENT_TYPE_LABEL[value] ?? value}</Tag>
      ),
    },
    { title: '单据日期', dataIndex: 'document_date', key: 'document_date', width: 105 },
    {
      title: '往来对象',
      key: 'counterparty',
      width: 150,
      ellipsis: true,
      render: (_, record) => counterpartyLabel(record),
    },
    {
      title: '金额',
      dataIndex: 'amount',
      key: 'amount',
      width: 130,
      align: 'right',
      render: (value: number) => <SensitiveText type="amount" value={fmtAmount(value)} />,
    },
    {
      title: '账户（来源→目标）',
      key: 'accounts',
      width: 210,
      ellipsis: true,
      render: (_, record) => {
        const parts: string[] = [];
        if (record.source_account_id !== null) parts.push(accountName(record.source_account_id));
        if (record.target_account_id !== null) parts.push(accountName(record.target_account_id));
        return parts.length > 0 ? parts.join(' → ') : '-';
      },
    },
    { title: '摘要', dataIndex: 'summary', key: 'summary', ellipsis: true },
    { title: '对方科目', dataIndex: 'counter_account_code', key: 'counter_account_code', width: 90, render: (v?: string | null) => v ?? '-' },
    { title: '状态', dataIndex: 'status', key: 'status', width: 90, render: statusTag },
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
          {record.status === 'draft' && (
            <>
              <Popconfirm
                title="提交后业务字段将冻结，须撤回才可修改。确认提交？"
                onConfirm={() => void runAction(() => submitFundDocument(record.id), '单据已提交', record.id)}
              >
                <Button type="link" size="small">
                  提交
                </Button>
              </Popconfirm>
              {CREATABLE_TYPE_OPTIONS.some((t) => t.value === record.document_type) && (
                <Button type="link" size="small" onClick={() => openEdit(record)}>
                  编辑
                </Button>
              )}
              <Button
                type="link"
                size="small"
                danger
                onClick={() => {
                  commentForm.resetFields();
                  setCommentAction({ action: 'void', doc: record });
                }}
              >
                作废
              </Button>
            </>
          )}
          {record.status === 'submitted' && (
            <>
              <Button
                type="link"
                size="small"
                onClick={() => {
                  commentForm.resetFields();
                  setCommentAction({ action: 'approve', doc: record });
                }}
              >
                审批
              </Button>
              <Button
                type="link"
                size="small"
                danger
                onClick={() => {
                  commentForm.resetFields();
                  setCommentAction({ action: 'reject', doc: record });
                }}
              >
                驳回
              </Button>
              <Popconfirm
                title="确认撤回至草稿？撤回后可修改并重新提交。"
                onConfirm={() => void runAction(() => withdrawFundDocument(record.id), '单据已撤回至草稿', record.id)}
              >
                <Button type="link" size="small">
                  撤回
                </Button>
              </Popconfirm>
            </>
          )}
          {record.status === 'rejected' && (
            <>
              <Popconfirm
                title="确认撤回至草稿？可按驳回意见修改后重新提交。"
                onConfirm={() => void runAction(() => withdrawFundDocument(record.id), '单据已撤回至草稿', record.id)}
              >
                <Button type="link" size="small">
                  撤回
                </Button>
              </Popconfirm>
              <Button
                type="link"
                size="small"
                danger
                onClick={() => {
                  commentForm.resetFields();
                  setCommentAction({ action: 'void', doc: record });
                }}
              >
                作废
              </Button>
            </>
          )}
          {record.status === 'approved' &&
            (BATCHABLE_TYPES.includes(record.document_type) ? (
              <Tooltip title="付款单/借款单审批后在「付款批次」创建通用批次，导出并标记付款后自动结算">
                <Button type="link" size="small" disabled>
                  待付款批次
                </Button>
              </Tooltip>
            ) : (
              <Popconfirm
                title={`确认结算该${FUND_DOCUMENT_TYPE_LABEL[record.document_type] ?? '单据'}？结算后只能通过冲正纠错。`}
                onConfirm={() => void runAction(() => settleFundDocument(record.id), '单据已结算', record.id)}
              >
                <Button type="link" size="small">
                  结算
                </Button>
              </Popconfirm>
            ))}
          {record.status === 'batched' && (
            <Tooltip title="已进入付款批次，批次标记付款后自动结算">
              <Button type="link" size="small" disabled>
                批次处理中
              </Button>
            </Tooltip>
          )}
          {record.status === 'settled' && (
            <Button
              type="link"
              size="small"
              danger
              onClick={() => {
                reverseForm.resetFields();
                reverseForm.setFieldsValue({
                  belong_month: month,
                  document_date: month.date(Math.min(dayjs().date(), month.daysInMonth())),
                });
                setReverseDoc(record);
              }}
            >
              冲正
            </Button>
          )}
        </Space>
      ),
    },
  ];

  const approveSelfBlocked =
    commentAction?.action === 'approve' &&
    makerChecker &&
    commentAction.doc.submitted_by !== null &&
    commentAction.doc.submitted_by === operator?.id;

  const accountFilterOptions = shared.accounts
    .filter((a) => a.is_active)
    .map((a) => ({ value: a.id, label: `${a.name}（${a.account_code}）` }));

  return (
    <>
      <Space wrap style={{ marginBottom: 16 }}>
        {!docType && (
          <Select
            style={{ width: 140 }}
            allowClear
            placeholder="单据类型"
            value={typeFilter}
            onChange={setTypeFilter}
            options={TYPE_OPTIONS}
          />
        )}
        <Select
          style={{ width: 120 }}
          allowClear
          placeholder="状态"
          value={statusFilter}
          onChange={setStatusFilter}
          options={STATUS_OPTIONS}
        />
        <Select
          style={{ width: 180 }}
          allowClear
          showSearch
          optionFilterProp="label"
          placeholder="往来对象"
          value={counterpartyFilter}
          onChange={setCounterpartyFilter}
          options={[
            {
              label: '往来单位',
              options: shared.partners.map((p) => ({ value: `p-${p.id}`, label: p.name })),
            },
            {
              label: '员工',
              options: shared.employees.map((e) => ({
                value: `e-${e.id}`,
                label: `${e.name}（${e.employee_no}）`,
              })),
            },
          ]}
        />
        <Select
          style={{ width: 200 }}
          allowClear
          showSearch
          optionFilterProp="label"
          placeholder="资金账户"
          value={accountFilter}
          onChange={setAccountFilter}
          options={accountFilterOptions}
        />
        <Input.Search
          style={{ width: 220 }}
          allowClear
          placeholder="搜索单号/摘要"
          value={keyword}
          onChange={(e) => setKeyword(e.target.value)}
          onSearch={fetchDocs}
        />
        <Button type="primary" icon={<PlusOutlined />} onClick={openCreate}>
          新建单据
        </Button>
        <Button icon={<ReloadOutlined />} onClick={fetchDocs}>
          刷新
        </Button>
      </Space>

      <Table
        rowKey="id"
        columns={columns}
        dataSource={docs}
        loading={loading}
        size="middle"
        scroll={{ x: 1360 }}
        pagination={{ pageSize: 20, showTotal: (t) => `共 ${t} 条` }}
      />

      {/* 新建/编辑弹窗：表单不含状态字段，状态完全由状态机命令流转 */}
      <Modal
        title={editing ? `编辑单据 ${editing.document_no}` : '新建资金单据'}
        open={formOpen}
        onOk={handleFormSave}
        confirmLoading={saving}
        onCancel={() => setFormOpen(false)}
        width={640}
        destroyOnHidden
      >
        <Form form={form} layout="vertical">
          <Space size="middle" style={{ display: 'flex' }}>
            <Form.Item
              name="document_type"
              label="单据类型"
              rules={[{ required: true, message: '请选择单据类型' }]}
              extra="员工借款/借款核销单在「借款备用金」页维护；冲正单由已结算单据的冲正入口产生"
            >
              <Select
                style={{ width: 150 }}
                options={CREATABLE_TYPE_OPTIONS}
                disabled={Boolean(editing)}
                onChange={() => {
                  // 切换类型时清空方向相关字段，避免残留非法组合
                  form.setFieldsValue({
                    source_account_id: undefined,
                    target_account_id: undefined,
                    partner_id: undefined,
                    employee_id: undefined,
                    counter_account_code: undefined,
                  });
                }}
              />
            </Form.Item>
            <Form.Item
              name="belong_month"
              label="归属月份"
              rules={[{ required: true, message: '请选择归属月份' }]}
            >
              <DatePicker
                picker="month"
                style={{ width: 130 }}
                onChange={(value) => {
                  // 单据日期必须落在归属月份内（后端强校验），切月时联动修正
                  if (value) {
                    form.setFieldsValue({
                      document_date: value.date(Math.min(dayjs().date(), value.daysInMonth())),
                    });
                  }
                }}
              />
            </Form.Item>
            <Form.Item
              name="document_date"
              label="单据日期"
              rules={[{ required: true, message: '请选择单据日期' }]}
            >
              <DatePicker
                style={{ width: 140 }}
                disabledDate={(d) =>
                  watchBelongMonth ? d.format('YYYY-MM') !== watchBelongMonth.format('YYYY-MM') : false
                }
              />
            </Form.Item>
          </Space>
          <Space size="middle" style={{ display: 'flex' }}>
            <Form.Item
              name="amount"
              label="金额"
              rules={[
                { required: true, message: '请输入金额' },
                { type: 'number', min: 0.01, message: '金额必须为正数' },
              ]}
            >
              <InputNumber min={0.01} step={0.01} style={{ width: 160 }} precision={2} />
            </Form.Item>
            <Form.Item
              name="summary"
              label="摘要"
              rules={[{ required: true, message: '请输入摘要' }]}
            >
              <Input placeholder="如 支付XX供应商货款" style={{ width: 320 }} maxLength={100} />
            </Form.Item>
          </Space>

          {needsCounterparty && (
            <Space size="middle" style={{ display: 'flex' }}>
              <Form.Item name="counterparty_kind" label="往来对象类型">
                <Select
                  style={{ width: 130 }}
                  options={[
                    { value: 'partner', label: '往来单位' },
                    { value: 'employee', label: '员工' },
                  ]}
                  onChange={() => {
                    form.setFieldsValue({ partner_id: undefined, employee_id: undefined });
                  }}
                />
              </Form.Item>
              {watchDocType && (
                <Form.Item
                  name={watchCounterpartyKind === 'employee' ? 'employee_id' : 'partner_id'}
                  label="往来对象"
                  rules={[{ required: true, message: '请选择往来对象' }]}
                  preserve={false}
                >
                  <Select
                    style={{ width: 240 }}
                    showSearch
                    optionFilterProp="label"
                    options={
                      watchCounterpartyKind === 'employee'
                        ? shared.employees.map((e) => ({
                            value: e.id,
                            label: `${e.name}（${e.employee_no}）`,
                          }))
                        : shared.partners.map((p) => ({ value: p.id, label: p.name }))
                    }
                  />
                </Form.Item>
              )}
            </Space>
          )}

          <Space size="middle" style={{ display: 'flex' }}>
            {(watchDocType === 'payment' || watchDocType === 'transfer') && (
              <Form.Item
                name="source_account_id"
                label="来源账户（资金流出）"
                rules={[{ required: true, message: '请选择来源账户' }]}
                preserve={false}
              >
                <Select style={{ width: 220 }} showSearch optionFilterProp="label" options={accountFilterOptions} />
              </Form.Item>
            )}
            {(watchDocType === 'receipt' || watchDocType === 'transfer') && (
              <Form.Item
                name="target_account_id"
                label="目标账户（资金流入）"
                rules={[
                  { required: true, message: '请选择目标账户' },
                  ({ getFieldValue }) => ({
                    validator: (_, value) =>
                      watchDocType === 'transfer' &&
                      value &&
                      getFieldValue('source_account_id') === value
                        ? Promise.reject(new Error('内部转账的来源账户与目标账户不能相同'))
                        : Promise.resolve(),
                  }),
                ]}
                preserve={false}
              >
                <Select style={{ width: 220 }} showSearch optionFilterProp="label" options={accountFilterOptions} />
              </Form.Item>
            )}
          </Space>

          <Space size="middle" style={{ display: 'flex' }}>
            <Form.Item
              name="counter_account_code"
              label="对方科目"
              rules={
                watchDocType === 'receipt' || watchDocType === 'payment'
                  ? [{ required: true, message: '收款/付款单必须选择对方科目' }]
                  : undefined
              }
              extra={
                watchDocType === 'transfer'
                  ? '内部转账可选；两账户挂接科目相同时无需填写'
                  : '资金科目以外的总账科目（结算生成凭证的借贷对方）'
              }
            >
              <Select
                style={{ width: 260 }}
                allowClear
                showSearch
                optionFilterProp="label"
                placeholder="从科目表选择"
                options={shared.glOptions}
              />
            </Form.Item>
            {needsCounterparty && (
              <>
                <Form.Item name="department" label="部门">
                  <Input style={{ width: 140 }} maxLength={32} />
                </Form.Item>
                <Form.Item name="expense_type" label="费用类型">
                  <Input style={{ width: 140 }} maxLength={32} />
                </Form.Item>
              </>
            )}
          </Space>

          <Form.Item name="remark" label="备注">
            <TextArea rows={2} placeholder="备注（可选）" maxLength={200} />
          </Form.Item>
        </Form>
      </Modal>

      {/* 审批/驳回/作废意见弹窗（意见必填由后端强校验） */}
      <Modal
        title={
          commentAction
            ? `${COMMENT_META[commentAction.action].title}：${commentAction.doc.document_no}`
            : ''
        }
        open={commentAction !== null}
        onOk={handleCommentOk}
        confirmLoading={commentSaving}
        okText={commentAction ? COMMENT_META[commentAction.action].okText : '确定'}
        okButtonProps={commentAction?.action === 'void' ? { danger: true } : undefined}
        onCancel={() => setCommentAction(null)}
        destroyOnHidden
      >
        {commentAction && (
          <>
            {approveSelfBlocked && (
              <Alert
                type="warning"
                showIcon
                style={{ marginBottom: 12 }}
                message="经办复核已开启：您是该单据的提交人，不能审批自己的单据。请先在页头切换为其他操作人。"
              />
            )}
            <Descriptions size="small" column={1} style={{ marginBottom: 12 }}>
              <Descriptions.Item label="单据">
                {commentAction.doc.document_no}（
                {FUND_DOCUMENT_TYPE_LABEL[commentAction.doc.document_type] ?? commentAction.doc.document_type}
                ）
              </Descriptions.Item>
              <Descriptions.Item label="金额">
                <SensitiveText type="amount" value={fmtAmount(commentAction.doc.amount)} />
              </Descriptions.Item>
            </Descriptions>
            <Form form={commentForm} layout="vertical">
              <Form.Item
                name="comment"
                label="意见"
                rules={[{ required: true, message: COMMENT_META[commentAction.action].placeholder }]}
              >
                <TextArea
                  rows={3}
                  maxLength={200}
                  placeholder={COMMENT_META[commentAction.action].placeholder}
                />
              </Form.Item>
            </Form>
          </>
        )}
      </Modal>

      {/* 冲正弹窗：原因必填，冲正月份默认当前业务月份（原单月份与冲正月份均须未月结） */}
      <Modal
        title={`冲正单据：${reverseDoc?.document_no ?? ''}`}
        open={reverseDoc !== null}
        onOk={handleReverseOk}
        confirmLoading={reverseSaving}
        okText="生成冲正单"
        onCancel={() => setReverseDoc(null)}
        destroyOnHidden
      >
        {reverseDoc && (
          <>
            <Alert
              type="warning"
              showIcon
              style={{ marginBottom: 12 }}
              message="将在所选月份创建一张相反方向的冲正单（立即结算生效），原单置为「已冲正」。原单与冲正月份都必须未月结。"
            />
            <Form form={reverseForm} layout="vertical">
              <Space size="middle" style={{ display: 'flex' }}>
                <Form.Item
                  name="belong_month"
                  label="冲正月份"
                  rules={[{ required: true, message: '请选择冲正月份' }]}
                >
                  <DatePicker
                    picker="month"
                    style={{ width: 130 }}
                    onChange={(value) => {
                      if (value) {
                        reverseForm.setFieldsValue({
                          document_date: value.date(Math.min(dayjs().date(), value.daysInMonth())),
                        });
                      }
                    }}
                  />
                </Form.Item>
                <Form.Item
                  name="document_date"
                  label="冲正日期"
                  rules={[{ required: true, message: '请选择冲正日期' }]}
                >
                  <DatePicker
                    style={{ width: 140 }}
                    disabledDate={(d) => {
                      const m = reverseForm.getFieldValue('belong_month') as Dayjs | undefined;
                      return m ? d.format('YYYY-MM') !== m.format('YYYY-MM') : false;
                    }}
                  />
                </Form.Item>
              </Space>
              <Form.Item
                name="comment"
                label="冲正原因"
                rules={[{ required: true, message: '冲正必须填写原因' }]}
              >
                <TextArea rows={3} maxLength={200} placeholder="请填写冲正原因（必填）" />
              </Form.Item>
            </Form>
          </>
        )}
      </Modal>

      {/* 详情抽屉：单据信息 + 审批时间线 + 附件；凭证链接在凭证联动上线后展示 */}
      <Drawer
        title={detail ? `单据详情 ${detail.document.document_no}` : '单据详情'}
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
                { key: 'no', label: '单号', children: detail.document.document_no },
                {
                  key: 'type',
                  label: '类型',
                  children: FUND_DOCUMENT_TYPE_LABEL[detail.document.document_type] ?? detail.document.document_type,
                },
                { key: 'status', label: '状态', children: statusTag(detail.document.status) },
                {
                  key: 'amount',
                  label: '金额',
                  children: <SensitiveText type="amount" value={fmtAmount(detail.document.amount)} />,
                },
                { key: 'date', label: '单据日期', children: detail.document.document_date },
                { key: 'month', label: '归属月份', children: detail.document.belong_month },
                { key: 'partner', label: '往来对象', children: counterpartyLabel(detail.document) },
                {
                  key: 'accounts',
                  label: '账户',
                  children:
                    [
                      detail.document.source_account_id !== null
                        ? `来源：${accountName(detail.document.source_account_id)}`
                        : null,
                      detail.document.target_account_id !== null
                        ? `目标：${accountName(detail.document.target_account_id)}`
                        : null,
                    ]
                      .filter(Boolean)
                      .join('；') || '-',
                },
                {
                  key: 'counter',
                  label: '对方科目',
                  children: detail.document.counter_account_code ?? '-',
                },
                {
                  key: 'voucher',
                  label: '关联凭证',
                  children: '结算/付款后自动生成（凭证联动上线后可跳转）',
                },
                {
                  key: 'department',
                  label: '部门/费用',
                  children:
                    [detail.document.department, detail.document.expense_type]
                      .filter(Boolean)
                      .join(' / ') || '-',
                },
                {
                  key: 'submit',
                  label: '提交',
                  children: detail.document.submitted_at
                    ? `${operatorName(detail.document.submitted_by)} ${dayjs(detail.document.submitted_at).format('YYYY-MM-DD HH:mm')}`
                    : '-',
                },
                {
                  key: 'approve',
                  label: '审批',
                  children: detail.document.approved_at
                    ? `${operatorName(detail.document.approved_by)} ${dayjs(detail.document.approved_at).format('YYYY-MM-DD HH:mm')}`
                    : '-',
                },
                {
                  key: 'settle',
                  label: '结算',
                  children: detail.document.settled_at
                    ? `${operatorName(detail.document.settled_by)} ${dayjs(detail.document.settled_at).format('YYYY-MM-DD HH:mm')}`
                    : '-',
                },
                { key: 'remark', label: '备注', children: detail.document.remark ?? '-', span: 2 },
              ]}
            />

            <Title level={5} style={{ marginTop: 24 }}>
              审批轨迹
            </Title>
            {detail.events.length === 0 ? (
              <Text type="secondary">尚无审批记录（草稿未提交）</Text>
            ) : (
              <Timeline
                items={detail.events.map((e: ApprovalEvent) => ({
                  color:
                    e.action === 'reject' || e.action === 'void'
                      ? 'red'
                      : e.action === 'approve' || e.action === 'settle'
                        ? 'green'
                        : 'blue',
                  content: (
                    <div key={e.id}>
                      <div>
                        <Text strong>{APPROVAL_ACTION_LABEL[e.action] ?? e.action}</Text>
                        {(e.from_status || e.to_status) && (
                          <Text type="secondary">
                            {' '}
                            {e.from_status
                              ? `${FUND_DOCUMENT_STATUS_LABEL[e.from_status] ?? e.from_status}`
                              : '—'}{' '}
                            → {e.to_status ? FUND_DOCUMENT_STATUS_LABEL[e.to_status] ?? e.to_status : '—'}
                          </Text>
                        )}
                      </div>
                      <Text type="secondary">
                        {operatorName(e.operator_id)} · {dayjs(e.created_at).format('YYYY-MM-DD HH:mm')}
                      </Text>
                      {e.comment && <div>意见：{e.comment}</div>}
                    </div>
                  ),
                }))}
              />
            )}

            <Title level={5} style={{ marginTop: 24 }}>
              附件
            </Title>
            {detailEditable ? (
              <Button
                size="small"
                icon={<UploadOutlined />}
                style={{ marginBottom: 12 }}
                onClick={handleUploadAttachment}
              >
                上传附件
              </Button>
            ) : (
              <Alert
                type="info"
                showIcon
                style={{ marginBottom: 12 }}
                message="仅草稿/已驳回/已作废单据可上传或删除附件；已提交单据须先撤回或驳回。"
              />
            )}
            <List
              size="small"
              dataSource={attachments}
              locale={{ emptyText: '暂无附件' }}
              renderItem={(att) => (
                <List.Item
                  actions={
                    detailEditable
                      ? [
                          <Button key="preview" type="link" size="small" onClick={() => setPreviewAtt(att)}>
                            预览
                          </Button>,
                          <Popconfirm
                            key="delete"
                            title="确认删除该附件？"
                            onConfirm={() => void handleDeleteAttachment(att)}
                          >
                            <Button type="link" size="small" danger>
                              删除
                            </Button>
                          </Popconfirm>,
                        ]
                      : [
                          <Button key="preview" type="link" size="small" onClick={() => setPreviewAtt(att)}>
                            预览
                          </Button>,
                        ]
                  }
                >
                  <List.Item.Meta
                    title={att.file_name}
                    description={`${att.encrypted ? '已加密' : '未加密'} · ${
                      att.file_size ? `${(att.file_size / 1024).toFixed(1)} KB` : '大小未知'
                    } · ${att.uploaded_by ?? '-'} 上传于 ${dayjs(att.created_at).format('YYYY-MM-DD HH:mm')}`}
                  />
                </List.Item>
              )}
            />
          </>
        )}
      </Drawer>

      <AttachmentPreviewModal attachment={previewAtt} onClose={() => setPreviewAtt(null)} />
    </>
  );
};

// ==================== 页面入口 ====================

const FundDocuments: React.FC = () => {
  const [shared, setShared] = useState<SharedOptions>({
    partners: [],
    employees: [],
    accounts: [],
    glOptions: [],
    operators: [],
  });
  const [makerChecker, setMakerChecker] = useState(false);
  const [settingOpen, setSettingOpen] = useState(false);
  const [settingSaving, setSettingSaving] = useState(false);

  useEffect(() => {
    // 基础资料下拉一次性加载；单项失败不互相阻断
    getBusinessPartners()
      .then((partners) => setShared((prev) => ({ ...prev, partners })))
      .catch(() => undefined);
    getEmployees()
      .then((employees) => setShared((prev) => ({ ...prev, employees })))
      .catch(() => undefined);
    getFundAccounts()
      .then((accounts) => setShared((prev) => ({ ...prev, accounts })))
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
    getMakerCheckerEnabled()
      .then(setMakerChecker)
      .catch(() => undefined);
  }, []);

  const handleSaveSetting = async () => {
    setSettingSaving(true);
    try {
      await setMakerCheckerEnabled(makerChecker);
      message.success(makerChecker ? '已开启经办复核' : '已关闭经办复核');
      setSettingOpen(false);
    } catch (e: unknown) {
      message.error('保存失败: ' + errText(e));
    } finally {
      setSettingSaving(false);
    }
  };

  return (
    <Card>
      <div className="page-header" style={{ marginBottom: 8 }}>
        <Title level={4} style={{ marginTop: 0, marginBottom: 0 }}>
          收付款单
        </Title>
        <Button icon={<SettingOutlined />} onClick={() => setSettingOpen(true)}>
          审批设置
        </Button>
      </div>
      <Alert
        type="info"
        showIcon
        style={{ marginBottom: 16 }}
        message="单据流程：草稿 → 提交 → 审批 → 结算/付款批次；所有状态变更都会记录审批轨迹与操作日志。已结算单据只能通过冲正纠错。"
      />
      <Tabs
        defaultActiveKey="receipt"
        destroyOnHidden
        items={[
          { key: 'receipt', label: '收款单', children: <DocumentTab docType="receipt" shared={shared} makerChecker={makerChecker} /> },
          { key: 'payment', label: '付款单', children: <DocumentTab docType="payment" shared={shared} makerChecker={makerChecker} /> },
          { key: 'transfer', label: '内部转账', children: <DocumentTab docType="transfer" shared={shared} makerChecker={makerChecker} /> },
          { key: 'all', label: '全部单据', children: <DocumentTab shared={shared} makerChecker={makerChecker} /> },
        ]}
      />

      <Modal
        title="审批设置（经办复核）"
        open={settingOpen}
        onOk={handleSaveSetting}
        confirmLoading={settingSaving}
        okText="保存"
        onCancel={() => setSettingOpen(false)}
      >
        <Space align="start">
          <Switch
            checked={makerChecker}
            onChange={(checked) => setMakerChecker(checked)}
            style={{ marginTop: 4 }}
          />
          <div>
            <div>开启后，单据的审批人与提交人不能是同一操作人</div>
            <Text type="secondary">
              操作人仅为本地署名（不是多用户权限）；开启审批前请通过页头切换到其他操作人再执行审批。
            </Text>
          </div>
        </Space>
      </Modal>
    </Card>
  );
};

export default FundDocuments;
