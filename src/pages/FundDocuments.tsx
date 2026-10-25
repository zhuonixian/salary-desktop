import { useCallback, useEffect, useState } from 'react';
import {
  Alert,
  Button,
  Card,
  DatePicker,
  Descriptions,
  Form,
  Input,
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
import { PlusOutlined, QuestionCircleOutlined, ReloadOutlined, SettingOutlined } from '@ant-design/icons';
import { open } from '@tauri-apps/plugin-dialog';
import dayjs from 'dayjs';
import type { Dayjs } from 'dayjs';
import SensitiveText from '@/components/SensitiveText';
import PageTour from '@/components/PageTour';
import { tourTarget } from '@/components/tourTarget';
import FundDocumentDetailDrawer from '@/components/FundDocumentDetailDrawer';
import FundDocumentFormModal from '@/components/FundDocumentFormModal';
import {
  BATCHABLE_TYPES,
  COMMENT_META,
  CREATABLE_TYPE_OPTIONS,
  STATUS_OPTIONS,
  TYPE_OPTIONS,
  errText,
  fmtAmount,
  statusTag,
} from '@/components/fundDocumentsShared';
import type { CommentAction, FundDocFormValues, SharedOptions } from '@/components/fundDocumentsShared';
import { useBusinessMonth } from '@/contexts/BusinessMonthContext';
import { useOperator } from '@/contexts/OperatorContext';
import {
  addBusinessAttachment,
  approveFundDocument,
  deleteBusinessAttachment,
  getBusinessPartners,
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
  voidFundDocument,
  withdrawFundDocument,
} from '@/api';
import { FUND_DOCUMENT_TYPE_LABEL } from '@/types';
import type {
  BusinessAttachment,
  FundDocument,
  FundDocumentDetail,
  GlAccount,
} from '@/types';

const { Title, Text } = Typography;
const { TextArea } = Input;

// 模块常量（errText/fmtAmount/statusTag/STATUS_COLOR/CREATABLE_TYPE_OPTIONS/STATUS_OPTIONS/
// TYPE_OPTIONS/BATCHABLE_TYPES/ATTACHMENT_EDITABLE_STATUSES/COMMENT_META）、
// SharedOptions/FundDocFormValues 等类型已下沉至 @/components/fundDocumentsShared
// （含 JSX 的 statusTag 亦居于此 .tsx 共享文件）；附件预览弹窗随
// @/components/FundDocumentDetailDrawer 迁出，页面表格列复用同一 statusTag。

// ==================== 单据列表 Tab ====================

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
  const [editing, setEditing] = useState<FundDocument | null>(null);
  // 表单实例留在页面：openCreate/openEdit 预填初值；useWatch 随表单 JSX 在 FundDocumentFormModal 内订阅
  const [form] = Form.useForm<FundDocFormValues>();

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
        <Button type="primary" data-tour="doc-create" icon={<PlusOutlined />} onClick={openCreate}>
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

      {/* 新建/编辑弹窗：表单不含状态字段，状态完全由状态机命令流转；useWatch 与保存逻辑随组件迁出 */}
      <FundDocumentFormModal
        form={form}
        open={formOpen}
        editing={editing}
        shared={shared}
        accountOptions={accountFilterOptions}
        onClose={() => setFormOpen(false)}
        onSaved={() => {
          fetchDocs();
        }}
      />

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

      {/* 详情抽屉：单据信息 + 审批时间线 + 附件；凭证链接在凭证联动上线后展示（已抽为组件，含附件预览弹窗） */}
      <FundDocumentDetailDrawer
        open={detailOpen}
        detail={detail}
        detailLoading={detailLoading}
        attachments={attachments}
        onClose={() => setDetailOpen(false)}
        counterpartyLabel={counterpartyLabel}
        accountName={accountName}
        operatorName={operatorName}
        onUploadAttachment={handleUploadAttachment}
        onDeleteAttachment={handleDeleteAttachment}
      />
    </>
  );
};

// ==================== 页面入口 ====================

const FundDocuments: React.FC = () => {
  // 页面操作指引（第十五阶段）：递增触发 PageTour 复看
  const [tourTick, setTourTick] = useState(0);
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
        <div className="page-header-actions">
          <Button
            icon={<QuestionCircleOutlined />}
            onClick={() => setTourTick((t) => t + 1)}
          >
            指引
          </Button>
          <Button data-tour="doc-setting" icon={<SettingOutlined />} onClick={() => setSettingOpen(true)}>
            审批设置
          </Button>
        </div>
      </div>

      <PageTour
        storageKey="fund-documents"
        reopenTick={tourTick}
        steps={[
          {
            title: '收付款单',
            description:
              '除工资代发与报销外的一切资金进出都在这里立单：收款、付款、内部转账。单据是自动记账的源头——状态每前进一步，凭证与日记账自动联动。',
          },
          {
            title: '四类视图',
            description:
              '「收款单 / 付款单 / 内部转账」分类视图加「全部单据」汇总；列表上方可按单号 / 往来单位搜索。',
            target: tourTarget('.ant-tabs-nav'),
          },
          {
            title: '新建单据',
            description:
              '填账户、往来单位、金额与业务类别保存即出草稿；附件可选加密归档。草稿提交后才进入审批与结算流转。',
            target: tourTarget('[data-tour="doc-create"]'),
          },
          {
            title: '状态机与纠错',
            description:
              '流程：草稿 → 提交 → 审批 → 结算 / 进付款批次，每步留审批轨迹与操作日志。已结算单据不能改，只能冲正（红字凭证反向冲回）。',
          },
          {
            title: '经办复核',
            description:
              '「审批设置」开启后，审批人必须与提交人是不同操作人（本地署名双签语义），适合内部牵制。',
            target: tourTarget('[data-tour="doc-setting"]'),
          },
        ]}
      />
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
