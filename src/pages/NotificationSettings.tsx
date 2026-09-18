import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Alert,
  Button,
  Card,
  DatePicker,
  Form,
  Input,
  InputNumber,
  Select,
  Space,
  Table,
  Tabs,
  Tag,
  Tooltip,
  Typography,
  message,
} from 'antd';
import { MailOutlined, ReloadOutlined, SendOutlined, SoundOutlined } from '@ant-design/icons';
import dayjs from 'dayjs';
import type { Dayjs } from 'dayjs';
import { getNotificationLogs, getSmtpConfig, sendTestEmail, setSmtpConfig } from '@/api';
import type { NotificationLog, NotificationLogQuery, SmtpConfigMasked } from '@/types';
import {
  NOTIFICATION_CHANNEL_LABEL,
  NOTIFICATION_STATUS_LABEL,
  SMTP_ENCRYPTION_LABEL,
  SMTP_PRESETS,
} from '@/types';

const { Text } = Typography;

interface SmtpFormValues {
  host: string;
  port: number;
  encryption: string;
  username: string;
  /** 授权码输入值：留空表示沿用已保存授权码（不修改） */
  password?: string;
  from_name?: string;
}

const statusTagColor = (status: string): string => {
  if (status === 'sent') return 'green';
  if (status === 'skipped') return 'orange';
  return 'red';
};

const NotificationSettings: React.FC = () => {
  const [form] = Form.useForm<SmtpFormValues>();
  const [masked, setMasked] = useState<SmtpConfigMasked | null>(null);
  const [configLoading, setConfigLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);

  // 发送记录（Tab 二）
  const [logs, setLogs] = useState<NotificationLog[]>([]);
  const [logsLoading, setLogsLoading] = useState(false);
  const [logMonth, setLogMonth] = useState<Dayjs | null>(null);
  const [logStatus, setLogStatus] = useState<string | undefined>(undefined);
  const [logChannel, setLogChannel] = useState<string | undefined>(undefined);

  const fetchLogs = useCallback(async () => {
    setLogsLoading(true);
    try {
      const query: NotificationLogQuery = {
        belong_month: logMonth ? logMonth.format('YYYY-MM') : undefined,
        status: logStatus,
        channel: logChannel,
        limit: 200,
      };
      setLogs(await getNotificationLogs(query));
    } catch (e: unknown) {
      message.error('获取发送记录失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setLogsLoading(false);
    }
  }, [logMonth, logStatus, logChannel]);

  const fetchConfig = useCallback(async () => {
    setConfigLoading(true);
    try {
      const config = await getSmtpConfig();
      setMasked(config);
      if (config) {
        form.setFieldsValue({
          host: config.host,
          port: config.port,
          encryption: config.encryption,
          username: config.username,
          from_name: config.from_name,
          // 授权码不回填真实值；placeholder 展示脱敏值，留空即「不修改」
          password: '',
        });
      }
    } catch (e: unknown) {
      message.error('获取通知设置失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setConfigLoading(false);
    }
  }, [form]);

  useEffect(() => {
    void fetchConfig();
    void fetchLogs();
  }, [fetchConfig, fetchLogs]);

  const handlePreset = (key: string) => {
    const preset = SMTP_PRESETS.find((item) => item.key === key);
    if (!preset) return;
    form.setFieldsValue({ host: preset.host, port: preset.port, encryption: preset.encryption });
  };

  const handleSave = async (values: SmtpFormValues) => {
    setSaving(true);
    try {
      // 掩码回存防呆（与后端同规则）：授权码留空/保持脱敏值 → 回传脱敏值保留原授权码
      const inputPassword = values.password?.trim() ?? '';
      const saved = await setSmtpConfig({
        host: values.host.trim(),
        port: Number(values.port),
        encryption: values.encryption,
        username: values.username.trim(),
        password: inputPassword || masked?.password_masked || '****',
        from_name: values.from_name?.trim() ?? '',
      });
      message.success('通知设置已保存');
      setMasked(saved);
      form.setFieldsValue({
        host: saved.host,
        port: saved.port,
        encryption: saved.encryption,
        username: saved.username,
        from_name: saved.from_name,
        password: '',
      });
      void fetchLogs();
    } catch (e: unknown) {
      message.error('保存失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setSaving(false);
    }
  };

  const handleSendTest = async () => {
    setTesting(true);
    try {
      // 收件人=已保存的发件账号（后端 send_test_email）；测试前须先保存
      await sendTestEmail();
      message.success(`测试邮件已发送至 ${masked?.username ?? ''}，请查收（发送记录可查看）`);
      void fetchLogs();
    } catch (e: unknown) {
      message.error('测试发送失败: ' + (e instanceof Error ? e.message : String(e)));
      void fetchLogs();
    } finally {
      setTesting(false);
    }
  };

  const logChannelOptions = useMemo(
    () =>
      Object.entries(NOTIFICATION_CHANNEL_LABEL).map(([value, label]) => ({ value, label })),
    [],
  );

  const logStatusOptions = useMemo(
    () => Object.entries(NOTIFICATION_STATUS_LABEL).map(([value, label]) => ({ value, label })),
    [],
  );

  const columns = [
    {
      title: '时间',
      dataIndex: 'created_at',
      key: 'created_at',
      width: 170,
      render: (value?: string) => (value ? dayjs(value).format('YYYY-MM-DD HH:mm:ss') : '-'),
    },
    {
      title: '媒介',
      dataIndex: 'channel',
      key: 'channel',
      width: 80,
      render: (value: string) => NOTIFICATION_CHANNEL_LABEL[value] ?? value,
    },
    { title: '收件人', dataIndex: 'recipient', key: 'recipient', width: 210, ellipsis: true },
    { title: '主题', dataIndex: 'subject', key: 'subject', width: 180, ellipsis: true },
    {
      title: '账期',
      dataIndex: 'belong_month',
      key: 'belong_month',
      width: 90,
      render: (value?: string | null) => value ?? '-',
    },
    {
      title: '状态',
      dataIndex: 'status',
      key: 'status',
      width: 90,
      render: (value: string) => (
        <Tag color={statusTagColor(value)}>{NOTIFICATION_STATUS_LABEL[value] ?? value}</Tag>
      ),
    },
    {
      title: '失败原因',
      dataIndex: 'error_msg',
      key: 'error_msg',
      ellipsis: { showTitle: false },
      render: (value?: string | null) =>
        value ? (
          <Tooltip title={value} placement="topLeft">
            <Text type="danger">{value}</Text>
          </Tooltip>
        ) : (
          '-'
        ),
    },
    {
      title: '操作人',
      dataIndex: 'operator',
      key: 'operator',
      width: 100,
      render: (value?: string | null) => value ?? '-',
    },
  ];

  const smtpTab = (
    <div>
      <Card loading={configLoading}>
        <Alert
          type="info"
          showIcon
          message="授权码仅用于本机发送邮件，AES-GCM 加密存储、脱敏回显；QQ/163 邮箱须在邮箱设置中开通 SMTP 并使用「授权码」而非登录密码"
          style={{ marginBottom: 16 }}
        />
        <Form<SmtpFormValues>
          form={form}
          layout="vertical"
          style={{ maxWidth: 620 }}
          onFinish={handleSave}
        >
          <Form.Item label="服务商预设（一键填）">
            <Space wrap>
              {SMTP_PRESETS.map((preset) => (
                <Button key={preset.key} onClick={() => handlePreset(preset.key)}>
                  {preset.label}
                </Button>
              ))}
            </Space>
          </Form.Item>
          <Space wrap size={16}>
            <Form.Item
              name="host"
              label="SMTP 服务器"
              rules={[{ required: true, message: '请输入 SMTP 服务器地址' }]}
            >
              <Input placeholder="smtp.qq.com" style={{ width: 220 }} allowClear />
            </Form.Item>
            <Form.Item
              name="port"
              label="端口"
              rules={[{ required: true, message: '请输入端口' }]}
            >
              <InputNumber min={1} max={65535} style={{ width: 100 }} />
            </Form.Item>
            <Form.Item
              name="encryption"
              label="加密方式"
              rules={[{ required: true, message: '请选择加密方式' }]}
            >
              <Select
                style={{ width: 140 }}
                options={Object.entries(SMTP_ENCRYPTION_LABEL).map(([value, label]) => ({
                  value,
                  label,
                }))}
              />
            </Form.Item>
          </Space>
          <Space wrap size={16}>
            <Form.Item
              name="username"
              label="发件账号"
              rules={[{ required: true, message: '请输入发件账号（邮箱地址）' }]}
            >
              <Input
                placeholder="用于发工资条，也作为测试邮件收件人"
                style={{ width: 280 }}
                allowClear
              />
            </Form.Item>
            <Form.Item
              name="password"
              label="授权码"
              extra={
                masked
                  ? `已保存：${masked.password_masked}（留空表示不修改）`
                  : '首次配置请填写邮箱服务商提供的授权码'
              }
            >
              <Input.Password
                placeholder={masked ? masked.password_masked : '请输入授权码'}
                style={{ width: 280 }}
                autoComplete="new-password"
              />
            </Form.Item>
          </Space>
          <Form.Item name="from_name" label="发件人显示名">
            <Input placeholder="如：某某公司人事（可空）" style={{ width: 280 }} allowClear />
          </Form.Item>
          <Space>
            <Button type="primary" htmlType="submit" loading={saving}>
              保存设置
            </Button>
            <Tooltip title="使用已保存的发件账号给自己发一封测试邮件">
              <Button
                icon={<SendOutlined />}
                loading={testing}
                onClick={() => void handleSendTest()}
              >
                发测试邮件给自己
              </Button>
            </Tooltip>
          </Space>
        </Form>
      </Card>
      <Card size="small" style={{ marginTop: 16 }}>
        <Space>
          <SoundOutlined style={{ color: '#999' }} />
          <Text type="secondary">短信通道：预留接口，接入服务商后启用</Text>
        </Space>
      </Card>
    </div>
  );

  const logsTab = (
    <div>
      <Card style={{ marginBottom: 16 }}>
        <Space wrap>
          <DatePicker.MonthPicker
            value={logMonth}
            onChange={(value) => setLogMonth(value)}
            placeholder="按月份筛选"
            allowClear
          />
          <Select
            style={{ width: 140 }}
            placeholder="状态"
            allowClear
            value={logStatus}
            onChange={setLogStatus}
            options={logStatusOptions}
          />
          <Select
            style={{ width: 140 }}
            placeholder="媒介"
            allowClear
            value={logChannel}
            onChange={setLogChannel}
            options={logChannelOptions}
          />
          <Button type="primary" onClick={() => void fetchLogs()}>
            查询
          </Button>
          <Button icon={<ReloadOutlined />} onClick={() => void fetchLogs()} loading={logsLoading}>
            刷新
          </Button>
        </Space>
      </Card>
      <Card>
        <Table
          rowKey="id"
          columns={columns}
          dataSource={logs}
          loading={logsLoading}
          size="small"
          pagination={{ pageSize: 20, showSizeChanger: true, showTotal: (t) => `共 ${t} 条` }}
          scroll={{ x: 1100 }}
        />
      </Card>
    </div>
  );

  return (
    <div>
      <div className="page-header">
        <span className="page-title">
          <MailOutlined /> 通知设置
        </span>
      </div>
      <Tabs
        defaultActiveKey="smtp"
        items={[
          { key: 'smtp', label: '邮件设置（SMTP）', children: smtpTab },
          { key: 'logs', label: '发送记录', children: logsTab },
        ]}
      />
    </div>
  );
};

export default NotificationSettings;
