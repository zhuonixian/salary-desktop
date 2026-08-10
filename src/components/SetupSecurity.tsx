import { useMemo, useState, type CSSProperties } from 'react';
import {
  Alert,
  Button,
  Checkbox,
  Input,
  Select,
  Steps,
  Typography,
  message,
} from 'antd';

import { useSecurity } from '@/contexts/SecurityContext';

const { Title, Paragraph, Text } = Typography;

const QUESTIONS = [
  '你小学班主任姓什么?',
  '你父亲的名字最后一个字?',
  '你出生的城市?',
  '你的第一家公司名称?',
  '你最喜欢的菜品?',
];

// 排除易混字符 O/0/I/1,base32 风格字母表。
function generateRecoveryCode(): string {
  const chars = 'ABCDEFGHIJKLMNPQRSTUVWXYZ23456789';
  const segments: string[] = [];
  for (let s = 0; s < 6; s++) {
    let seg = '';
    for (let i = 0; i < 4; i++) {
      seg += chars[Math.floor(Math.random() * chars.length)];
    }
    segments.push(seg);
  }
  return segments.join('-');
}

const wrapperStyle: CSSProperties = {
  minHeight: '100vh',
  width: '100%',
  background: 'linear-gradient(135deg, #f5f7fa 0%, #e8eef5 100%)',
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  padding: 24,
};

const cardStyle: CSSProperties = {
  width: 640,
  maxWidth: '100%',
  background: '#fff',
  borderRadius: 12,
  boxShadow: '0 12px 32px rgba(0,0,0,0.12)',
  padding: 32,
};

const stepBodyStyle: CSSProperties = {
  marginTop: 24,
  minHeight: 240,
};

const recoveryBoxStyle: CSSProperties = {
  background: '#f6f8fa',
  border: '1px dashed #d9d9d9',
  borderRadius: 8,
  padding: '16px 20px',
  fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',
  fontSize: 20,
  letterSpacing: 1.5,
  textAlign: 'center',
  wordBreak: 'break-all',
};

function getPasswordStrength(pw: string): { score: 0 | 1 | 2 | 3; label: string; color: string } {
  if (!pw) return { score: 0, label: '尚未输入', color: '#999' };
  let score = 0;
  if (pw.length >= 8) score += 1;
  if (/[a-zA-Z]/.test(pw) && /\d/.test(pw)) score += 1;
  if (pw.length >= 12 && /[^a-zA-Z0-9]/.test(pw)) score += 1;
  if (score <= 0) return { score: 0, label: '过短', color: '#ff4d4f' };
  if (score === 1) return { score: 1, label: '弱', color: '#fa8c16' };
  if (score === 2) return { score: 2, label: '中等', color: '#52c41a' };
  return { score: 3, label: '强', color: '#1677ff' };
}

export function SetupSecurity() {
  const { setup, migrationStatus, runMigration } = useSecurity();
  const [step, setStep] = useState(0);
  const [pw, setPw] = useState('');
  const [pw2, setPw2] = useState('');
  const [err, setErr] = useState('');
  const [recovery] = useState<string>(() => generateRecoveryCode());
  const [copied, setCopied] = useState(false);
  const [savedAck, setSavedAck] = useState(false);
  const [question, setQuestion] = useState<string>(QUESTIONS[0]);
  const [answer, setAnswer] = useState('');
  const [answer2, setAnswer2] = useState('');
  const [busy, setBusy] = useState(false);

  const pwStrength = useMemo(() => getPasswordStrength(pw), [pw]);
  const pwStrengthOk = pw.length >= 8 && /[a-zA-Z]/.test(pw) && /\d/.test(pw);

  const copyRecovery = async () => {
    try {
      await navigator.clipboard.writeText(recovery);
      setCopied(true);
      message.success('已复制到剪贴板');
      window.setTimeout(() => setCopied(false), 2000);
    } catch {
      message.error('复制失败,请手动抄写');
    }
  };

  const nextFromPassword = () => {
    setErr('');
    if (!pwStrengthOk) {
      setErr('密码至少 8 位且同时包含字母和数字');
      return;
    }
    if (pw !== pw2) {
      setErr('两次输入的密码不一致');
      return;
    }
    setStep(1);
  };

  const nextFromRecovery = () => {
    setErr('');
    if (!savedAck) {
      setErr('请先勾选“我已抄写保存恢复码”');
      return;
    }
    setStep(2);
  };

  const nextFromQuestion = () => {
    setErr('');
    if (!answer.trim()) {
      setErr('请填写安全问题答案');
      return;
    }
    if (answer !== answer2) {
      setErr('两次输入的答案不一致');
      return;
    }
    setStep(3);
  };

  const submit = async () => {
    setErr('');
    if (!pwStrengthOk) {
      setErr('密码至少 8 位且同时包含字母和数字');
      setStep(0);
      return;
    }
    if (pw !== pw2) {
      setErr('两次输入的密码不一致');
      setStep(0);
      return;
    }
    if (!savedAck) {
      setErr('请先确认已抄写保存恢复码');
      setStep(1);
      return;
    }
    if (!answer.trim()) {
      setErr('请填写安全问题答案');
      setStep(2);
      return;
    }
    setBusy(true);
    try {
      await setup(pw, recovery, question, answer.trim());
      // 当存在待迁移资源时,自动触发迁移;迁移失败不影响 setup 成功,但需提示用户。
      if (migrationStatus === 'pending' || migrationStatus === 'in_progress') {
        try {
          await runMigration();
        } catch (e) {
          const msg = e instanceof Error ? e.message : '迁移失败';
          message.warning(`初始化完成,但旧版资源迁移失败: ${msg}`);
        }
      }
      message.success('安全初始化完成');
    } catch (e) {
      const msg = e instanceof Error ? e.message : '初始化失败';
      setErr(msg);
    } finally {
      setBusy(false);
    }
  };

  const stepsItems = [
    { title: '设置启动密码' },
    { title: '保存恢复码' },
    { title: '设置安全问题' },
    { title: '完成确认' },
  ];

  return (
    <div style={wrapperStyle}>
      <div style={cardStyle}>
        <Title level={3} style={{ textAlign: 'center', marginBottom: 8 }}>
          安全初始化
        </Title>
        <Paragraph type="secondary" style={{ textAlign: 'center', marginBottom: 24 }}>
          为保护工资数据,首次使用前请完成以下 4 步设置。
        </Paragraph>

        <Steps current={step} items={stepsItems} size="small" />

        {err && (
          <Alert type="error" message={err} style={{ marginTop: 16 }} showIcon closable onClose={() => setErr('')} />
        )}

        {/* Step 0: 启动密码 */}
        {step === 0 && (
          <div style={stepBodyStyle}>
            <Paragraph>请设置一个启动密码,所有工资数据将通过该密码加密保存。</Paragraph>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
              <div>
                <Text type="secondary" style={{ fontSize: 12 }}>
                  启动密码
                </Text>
                <Input.Password
                  placeholder="至少 8 位,需同时包含字母和数字"
                  value={pw}
                  onChange={(e) => setPw(e.target.value)}
                  size="large"
                  style={{ marginTop: 4 }}
                />
              </div>
              <div>
                <Text type="secondary" style={{ fontSize: 12 }}>
                  强度:
                  <Text style={{ color: pwStrength.color, marginLeft: 4 }}>{pwStrength.label}</Text>
                </Text>
              </div>
              <div>
                <Text type="secondary" style={{ fontSize: 12 }}>
                  确认启动密码
                </Text>
                <Input.Password
                  placeholder="请再次输入启动密码"
                  value={pw2}
                  onChange={(e) => setPw2(e.target.value)}
                  size="large"
                  style={{ marginTop: 4 }}
                />
              </div>
            </div>
          </div>
        )}

        {/* Step 1: 恢复码 */}
        {step === 1 && (
          <div style={stepBodyStyle}>
            <Paragraph>
              以下恢复码用于在忘记启动密码时找回访问权限。请务必抄写保存到离线安全位置(如纸质记事本)。
            </Paragraph>
            <div style={recoveryBoxStyle}>{recovery}</div>
            <div style={{ marginTop: 12, display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
              <Button size="small" onClick={copyRecovery}>
                {copied ? '已复制' : '复制到剪贴板'}
              </Button>
              <Text type="warning" style={{ fontSize: 12 }}>
                此恢复码仅显示一次,关闭后无法再次查看。
              </Text>
            </div>
            <Checkbox
              checked={savedAck}
              onChange={(e) => setSavedAck(e.target.checked)}
              style={{ marginTop: 16 }}
            >
              我已抄写保存恢复码
            </Checkbox>
          </div>
        )}

        {/* Step 2: 安全问题 */}
        {step === 2 && (
          <div style={stepBodyStyle}>
            <Paragraph>选择一个安全问题并填写答案,作为忘记密码时的备用找回方式。</Paragraph>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
              <div>
                <Text type="secondary" style={{ fontSize: 12 }}>
                  安全问题
                </Text>
                <Select
                  value={question}
                  onChange={setQuestion}
                  style={{ width: '100%', marginTop: 4 }}
                  options={QUESTIONS.map((q) => ({ value: q, label: q }))}
                />
              </div>
              <div>
                <Text type="secondary" style={{ fontSize: 12 }}>
                  答案
                </Text>
                <Input
                  placeholder="请输入安全问题答案"
                  value={answer}
                  onChange={(e) => setAnswer(e.target.value)}
                  style={{ marginTop: 4 }}
                />
              </div>
              <div>
                <Text type="secondary" style={{ fontSize: 12 }}>
                  确认答案
                </Text>
                <Input
                  placeholder="请再次输入答案"
                  value={answer2}
                  onChange={(e) => setAnswer2(e.target.value)}
                  style={{ marginTop: 4 }}
                />
              </div>
            </div>
          </div>
        )}

        {/* Step 3: 完成 */}
        {step === 3 && (
          <div style={stepBodyStyle}>
            <Paragraph>请确认以下信息无误后提交。提交后即可开始使用工资核算助手。</Paragraph>
            <div
              style={{
                background: '#fafafa',
                borderRadius: 8,
                padding: 16,
                display: 'flex',
                flexDirection: 'column',
                gap: 8,
              }}
            >
              <div>
                <Text strong>启动密码:</Text>{' '}
                <Text type={pwStrengthOk ? 'success' : 'danger'}>
                  {pwStrengthOk ? '已设置,符合强度要求' : '不满足要求'}
                </Text>
              </div>
              <div>
                <Text strong>恢复码:</Text>{' '}
                <Text type={savedAck ? 'success' : 'danger'}>
                  {savedAck ? '已抄写保存' : '未确认'}
                </Text>
              </div>
              <div>
                <Text strong>安全问题:</Text> <Text>{question}</Text>
              </div>
              <div>
                <Text strong>答案:</Text>{' '}
                <Text type={answer.trim() ? 'success' : 'danger'}>
                  {answer.trim() ? '已填写' : '未填写'}
                </Text>
              </div>
            </div>
            <Alert
              type="info"
              message="提交后将立即加密数据库与发票,无法撤销"
              style={{ marginTop: 12 }}
              showIcon
            />
          </div>
        )}

        {/* 底部导航 */}
        <div style={{ marginTop: 24, display: 'flex', justifyContent: 'space-between' }}>
          <Button
            disabled={step === 0 || busy}
            onClick={() => {
              setErr('');
              setStep((s) => Math.max(0, s - 1));
            }}
          >
            上一步
          </Button>
          <div>
            {step < 3 ? (
              <Button
                type="primary"
                onClick={() => {
                  if (step === 0) nextFromPassword();
                  else if (step === 1) nextFromRecovery();
                  else if (step === 2) nextFromQuestion();
                }}
              >
                下一步
              </Button>
            ) : (
              <Button type="primary" loading={busy} onClick={submit}>
                完成并启用加密
              </Button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

export default SetupSecurity;
