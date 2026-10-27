import { useCallback, useEffect, useMemo, useState } from 'react';
import { Button, Card, DatePicker, Input, Select, Space, Table, Tag, message } from 'antd';
import { ReloadOutlined, SearchOutlined } from '@ant-design/icons';
import dayjs from 'dayjs';
import type { Dayjs } from 'dayjs';
import { queryOperationLogs } from '@/api';
import type { OperationLog, OperationLogQuery } from '@/types';

const { RangePicker } = DatePicker;

const operationTypeLabels: Record<string, string> = {
  create_employee: '新增员工',
  update_employee: '更新员工',
  delete_employee: '删除员工',
  import_employees: '导入员工',
  import_attendance: '导入考勤',
  update_salary_rule: '更新工资规则',
  update_tax_rule: '更新个税规则',
  calculate_salary: '计算工资',
  review_salary: '复核工资',
  lock_salary: '锁定工资',
  unlock_salary: '受控解锁工资',
  salary_unlock_failed: '受控解锁工资失败',
  export: '导出工资/考勤文件',
  create_expense_type: '新增费用类型',
  update_expense_type: '更新费用类型',
  delete_expense_type: '删除费用类型',
  save_invoice: '保存发票',
  update_invoice: '更新发票',
  delete_invoice: '作废发票',
  export_invoices: '导出发票清单',
  create_reimbursement: '新增报销单',
  update_reimbursement: '更新报销单',
  submit_reimbursement: '提交报销单',
  approve_reimbursement: '审批通过报销单',
  reject_reimbursement: '驳回报销单',
  withdraw_reimbursement: '撤回报销单',
  unapprove_reimbursement: '报销反审批',
  // 历史直写状态通道已退役（Task 15），标签仅为旧日志可读性保留
  update_reimbursement_status: '更新报销状态',
  delete_reimbursement: '作废报销单',
  export_department_cost_report: '导出部门成本表',
  export_expense_analysis_report: '导出费用分析表',
  export_month_close_report: '导出月结报告',
  create_payment_batch: '生成付款批次',
  export_payment_batch: '导出付款批次',
  mark_payment_batch_paid: '标记批次付款',
  void_payment_batch: '作废付款批次',
  update_payment_batch_remark: '更新批次备注',
  import_bank_transactions: '导入银行流水',
  auto_match_bank_transactions: '自动匹配银行流水',
  confirm_bank_transaction_match: '确认流水匹配',
  cancel_bank_transaction_match: '取消流水匹配',
  ignore_bank_transaction: '忽略银行流水',
  save_budget: '保存预算',
  delete_budget: '删除预算',
  save_opening_balances: '保存期初余额',
  create_bank_manual_voucher: '银行流水生成凭证',
  export_financial_report: '导出财务报表',
  export_trial_balance: '导出科目余额表',
  export_annual_tax_summary: '导出个税年度汇总',
  export_tax_withholding_declaration: '导出个税扣缴申报表',
  set_reminder_advance_days: '设置账期提醒提前天数',
  upsert_salary_rule_key: '保存全局社保份额规则',
  period_close_vouchers: '年末结转凭证',
  save_social_profile: '保存社保台账',
  delete_social_profile: '删除社保台账',
  copy_social_profiles: '年度调基',
  set_social_base_limits: '保存基数上下限',
  // 第七阶段出纳命令（与 src-tauri/src/commands.rs 实名一一对应）
  get_fund_accounts: '查询资金账户',
  save_fund_account: '保存资金账户',
  set_active_fund_account: '启停资金账户',
  get_business_partners: '查询往来单位',
  save_business_partner: '保存往来单位',
  set_active_business_partner: '启停往来单位',
  get_operator_profiles: '查询操作人',
  save_operator_profile: '保存操作人',
  set_active_operator_profile: '启停操作人',
  set_current_operator: '切换当前操作人',
  get_current_operator: '查询当前操作人',
  add_business_attachment: '上传业务附件',
  delete_business_attachment: '删除业务附件',
  set_maker_checker_enabled: '设置经办复核开关',
  create_fund_document: '新建资金单据',
  update_fund_document: '修改资金单据',
  submit_fund_document: '提交资金单据',
  approve_fund_document: '审批通过资金单据',
  reject_fund_document: '驳回资金单据',
  withdraw_fund_document: '撤回资金单据',
  void_fund_document: '作废资金单据',
  settle_fund_document: '结算资金单据',
  reverse_fund_document: '冲正资金单据',
  apply_fund_assignment: '历史资金归集',
  preview_bank_allocation_candidates: '预览流水核销候选',
  preview_bank_auto_matches: '预览自动匹配',
  confirm_bank_allocations: '确认流水核销',
  cancel_bank_allocation: '取消流水核销',
  list_bank_allocations: '查询流水核销明细',
  batch_confirm_bank_auto_matches: '自动匹配批量核销',
  migrate_legacy_bank_matches: '旧银行匹配迁移',
  cancel_advance_settlement_link: '取消借款核销',
  export_advance_ledger: '导出借款台账',
  // 第八阶段票据台账命令（与 src-tauri/src/commands.rs 实名一一对应；get 类不落日志，映射仅兜底）
  get_negotiable_instruments: '查询票据台账',
  get_instrument_detail: '查询票据详情',
  register_instrument: '登记票据',
  endorse_instrument: '背书转出票据',
  discount_instrument: '票据贴现',
  start_collection: '发起票据托收',
  confirm_collection: '托收到账确认',
  settle_issued_instrument: '兑付开出票据',
  void_instrument: '作废票据',
  reverse_instrument_flow: '红字冲正票据',
  // 第八阶段现金盘点命令（与 src-tauri/src/commands.rs 实名一一对应；get 类不落日志，映射仅兜底）
  get_count_sheets: '查询现金盘点单',
  get_count_sheet_detail: '查询现金盘点详情',
  create_count_sheet: '新建现金盘点单',
  update_count_sheet: '修改现金盘点单',
  confirm_count_sheet: '确认现金盘点单',
  void_count_sheet: '作废现金盘点单',
  // 第八阶段资金日报命令（与 src-tauri/src/commands.rs 实名一一对应；get 类不落日志，映射仅兜底）
  get_fund_daily_report: '查询资金日报',
  export_fund_daily_report: '导出资金日报',
  // 第八阶段进项台账命令（与 src-tauri/src/commands.rs 实名一一对应；get 类不落日志，映射仅兜底）
  get_input_tax_ledger: '查询进项台账',
  export_input_tax_ledger: '导出进项台账',
  generate_bank_reconciliation_period: '生成余额调节表',
  confirm_bank_reconciliation_period: '确认余额调节表',
  export_bank_reconciliation_period: '导出余额调节表',
  export_fund_journal: '导出资金日记账',
  export_month_close_package: '导出月结报告包',
  // 月结与数据安全（第三/六阶段既有命令补齐中文映射）
  close_month: '正式月结',
  reopen_month: '反月结',
  backup_database: '备份数据库',
  restore_database: '恢复数据库',
  compact_database: '压缩整理数据库',
  verify_database: '数据库体检',
  // 第九阶段通知设置命令（与 src-tauri/src/commands.rs 实名一一对应；get 不落日志）
  smtp_config_update: '更新 SMTP 通知配置',
  // 第九阶段工资条邮件（notification.rs 落 operation_logs：发起批量/重发；preview 只读不落日志）
  payslip_email_batch_send: '发起工资条邮件批量发送',
  payslip_email_resend: '重发工资条邮件',
  // 安全事件（security_commands.rs：解锁/锁屏/敏感数据/改密/找回/换绑，操作人固定 security）
  unlock_success: '密码解锁成功',
  unlock_failed: '密码解锁失败',
  lock: '手动锁屏',
  reveal_sensitive: '敏感数据解锁',
  reveal_sensitive_failed: '敏感数据解锁失败',
  change_password: '修改启动密码',
  change_password_failed: '修改启动密码失败',
  reset_by_recovery: '恢复码重置密码',
  reset_by_recovery_failed: '恢复码重置密码失败',
  reset_by_question: '安全问题重置密码',
  reset_by_question_failed: '安全问题重置密码失败',
  security_recovery_code_rotated: '换绑恢复码',
  security_recovery_code_rotated_failed: '换绑恢复码失败',
  security_question_updated: '修改安全问题',
  security_question_updated_failed: '修改安全问题失败',
  // 自动备份与数据安全（data_safety.rs / commands.rs）
  auto_backup_created: '自动备份',
  auto_backup_pruned: '自动备份滚动清理',
  update_auto_backup_settings: '更新自动备份设置',
  // 上手引导与企业信息（onboarding.rs / commands.rs）
  seed_demo_data: '载入示例账套',
  clear_demo_data: '清除示例账套',
  save_company_info: '保存企业信息',
  // 征期日历与本地通知（commands.rs）
  save_tax_calendar_settings: '更新征期提醒设置',
  complete_tax_filing: '标记征期申报完成',
  undo_tax_filing: '撤销征期完成标记',
  save_local_notify_settings: '更新本地通知设置',
  // 固定资产（assets.rs：登记/折旧/处置/台账导出）
  save_fixed_asset: '登记固定资产',
  run_asset_depreciation: '计提折旧',
  dispose_fixed_asset: '固定资产处置',
  export_fixed_asset_ledger: '导出固定资产台账',
  // 手工凭证（commands.rs：新增/作废）
  create_manual_voucher: '新增手工凭证',
  void_manual_voucher: '作废手工凭证',
  // 应收核销与销项/账龄导出（ar.rs / commands.rs）
  create_ar_allocation: '应收核销',
  cancel_ar_allocation: '取消应收核销',
  export_ar_aging_report: '导出应收账龄表',
  export_output_tax_ledger: '导出销项台账',
  // OCR 批次确认（ocr.rs）
  ocr_confirm: '确认 OCR 识别批次',
  // 申报模板与增值税申报辅助（commands.rs，第十/十六阶段）
  export_eit_payer_template: '导出人员信息采集模板',
  export_eit_salary_template: '导出工资薪金申报模板',
  export_social_base_declaration: '导出社保基数申报表',
  export_vat_quarterly_report: '导出增值税申报辅助表',
};

const getOperationLabel = (value?: string) =>
  value ? operationTypeLabels[value] ?? value : '-';

const OperationLogs: React.FC = () => {
  const [logs, setLogs] = useState<OperationLog[]>([]);
  const [loading, setLoading] = useState(false);
  const [operationType, setOperationType] = useState<string | undefined>(undefined);
  const [operatorFilter, setOperatorFilter] = useState<string | undefined>(undefined);
  const [keyword, setKeyword] = useState('');
  const [range, setRange] = useState<[Dayjs, Dayjs] | null>(null);

  const operationOptions = useMemo(() => {
    const values = Array.from(new Set(logs.map((log) => log.operation_type))).filter(Boolean);
    return values.map((value) => ({ value, label: getOperationLabel(value) }));
  }, [logs]);

  // 操作人下拉选项从当前已加载结果收集；筛选本身由后端 operator 参数精确过滤
  // （Task 4 挂账承接：替代原前端过滤，消除 limit 截断盲区）。
  const operatorOptions = useMemo(() => {
    const values = Array.from(new Set(logs.map((log) => log.operator).filter(Boolean))) as string[];
    return values.map((value) => ({ value, label: value }));
  }, [logs]);

  const fetchData = useCallback(async () => {
    setLoading(true);
    try {
      const query: OperationLogQuery = {
        operation_type: operationType,
        operator: operatorFilter,
        keyword: keyword || undefined,
        start_date: range?.[0]?.startOf('day').toISOString(),
        end_date: range?.[1]?.endOf('day').toISOString(),
        limit: 300,
      };
      setLogs(await queryOperationLogs(query));
    } catch (e: unknown) {
      message.error('获取操作日志失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setLoading(false);
    }
  }, [operationType, operatorFilter, keyword, range]);

  useEffect(() => {
    fetchData();
  }, [fetchData]);

  const columns = [
    {
      title: '时间',
      dataIndex: 'created_at',
      key: 'created_at',
      width: 190,
      render: (value?: string) => (value ? dayjs(value).format('YYYY-MM-DD HH:mm:ss') : '-'),
    },
    {
      title: '操作类型',
      dataIndex: 'operation_type',
      key: 'operation_type',
      width: 190,
      render: (value: string) => <Tag color="blue">{getOperationLabel(value)}</Tag>,
    },
    { title: '说明', dataIndex: 'description', key: 'description', width: 360, ellipsis: true },
    { title: '操作人', dataIndex: 'operator', key: 'operator', width: 110 },
    { title: '详情', dataIndex: 'detail', key: 'detail', ellipsis: true },
  ];

  return (
    <div>
      <div className="page-header">
        <span className="page-title">操作日志</span>
        <Button icon={<ReloadOutlined />} onClick={fetchData} loading={loading}>
          刷新
        </Button>
      </div>

      <Card style={{ marginBottom: 16 }}>
        <Space wrap>
          <Select
            style={{ width: 220 }}
            allowClear
            showSearch
            placeholder="操作类型"
            value={operationType}
            onChange={setOperationType}
            options={operationOptions}
          />
          <Select
            style={{ width: 160 }}
            allowClear
            showSearch
            placeholder="操作人"
            value={operatorFilter}
            onChange={setOperatorFilter}
            options={operatorOptions}
          />
          <RangePicker value={range} onChange={(value) => setRange(value as [Dayjs, Dayjs] | null)} />
          <Input.Search
            style={{ width: 280 }}
            allowClear
            prefix={<SearchOutlined />}
            placeholder="搜索说明/详情/操作人"
            value={keyword}
            onChange={(e) => setKeyword(e.target.value)}
            onSearch={fetchData}
          />
          <Button type="primary" onClick={fetchData}>查询</Button>
        </Space>
      </Card>

      <Card>
        <Table
          rowKey="id"
          columns={columns}
          dataSource={logs}
          loading={loading}
          size="small"
          pagination={{ pageSize: 30, showSizeChanger: true, showTotal: (t) => `共 ${t} 条` }}
          scroll={{ x: 1100 }}
        />
      </Card>
    </div>
  );
};

export default OperationLogs;
