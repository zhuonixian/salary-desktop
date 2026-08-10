import { useEffect, useMemo, useState } from 'react';
import {
  Alert,
  Button,
  Card,
  Col,
  Form,
  Input,
  Radio,
  Row,
  Space,
  Statistic,
  Switch,
  Tag,
  Typography,
  message,
} from 'antd';
import { LockOutlined, ReloadOutlined } from '@ant-design/icons';
import dayjs from 'dayjs';
import { useSecurity } from '@/contexts/SecurityContext';

const { Text } = Typography;

// 时长选项：与 SecurityContext 默认值和 brief 中给定的 1/5/15/30 分钟对齐。
const DURATION_OPTIONS: Array<{ label: string; value: number }> = [
  { label: '1 分钟', value: 60 },
  { label: '5 分钟', value: 300 },
  { label: '15 分钟', value: 900 },
  { label: '30 分钟', value: 1800 },
];

// 与 SetupSecurity.tsx 同源强度算法，避免引入新依赖。
function getPasswordStrength(pw: string): { label: string; color: string } {
  if (!pw) return { label: '尚未输入', color: '#999' };
  let score = 0;
  if (pw.length >= 8) score += 1;
  if (/[a-zA-Z]/.test(pw) && /\d/.test(pw)) score += 1;
  if (pw.length >= 12 && /[^a-zA-Z0-9]/.test(pw)) score += 1;
  if (score <= 0) return { label: '过短', color: '#ff4d4f' };
  if (score === 1) return { label: '弱', color: '#fa8c16' };
  if (score === 2) return { label: '中等', color: '#52c41a' };
  return { label: '强', color: '#1677ff' };
}

interface ChangePasswordForm {
  oldPassword: string;
  newPassword: string;
  confirmPassword: string;
}

const SecurityCenter: React.FC = () => {
  const sec = useSecurity();
  const [changeForm] = Form.useForm<ChangePasswordForm>();
  const [changeLoading, setChangeLoading] = useState(false);
  const [idleLoading, setIdleLoading] = useState(false);
  const [revealLoading, setRevealLoading] = useState(false);
  const [lockLoading, setLockLoading] = useState(false);
  const [newPassword, setNewPassword] = useState('');
  const [idleEnabled, setIdleEnabled] = useState<boolean>(sec.idleLockEnabled);
  const [idleSeconds, setIdleSeconds] = useState<number>(sec.idleTimeoutSeconds);
  const [revealSeconds, setRevealSeconds] = useState<number>(sec.sensitiveRevealSeconds);

  // 后端状态变化时（如 refreshStatus 之后）同步本地表单初值。
  useEffect(() => {
    setIdleEnabled(sec.idleLockEnabled);
    setIdleSeconds(sec.idleTimeoutSeconds);
  }, [sec.idleLockEnabled, sec.idleTimeoutSeconds]);

  useEffect(() => {
    setRevealSeconds(sec.sensitiveRevealSeconds);
  }, [sec.sensitiveRevealSeconds]);

  const pwStrength = useMemo(() => getPasswordStrength(newPassword), [newPassword]);

  const handleChangePassword = async (values: ChangePasswordForm) => {
    if (values.newPassword !== values.confirmPassword) {
      message.error('两次输入的新密码不一致');
      return;
    }
    setChangeLoading(true);
    try {
      await sec.changePassword(values.oldPassword, values.newPassword);
      message.success('密码修改成功');
      changeForm.resetFields();
      setNewPassword('');
    } catch (e) {
      message.error(e instanceof Error ? e.message : '修改密码失败');
    } finally {
      setChangeLoading(false);
    }
  };

  const handleIdleSubmit = async () => {
    setIdleLoading(true);
    try {
      await sec.updateIdle(idleEnabled, idleSeconds);
      message.success('闲置锁定配置已保存');
    } catch (e) {
      message.error(e instanceof Error ? e.message : '保存闲置配置失败');
    } finally {
      setIdleLoading(false);
    }
  };

  const handleRevealSubmit = async () => {
    setRevealLoading(true);
    try {
      await sec.updateReveal(revealSeconds);
      message.success('敏感解锁时长已保存');
    } catch (e) {
      message.error(e instanceof Error ? e.message : '保存敏感时长配置失败');
    } finally {
      setRevealLoading(false);
    }
  };

  const handleLock = async () => {
    setLockLoading(true);
    try {
      await sec.lock();
      message.success('已锁定应用');
    } catch (e) {
      message.error(e instanceof Error ? e.message : '锁定失败');
    } finally {
      setLockLoading(false);
    }
  };

  const handleRefresh = async () => {
    try {
      await sec.refreshStatus();
      message.success('已刷新安全状态');
    } catch (e) {
      message.error(e instanceof Error ? e.message : '刷新失败');
    }
  };

  // 倒计时展示：lock_until 是 RFC3339，转换为本地时间字符串。
  const lockUntilText = sec.lockUntil
    ? dayjs(sec.lockUntil).format('YYYY-MM-DD HH:mm:ss')
    : null;

  const migrationPending = sec.migrationStatus && sec.migrationStatus !== 'completed';

  return (
    <div>
      <div className="page-header">
        <span className="page-title">安全中心</span>
        <div className="page-header-actions">
          <Button icon={<ReloadOutlined />} onClick={handleRefresh}>
            刷新状态
          </Button>
        </div>
      </div>

      <Space direction="vertical" size="large" style={{ width: '100%' }}>
        {/* 卡片 1：安全状态 */}
        <Card title="安全状态">
          <Row gutter={[16, 16]}>
            <Col xs={24} md={6}>
              <Statistic
                title="应用状态"
                value={sec.isLocked ? '已锁定' : '已解锁'}
                prefix={<LockOutlined />}
              />
            </Col>
            <Col xs={24} md={6}>
              <Statistic title="失败尝试次数" value={sec.failedAttempts} suffix="/ 5" />
            </Col>
            <Col xs={24} md={6}>
              <Statistic
                title="闲置锁定"
                value={sec.idleLockEnabled ? '已启用' : '未启用'}
              />
            </Col>
            <Col xs={24} md={6}>
              <Statistic
                title="敏感解锁至"
                value={
                  sec.sensitiveRevealExpiresAt === null
                    ? '未解锁'
                    : dayjs(sec.sensitiveRevealExpiresAt).format('HH:mm:ss')
                }
              />
            </Col>
          </Row>
          {lockUntilText && (
            <Alert
              style={{ marginTop: 16 }}
              type="warning"
              showIcon
              message={`应用已临时锁定至 ${lockUntilText}，请稍后再试或使用找回密码功能。`}
            />
          )}
          {migrationPending && (
            <Alert
              style={{ marginTop: 16 }}
              type="warning"
              showIcon
              message="检测到旧版未加密资源"
              description={
                <Space direction="vertical" size={4}>
                  <Text>
                    当前迁移状态：<Tag color="orange">{sec.migrationStatus}</Tag>
                  </Text>
                  <Text type="secondary">
                    请在初始化引导或下次启动时完成迁移，否则部分历史数据将无法打开。
                  </Text>
                </Space>
              }
            />
          )}
        </Card>

        {/* 卡片 2：修改密码 */}
        <Card title="修改启动密码">
          <Form<ChangePasswordForm>
            form={changeForm}
            layout="vertical"
            onFinish={handleChangePassword}
            initialValues={{ oldPassword: '', newPassword: '', confirmPassword: '' }}
            style={{ maxWidth: 480 }}
          >
            <Form.Item
              label="当前密码"
              name="oldPassword"
              rules={[{ required: true, message: '请输入当前密码' }]}
            >
              <Input.Password placeholder="请输入当前密码" autoComplete="current-password" />
            </Form.Item>
            <Form.Item
              label="新密码"
              name="newPassword"
              rules={[
                { required: true, message: '请输入新密码' },
                {
                  validator: (_, value: string) => {
                    if (!value) return Promise.resolve();
                    if (value.length < 8) return Promise.reject(new Error('密码至少 8 位'));
                    if (!/[a-zA-Z]/.test(value) || !/\d/.test(value)) {
                      return Promise.reject(new Error('密码需同时包含字母和数字'));
                    }
                    return Promise.resolve();
                  },
                },
              ]}
              extra={
                <Text style={{ color: pwStrength.color }}>
                  强度：{pwStrength.label}
                </Text>
              }
            >
              <Input.Password
                placeholder="至少 8 位，需同时包含字母和数字"
                autoComplete="new-password"
                onChange={(e) => setNewPassword(e.target.value)}
              />
            </Form.Item>
            <Form.Item
              label="确认新密码"
              name="confirmPassword"
              dependencies={['newPassword']}
              rules={[
                { required: true, message: '请再次输入新密码' },
                ({ getFieldValue }) => ({
                  validator: (_, value: string) => {
                    if (!value || getFieldValue('newPassword') === value) {
                      return Promise.resolve();
                    }
                    return Promise.reject(new Error('两次输入的新密码不一致'));
                  },
                }),
              ]}
            >
              <Input.Password placeholder="请再次输入新密码" autoComplete="new-password" />
            </Form.Item>
            <Form.Item>
              <Button type="primary" htmlType="submit" loading={changeLoading}>
                提交修改
              </Button>
            </Form.Item>
          </Form>
        </Card>

        {/* 卡片 3：找回密码配置 */}
        <Card title="找回密码配置">
          <Space direction="vertical" size="middle" style={{ width: '100%' }}>
            <Alert
              type="info"
              showIcon
              message="找回密码说明"
              description={
                <Space direction="vertical" size={4}>
                  <Text>
                    忘记启动密码时，可使用初始化时抄写的 <Text strong>恢复码</Text> 或
                    <Text strong> 安全问题答案</Text> 在锁屏页找回访问权限。
                  </Text>
                  <Text type="secondary">
                    恢复码和安全问题在安全初始化时一次性生成；当前接口暂不支持在本页面查看或重置，如需变更请通过锁屏的找回流程或重新初始化。
                  </Text>
                </Space>
              }
            />
            <Text type="secondary">
              如怀疑恢复码已泄露，建议尽快完成密码修改；新密码生效后旧密码立即失效。
            </Text>
          </Space>
        </Card>

        {/* 卡片 4：闲置锁定配置 */}
        <Card title="闲置锁定配置">
          <Space direction="vertical" size="middle" style={{ width: '100%' }}>
            <Space align="center">
              <Switch
                checked={idleEnabled}
                onChange={setEnabled => setIdleEnabled(setEnabled)}
              />
              <Text>{idleEnabled ? '已启用闲置自动锁定' : '未启用闲置自动锁定'}</Text>
            </Space>
            <div>
              <Text type="secondary" style={{ display: 'block', marginBottom: 8 }}>
                闲置多长时间后自动锁定应用
              </Text>
              <Radio.Group
                value={idleSeconds}
                onChange={(e) => setIdleSeconds(Number(e.target.value))}
                optionType="button"
                buttonStyle="solid"
                options={DURATION_OPTIONS}
                disabled={!idleEnabled}
              />
            </div>
            <Button type="primary" loading={idleLoading} onClick={handleIdleSubmit}>
              保存闲置配置
            </Button>
          </Space>
        </Card>

        {/* 卡片 5：敏感解锁时长配置 */}
        <Card title="敏感解锁时长配置">
          <Space direction="vertical" size="middle" style={{ width: '100%' }}>
            <Text type="secondary">
              控制每次解锁敏感字段（如身份证号、银行卡号）后保持明文的时长，超时后自动恢复脱敏。
            </Text>
            <Radio.Group
              value={revealSeconds}
              onChange={(e) => setRevealSeconds(Number(e.target.value))}
              optionType="button"
              buttonStyle="solid"
              options={DURATION_OPTIONS}
            />
            <Button type="primary" loading={revealLoading} onClick={handleRevealSubmit}>
              保存敏感时长
            </Button>
          </Space>
        </Card>

        {/* 底部：手动锁屏 */}
        <Card title="立即锁定">
          <Space direction="vertical" size="small">
            <Text type="secondary">
              离开工位前可立即锁定应用，需要重新输入启动密码才能继续操作。
            </Text>
            <Button
              danger
              icon={<LockOutlined />}
              loading={lockLoading}
              onClick={handleLock}
            >
              立即锁定应用
            </Button>
          </Space>
        </Card>
      </Space>
    </div>
  );
};

export default SecurityCenter;
