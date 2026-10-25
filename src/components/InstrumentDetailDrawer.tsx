// 票据详情抽屉：票面信息 + 背书链时间线 + 状态轨迹（含关联凭证号）
// 自 src/pages/NotesInstruments.tsx 的 InstrumentTab 抽取（纯搬家）；
// 详情/事件数据与加载逻辑留在页面层，本组件仅负责展示。
import { Descriptions, Drawer, Empty, Timeline, Typography } from 'antd';
import dayjs from 'dayjs';
import SensitiveText from '@/components/SensitiveText';
import {
  APPROVAL_ACTION_LABEL,
  INSTRUMENT_ACTION_LABEL,
  INSTRUMENT_STATUS_LABEL,
  INSTRUMENT_TYPE_LABEL,
} from '@/types';
import type {
  ApprovalEvent,
  InstrumentDetail,
  InstrumentEndorsement,
} from '@/types';
import {
  accountName,
  directionTag,
  fmtAmount,
  statusTag,
} from '@/components/instrumentsShared';
import type { SharedOptions } from '@/components/instrumentsShared';

const { Title, Text } = Typography;

interface InstrumentDetailDrawerProps {
  open: boolean;
  loading: boolean;
  detail: InstrumentDetail | null;
  events: ApprovalEvent[];
  shared: SharedOptions;
  onClose: () => void;
}

const InstrumentDetailDrawer: React.FC<InstrumentDetailDrawerProps> = ({
  open,
  loading,
  detail,
  events,
  shared,
  onClose,
}) => (
  <Drawer
    title={detail ? `票据详情 ${detail.instrument.instrument_no}` : '票据详情'}
    open={open}
    width={680}
    onClose={onClose}
    destroyOnHidden
  >
    {detail === null ? (
      <Empty description={loading ? '加载中…' : '暂无数据'} />
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
);

export default InstrumentDetailDrawer;
