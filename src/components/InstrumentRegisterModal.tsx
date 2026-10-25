// 登记票据弹窗：按类型+方向动态约束（承兑类收方向免账户、支票必选账户）
// 自 src/pages/NotesInstruments.tsx 的 InstrumentTab 抽取（纯搬家）；
// 表单实例与提交逻辑留在页面层，watchRegType/watchRegDir 及派生布尔随 JSX 移入本组件。
import { DatePicker, Form, Input, InputNumber, Modal, Select, Space } from 'antd';
import type { FormInstance } from 'antd';
import type { Dayjs } from 'dayjs';
import { INSTRUMENT_DIRECTION_LABEL } from '@/types';
import { TYPE_OPTIONS, accountSelectOptions } from '@/components/instrumentsShared';
import type { RegisterFormValues, SharedOptions } from '@/components/instrumentsShared';

const { TextArea } = Input;

interface InstrumentRegisterModalProps {
  open: boolean;
  saving: boolean;
  form: FormInstance<RegisterFormValues>;
  shared: SharedOptions;
  onOk: () => void;
  onCancel: () => void;
}

const InstrumentRegisterModal: React.FC<InstrumentRegisterModalProps> = ({
  open,
  saving,
  form,
  shared,
  onOk,
  onCancel,
}) => {
  // 类型/方向联动 watch（原 InstrumentTab 内定义，随弹窗 JSX 一并移入）
  const watchRegType = Form.useWatch('instrument_type', form);
  const watchRegDir = Form.useWatch('direction', form);
  const regIsCheck = watchRegType === 'check';
  const regIsAcceptance = watchRegType === 'bank_acceptance' || watchRegType === 'commercial_acceptance';

  const accOptions = accountSelectOptions(shared.accounts);

  return (
    <Modal
      title="登记票据"
      open={open}
      onOk={onOk}
      confirmLoading={saving}
      okText="登记"
      onCancel={onCancel}
      width={680}
      destroyOnHidden
    >
      <Form form={form} layout="vertical">
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
                form.setFieldsValue({ fund_account_id: undefined, acceptor: undefined });
                if (form.getFieldValue('instrument_type') === 'check') {
                  const issue = form.getFieldValue('issue_date') as Dayjs | undefined;
                  if (issue) form.setFieldsValue({ due_date: issue });
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
                if (value && form.getFieldValue('instrument_type') === 'check') {
                  form.setFieldsValue({ due_date: value });
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
  );
};

export default InstrumentRegisterModal;
