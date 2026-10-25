import { useState } from 'react';
import { DatePicker, Form, Input, InputNumber, Modal, Select, Space, message } from 'antd';
import type { FormInstance } from 'antd';
import dayjs from 'dayjs';
import { createFundDocument, updateFundDocument } from '@/api';
import type { FundDocument, FundDocumentInput } from '@/types';
import { CREATABLE_TYPE_OPTIONS, errText } from '@/components/fundDocumentsShared';
import type { FundDocFormValues, SharedOptions } from '@/components/fundDocumentsShared';

const { TextArea } = Input;

export interface FundDocumentFormModalProps {
  /** 表单实例由页面持有：openCreate/openEdit 预填初值后打开弹窗 */
  form: FormInstance<FundDocFormValues>;
  open: boolean;
  /** 正在编辑的草稿（null 表示新建） */
  editing: FundDocument | null;
  shared: SharedOptions;
  /** 资金账户下拉选项（与页面列表筛选共用同一份） */
  accountOptions: { value: number; label: string }[];
  /** 取消关闭 */
  onClose: () => void;
  /** 保存成功后的收尾（页面刷新列表；关闭由组件在成功后一并触发） */
  onSaved: () => void;
}

/**
 * 新建/编辑资金单据弹窗：表单不含状态字段，状态完全由状态机命令流转。
 * Form.useWatch 随表单 JSX 迁入本组件，订阅页面持有的同一表单实例；
 * 保存成功后回调 onClose + onSaved（对应原页面 setFormOpen(false) + fetchDocs()）。
 */
const FundDocumentFormModal: React.FC<FundDocumentFormModalProps> = ({
  form,
  open,
  editing,
  shared,
  accountOptions,
  onClose,
  onSaved,
}) => {
  const [saving, setSaving] = useState(false);
  const watchBelongMonth = Form.useWatch('belong_month', form);
  const watchDocType = Form.useWatch('document_type', form);
  const watchCounterpartyKind = Form.useWatch('counterparty_kind', form);

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
        // 本表单不编辑借款/核销专属字段：编辑草稿时透传原值，避免后端把
        // settlement_mode 抹成缺省 cash_return、或借款单因缺预计归还日校验失败
        // （stage7 Minor：核销草稿编辑透传 settlement_mode/due_date）
        settlement_mode: editing?.settlement_mode ?? null,
        due_date: editing?.due_date ?? null,
      };
      if (editing) {
        await updateFundDocument(data);
        message.success('单据已保存');
      } else {
        await createFundDocument(data);
        message.success('单据已创建（草稿）');
      }
      onClose();
      onSaved();
    } catch (e: unknown) {
      message.error('保存失败: ' + errText(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Modal
      title={editing ? `编辑单据 ${editing.document_no}` : '新建资金单据'}
      open={open}
      onOk={handleFormSave}
      confirmLoading={saving}
      onCancel={onClose}
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
              <Select style={{ width: 220 }} showSearch optionFilterProp="label" options={accountOptions} />
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
              <Select style={{ width: 220 }} showSearch optionFilterProp="label" options={accountOptions} />
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
  );
};

export default FundDocumentFormModal;
