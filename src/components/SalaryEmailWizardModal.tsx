import { useCallback, useEffect, useState } from 'react';
import {
  Table, Button, Modal, message, Alert, Steps, Select, Tooltip, Space,
} from 'antd';
import { MailOutlined, ReloadOutlined } from '@ant-design/icons';
import { useNavigate } from 'react-router';
import {
  getEmployees, getSmtpConfig, previewPayslipEmail, sendPayslipEmails, resendPayslipEmails, getNotificationLogs,
} from '@/api';
import type { BatchSummary, Employee, NotificationLog } from '@/types';
import { useSecurity } from '@/contexts/SecurityContext';

interface SalaryEmailWizardModalProps {
  /** 外层请求打开向导（点击「邮件发送工资条」按钮后置 true） */
  open: boolean;
  /** 业务月份，格式 YYYY-MM */
  monthStr: string;
  /** 当月有工资结果的员工工号集合，仅这些员工进入向导候选列表 */
  resultEmployeeNos: Set<string>;
  /** 请求关闭向导（取消 / 关闭 / 跳转发送记录 / 入口校验未通过时由组件内部触发） */
  onClose: () => void;
}

// 工资条邮件三步向导（第九阶段 Task 6）：①选员工 ②预览 ③发送结果。
// 入口 SMTP 校验（spec 7：配置缺失不进向导）在组件内完成：外层 open 置 true 后先校验，
// 通过才展示向导并加载员工，未通过弹确认框引导去通知设置，与原先「按钮点击 → 校验 → 打开」一致。
export function SalaryEmailWizardModal({ open, monthStr, resultEmployeeNos, onClose }: SalaryEmailWizardModalProps) {
  const [emailStep, setEmailStep] = useState(0);
  const [emailEmployees, setEmailEmployees] = useState<Employee[]>([]);
  const [emailSelectedIds, setEmailSelectedIds] = useState<number[]>([]);
  const [emailLoading, setEmailLoading] = useState(false);
  const [previewEmployeeId, setPreviewEmployeeId] = useState<number | null>(null);
  const [previewHtml, setPreviewHtml] = useState('');
  const [previewLoading, setPreviewLoading] = useState(false);
  const [sending, setSending] = useState(false);
  const [sendError, setSendError] = useState('');
  const [summary, setSummary] = useState<BatchSummary | null>(null);
  const [failedLogs, setFailedLogs] = useState<NotificationLog[]>([]);
  const [resendIds, setResendIds] = useState<number[]>([]);
  const [resending, setResending] = useState(false);
  // SMTP 入口校验通过后才置 true 真正展示向导；关闭时复位，下次打开重新校验
  const [wizardVisible, setWizardVisible] = useState(false);

  const { isSensitiveRevealed } = useSecurity();
  const navigate = useNavigate();

  const emailMissing = emailEmployees.filter((e) => !(e.email ?? '').trim());
  const emailEmployeeName = useCallback(
    (log: NotificationLog) =>
      emailEmployees.find((e) => e.id === log.employee_id)?.name ??
      `员工#${log.employee_id ?? '?'}`,
    [emailEmployees]
  );

  const resetEmailWizard = () => {
    setEmailStep(0);
    setEmailEmployees([]);
    setEmailSelectedIds([]);
    setPreviewEmployeeId(null);
    setPreviewHtml('');
    setSending(false);
    setSendError('');
    setSummary(null);
    setFailedLogs([]);
    setResendIds([]);
  };

  // 进入向导前查 SMTP 配置（spec 7：配置缺失入口拦截，不进向导）。
  // 仅列当月有工资结果的员工（Task 5 挂账：避免无结果整体报错）。
  // 工资结果行不带 employee_id，按 employee_no 与员工表关联；发送传 employees.id。
  useEffect(() => {
    if (!open) {
      setWizardVisible(false);
      return;
    }
    (async () => {
      let configured: boolean;
      try {
        configured = Boolean(await getSmtpConfig());
      } catch (e: unknown) {
        message.error('获取通知设置失败: ' + (e instanceof Error ? e.message : String(e)));
        onClose();
        return;
      }
      if (!configured) {
        Modal.confirm({
          title: '尚未配置 SMTP 邮箱',
          content: '发送工资条邮件前，请先在「系统设置 → 通知设置」完成 SMTP 配置并测试发送。',
          okText: '前往通知设置',
          cancelText: '取消',
          onOk: () => navigate('/notification-settings'),
        });
        onClose();
        return;
      }
      resetEmailWizard();
      setWizardVisible(true);
      setEmailLoading(true);
      try {
        const employees = await getEmployees();
        const withResult = employees.filter((e) => resultEmployeeNos.has(e.employee_no));
        setEmailEmployees(withResult);
        setEmailSelectedIds(withResult.filter((e) => (e.email ?? '').trim()).map((e) => e.id));
      } catch (e: unknown) {
        message.error('获取员工列表失败: ' + (e instanceof Error ? e.message : String(e)));
        onClose();
      } finally {
        setEmailLoading(false);
      }
    })();
    // 入口校验仅由「打开」动作触发一次；onClose / resultEmployeeNos 变化不应重跑校验
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  // 预览：敏感未解锁时后端拒绝返回明文，前端直接不请求并提示
  useEffect(() => {
    if (!wizardVisible || emailStep !== 1 || previewEmployeeId == null) return;
    if (!isSensitiveRevealed) {
      setPreviewHtml('');
      return;
    }
    let cancelled = false;
    setPreviewLoading(true);
    previewPayslipEmail(monthStr, previewEmployeeId)
      .then((html) => {
        if (!cancelled) setPreviewHtml(html);
      })
      .catch((e: unknown) => {
        if (!cancelled) {
          message.error('获取邮件预览失败: ' + (e instanceof Error ? e.message : String(e)));
          setPreviewHtml('');
        }
      })
      .finally(() => {
        if (!cancelled) setPreviewLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [wizardVisible, emailStep, previewEmployeeId, isSensitiveRevealed, monthStr]);

  const handleEmailStepNext = () => {
    if (emailSelectedIds.length === 0) {
      message.warning('请先勾选要发送工资条的员工');
      return;
    }
    setPreviewEmployeeId(emailSelectedIds[0]);
    setEmailStep(1);
  };

  // 按最新 summary 拉取失败留痕（log.id ∈ failed_log_ids），供勾选重发
  const refreshFailedLogs = async (ids: number[]) => {
    if (ids.length === 0) {
      setFailedLogs([]);
      setResendIds([]);
      return;
    }
    try {
      const logs = await getNotificationLogs({ belong_month: monthStr, channel: 'email', limit: 200 });
      const idSet = new Set(ids);
      setFailedLogs(logs.filter((l) => idSet.has(l.id)));
    } catch (e: unknown) {
      message.error('获取发送记录失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setResendIds([]);
    }
  };

  // 一次调用整批发送，后端逐封处理并留痕（spec 5 步骤 3）
  const handleSendPayslipEmails = async () => {
    setSending(true);
    setSendError('');
    setEmailStep(2);
    try {
      const result = await sendPayslipEmails(monthStr, emailSelectedIds);
      setSummary(result);
      await refreshFailedLogs(result.failed_log_ids);
    } catch (e: unknown) {
      setSendError(e instanceof Error ? e.message : String(e));
      setSummary(null);
    } finally {
      setSending(false);
    }
  };

  // 失败勾选重发（重发仅失败者，spec 5 步骤 3）
  const handleResendFailed = async () => {
    if (resendIds.length === 0) {
      message.warning('请先勾选要重发的失败记录');
      return;
    }
    setResending(true);
    try {
      const result = await resendPayslipEmails(monthStr, resendIds);
      setSummary((prev) =>
        prev
          ? {
              sent: prev.sent + result.sent,
              failed: result.failed,
              skipped: prev.skipped + result.skipped,
              failed_log_ids: result.failed_log_ids,
            }
          : result
      );
      if (result.sent > 0) {
        message.success(`重发完成：成功 ${result.sent} 封`);
      }
      await refreshFailedLogs(result.failed_log_ids);
    } catch (e: unknown) {
      message.error('重发失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setResending(false);
    }
  };

  const failedLogColumns = [
    { title: '员工', key: 'employee', width: 100, render: (_: unknown, log: NotificationLog) => emailEmployeeName(log) },
    { title: '收件邮箱', dataIndex: 'recipient', key: 'recipient', width: 220, ellipsis: true },
    {
      title: '失败原因',
      dataIndex: 'error_msg',
      key: 'error_msg',
      render: (v?: string | null) => v ?? '-',
    },
  ];

  return (
    <Modal
      open={wizardVisible}
      onCancel={onClose}
      title={
        <span>
          <MailOutlined /> 邮件发送工资条 - {monthStr}
        </span>
      }
      width={760}
      destroyOnHidden
      mask={{ closable: false }}
      footer={
        emailStep === 0
          ? [
              <Button key="cancel" onClick={onClose}>
                取消
              </Button>,
              <Button
                key="next"
                type="primary"
                loading={emailLoading}
                disabled={emailSelectedIds.length === 0}
                onClick={handleEmailStepNext}
              >
                下一步（预览）
              </Button>,
            ]
          : emailStep === 1
            ? [
                <Button key="back" disabled={sending} onClick={() => setEmailStep(0)}>
                  上一步
                </Button>,
                <Button key="cancel" disabled={sending} onClick={onClose}>
                  取消
                </Button>,
                <Tooltip
                  key="send"
                  title={
                    isSensitiveRevealed
                      ? ''
                      : '工资明细为明文，请先点击任意金额眼睛解锁敏感数据'
                  }
                >
                  <span>
                    <Button
                      type="primary"
                      icon={<MailOutlined />}
                      disabled={!isSensitiveRevealed || emailSelectedIds.length === 0}
                      onClick={handleSendPayslipEmails}
                    >
                      发送给 {emailSelectedIds.length} 位员工
                    </Button>
                  </span>
                </Tooltip>,
              ]
            : [
                <Button key="close" type="primary" onClick={onClose}>
                  关闭
                </Button>,
              ]
      }
    >
      <Steps
        size="small"
        current={emailStep}
        style={{ marginBottom: 16 }}
        items={[{ title: '选择员工' }, { title: '预览确认' }, { title: '发送结果' }]}
      />

      {emailStep === 0 && (
        <div>
          <Table
            rowKey="id"
            size="small"
            loading={emailLoading}
            dataSource={emailEmployees}
            rowSelection={{
              selectedRowKeys: emailSelectedIds,
              onChange: (keys) => setEmailSelectedIds(keys as number[]),
              getCheckboxProps: (record) => ({ disabled: !(record.email ?? '').trim() }),
            }}
            pagination={false}
            scroll={{ y: 280 }}
            columns={[
              { title: '工号', dataIndex: 'employee_no', key: 'employee_no', width: 90 },
              { title: '姓名', dataIndex: 'name', key: 'name', width: 90 },
              { title: '部门', dataIndex: 'department', key: 'department', width: 100 },
              {
                title: '邮箱',
                dataIndex: 'email',
                key: 'email',
                render: (v: string) => v || <span style={{ color: '#bbb' }}>未填写</span>,
              },
            ]}
          />
          {emailEmployees.length === 0 && !emailLoading && (
            <Alert
              type="info"
              showIcon
              style={{ marginTop: 12 }}
              message="当月没有可发送的工资结果员工"
            />
          )}
          {emailMissing.length > 0 && (
            <Alert
              type="warning"
              showIcon
              style={{ marginTop: 12 }}
              message={`缺邮箱 ${emailMissing.length} 人：${emailMissing
                .map((e) => e.name)
                .join('、')}`}
              description="这些员工将无法接收工资条邮件；可到「员工管理」补充邮箱后重新打开向导。"
            />
          )}
          <p style={{ marginTop: 12, marginBottom: 0, color: '#999', fontSize: 12 }}>
            已选 {emailSelectedIds.length} / {emailEmployees.length} 人（默认勾选全部有邮箱员工）
          </p>
        </div>
      )}

      {emailStep === 1 && (
        <div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 12 }}>
            <span style={{ flexShrink: 0 }}>预览员工：</span>
            <Select
              style={{ width: 240 }}
              value={previewEmployeeId ?? undefined}
              onChange={(v) => setPreviewEmployeeId(v)}
              options={emailEmployees
                .filter((e) => emailSelectedIds.includes(e.id))
                .map((e) => ({ value: e.id, label: `${e.name}（${e.employee_no}）` }))}
            />
            <span style={{ color: '#999', fontSize: 12 }}>
              将发送 {emailSelectedIds.length} 封（其余员工邮件内容结构相同）
            </span>
          </div>
          {!isSensitiveRevealed && (
            <Alert
              type="warning"
              showIcon
              style={{ marginBottom: 12 }}
              message="工资明细为明文，请先点击任意金额眼睛解锁敏感数据"
              description="解锁后才能预览邮件正文并执行发送。"
            />
          )}
          {isSensitiveRevealed && (
            <div style={{ border: '1px solid #eee', borderRadius: 4, padding: 12, maxHeight: 360, overflowY: 'auto' }}>
              {previewLoading ? (
                <p style={{ color: '#999', textAlign: 'center' }}>预览加载中…</p>
              ) : previewHtml ? (
                // 邮件 HTML 由本域命令 preview_payslip_email 组装，金额经后端渲染，内容可信
                <div dangerouslySetInnerHTML={{ __html: previewHtml }} />
              ) : (
                <p style={{ color: '#999', textAlign: 'center' }}>暂无预览</p>
              )}
            </div>
          )}
        </div>
      )}

      {emailStep === 2 && (
        <div>
          {sending && (
            <Alert
              type="info"
              showIcon
              message={`正在发送 ${emailSelectedIds.length} 封工资条邮件…`}
              description="逐封发送中，请勿关闭窗口；完成后展示汇总结果。"
            />
          )}
          {!sending && sendError && (
            <Alert
              type="error"
              showIcon
              message="发送失败"
              description={
                <Space direction="vertical">
                  <span>{sendError}</span>
                  <Button size="small" onClick={() => setEmailStep(1)}>
                    返回上一步重试
                  </Button>
                </Space>
              }
            />
          )}
          {!sending && !sendError && summary && (
            <div>
              <Space size={24} style={{ marginBottom: 12 }}>
                <span>
                  成功 <strong style={{ color: '#52c41a' }}>{summary.sent}</strong> 封
                </span>
                <span>
                  失败 <strong style={{ color: summary.failed > 0 ? '#cf1322' : undefined }}>{summary.failed}</strong> 封
                </span>
                <span>
                  跳过 <strong>{summary.skipped}</strong> 封（无邮箱）
                </span>
                <Button
                  type="link"
                  size="small"
                  style={{ padding: 0 }}
                  onClick={() => {
                    onClose();
                    navigate('/notification-settings');
                  }}
                >
                  查看发送记录
                </Button>
              </Space>
              {summary.failed > 0 && (
                <div>
                  <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 8 }}>
                    <span>失败记录（可勾选重发，重发仅针对失败者）</span>
                    <Button
                      size="small"
                      icon={<ReloadOutlined />}
                      loading={resending}
                      disabled={resendIds.length === 0}
                      onClick={handleResendFailed}
                    >
                      重发勾选项（{resendIds.length}）
                    </Button>
                  </div>
                  <Table
                    rowKey="id"
                    size="small"
                    dataSource={failedLogs}
                    columns={failedLogColumns}
                    pagination={false}
                    rowSelection={{
                      selectedRowKeys: resendIds,
                      onChange: (keys) => setResendIds(keys as number[]),
                    }}
                  />
                </div>
              )}
              {summary.failed === 0 && (
                <Alert type="success" showIcon message="全部发送成功，发送记录已留痕（通知设置 → 发送记录）" />
              )}
            </div>
          )}
        </div>
      )}
    </Modal>
  );
}

export default SalaryEmailWizardModal;
