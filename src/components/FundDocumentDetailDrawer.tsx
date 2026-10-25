import { useEffect, useState } from 'react';
import {
  Alert,
  Button,
  Descriptions,
  Drawer,
  Empty,
  Image,
  List,
  Modal,
  Popconfirm,
  Timeline,
  Typography,
} from 'antd';
import { UploadOutlined } from '@ant-design/icons';
import { convertFileSrc } from '@tauri-apps/api/core';
import dayjs from 'dayjs';
import SensitiveText from '@/components/SensitiveText';
import { getDecryptedAttachmentUrl } from '@/api';
import {
  APPROVAL_ACTION_LABEL,
  FUND_DOCUMENT_STATUS_LABEL,
  FUND_DOCUMENT_TYPE_LABEL,
  SETTLEMENT_MODE_LABEL,
} from '@/types';
import type {
  ApprovalEvent,
  BusinessAttachment,
  FundDocument,
  FundDocumentDetail,
} from '@/types';
import {
  ATTACHMENT_EDITABLE_STATUSES,
  errText,
  fmtAmount,
  statusTag,
} from '@/components/fundDocumentsShared';

const { Title, Text } = Typography;

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

// ==================== 单据详情抽屉 ====================

export interface FundDocumentDetailDrawerProps {
  /** 抽屉开合 */
  open: boolean;
  /** 单据详情（null 表示尚未加载） */
  detail: FundDocumentDetail | null;
  detailLoading: boolean;
  /** 附件列表（由页面加载并维护） */
  attachments: BusinessAttachment[];
  onClose: () => void;
  /** 往来对象名称（页面按基础资料解析） */
  counterpartyLabel: (doc: FundDocument) => string;
  /** 资金账户名称（页面按基础资料解析） */
  accountName: (id: number | null) => string;
  /** 操作人名称（页面按基础资料解析） */
  operatorName: (id: number | null) => string;
  /** 上传附件（页面处理：弹文件选择框并写库） */
  onUploadAttachment: () => void;
  /** 删除附件（页面处理） */
  onDeleteAttachment: (att: BusinessAttachment) => void;
}

/**
 * 单据详情抽屉：单据信息 + 审批时间线 + 附件；凭证链接在凭证联动上线后展示。
 * 附件预览弹窗（previewAtt 状态）仅服务本抽屉，随迁至此；
 * 往来对象/账户/操作人名称与附件增删由页面通过 props 提供。
 */
const FundDocumentDetailDrawer: React.FC<FundDocumentDetailDrawerProps> = ({
  open,
  detail,
  detailLoading,
  attachments,
  onClose,
  counterpartyLabel,
  accountName,
  operatorName,
  onUploadAttachment,
  onDeleteAttachment,
}) => {
  // 仅服务附件预览弹窗的状态
  const [previewAtt, setPreviewAtt] = useState<BusinessAttachment | null>(null);

  // 附件可变更状态（与后端 FUND_DOCUMENT_ATTACHMENT_EDITABLE_STATUSES 一致）
  const detailEditable =
    detail !== null && ATTACHMENT_EDITABLE_STATUSES.includes(detail.document.status);

  return (
    <>
      <Drawer
        title={detail ? `单据详情 ${detail.document.document_no}` : '单据详情'}
        open={open}
        width={640}
        onClose={onClose}
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
                ...(detail.document.document_type === 'advance_settlement'
                  ? [
                      {
                        key: 'settlement_mode',
                        label: '核销方式',
                        children: detail.document.settlement_mode
                          ? SETTLEMENT_MODE_LABEL[detail.document.settlement_mode] ??
                            detail.document.settlement_mode
                          : '-',
                      },
                    ]
                  : []),
                ...(detail.document.document_type === 'advance'
                  ? [{ key: 'due_date', label: '预计归还日', children: detail.document.due_date ?? '-' }]
                  : []),
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
                onClick={onUploadAttachment}
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
                            onConfirm={() => void onDeleteAttachment(att)}
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

export default FundDocumentDetailDrawer;
