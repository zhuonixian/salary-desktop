import { useEffect, useState, type CSSProperties } from 'react';
import { Alert, Button, Input, Modal, Tabs, Typography, message } from 'antd';

import { useSecurity } from '@/contexts/SecurityContext';

const { Text, Paragraph } = Typography;

const overlayStyle: CSSProperties = {
  position: 'fixed',
  inset: 0,
  background: 'rgba(0,0,0,0.85)',
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  zIndex: 9999,
};

const cardStyle: CSSProperties = {
  width: 380,
  padding: 28,
  background: '#fff',
  borderRadius: 10,
  boxShadow: '0 12px 32px rgba(0,0,0,0.25)',
};

const titleStyle: CSSProperties = {
  textAlign: 'center',
  marginBottom: 8,
  fontSize: 18,
  fontWeight: 600,
};

const subtitleStyle: CSSProperties = {
  textAlign: 'center',
  marginBottom: 18,
  color: '#666',
  fontSize: 13,
};

const footerLinkStyle: CSSProperties = {
  textAlign: 'center',
  marginTop: 12,
  fontSize: 13,
};

// 锁定剩余秒数显示。每秒 tick 一次,以便倒计时刷新。
// 计算用 now 保存为 state(初始值在 useState 惰性初始化时取一次),
// 之后由 setInterval 触发 setState 推进,避免在 render 期间调用 Date.now()。
function useCountdownTick(lockUntil: string | null): { remainingMs: number; nowBlocked: boolean } {
  const ts = lockUntil ? Date.parse(lockUntil) : null;
  const finiteTs = ts !== null && Number.isFinite(ts) ? ts : null;

  const [now, setNow] = useState<number>(() => Date.now());

  useEffect(() => {
    if (finiteTs === null) return;
    setNow(Date.now());
    if (Date.now() >= finiteTs) return;
    const timer = window.setInterval(() => {
      const current = Date.now();
      setNow(current);
      if (current >= finiteTs) {
        window.clearInterval(timer);
      }
    }, 1000);
    return () => window.clearInterval(timer);
  }, [finiteTs]);

  if (finiteTs === null) return { remainingMs: 0, nowBlocked: false };
  const remaining = finiteTs - now;
  return { remainingMs: remaining, nowBlocked: remaining > 0 };
}

function formatRemaining(ms: number): string {
  const total = Math.max(0, Math.ceil(ms / 1000));
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
}

interface ResetPasswordModalProps {
  open: boolean;
  onClose: () => void;
  resetByRecovery: (code: string, newPassword: string) => Promise<void>;
  resetByQuestion: (answer: string, newPassword: string) => Promise<void>;
}

function ResetPasswordModal(props: ResetPasswordModalProps) {
  const [tab, setTab] = useState<'recovery' | 'question'>('recovery');
  const [recoveryCode, setRecoveryCode] = useState('');
  const [questionAnswer, setQuestionAnswer] = useState('');
  const [newPassword, setNewPassword] = useState('');
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState('');

  const reset = () => {
    setRecoveryCode('');
    setQuestionAnswer('');
    setNewPassword('');
    setErr('');
    setBusy(false);
    setTab('recovery');
  };

  const handleClose = () => {
    reset();
    props.onClose();
  };

  const onSubmit = async () => {
    setErr('');
    const newPw = newPassword.trim();
    if (newPw.length < 8) {
      setErr('新密码至少 8 位且同时包含字母和数字');
      return;
    }
    if (!/[a-zA-Z]/.test(newPw) || !/\d/.test(newPw)) {
      setErr('新密码必须同时包含字母和数字');
      return;
    }

    setBusy(true);
    try {
      if (tab === 'recovery') {
        const code = recoveryCode.trim();
        if (!code) {
          setErr('请输入恢复码');
          return;
        }
        await props.resetByRecovery(code, newPw);
      } else {
        const ans = questionAnswer.trim();
        if (!ans) {
          setErr('请输入安全问题答案');
          return;
        }
        await props.resetByQuestion(ans, newPw);
      }
      message.success('密码已重置,请使用新密码解锁');
      handleClose();
    } catch (e) {
      const msg = e instanceof Error ? e.message : '重置失败';
      setErr(msg);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open={props.open}
      onCancel={handleClose}
      footer={null}
      title="找回密码"
      destroyOnClose
      maskClosable={false}
      zIndex={10000}
    >
      <Tabs
        activeKey={tab}
        onChange={(key) => setTab(key as 'recovery' | 'question')}
        items={[
          {
            key: 'recovery',
            label: '恢复码',
            children: (
              <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
                <Paragraph type="secondary" style={{ marginBottom: 0, fontSize: 13 }}>
                  请输入初始化时抄写的 24 位恢复码(XXXX-XXXX-XXXX-XXXX-XXXX-XXXX)。
                </Paragraph>
                <Input.TextArea
                  rows={3}
                  placeholder="恢复码"
                  value={recoveryCode}
                  onChange={(e) => setRecoveryCode(e.target.value)}
                />
                <Input.Password
                  placeholder="新密码(≥8 位,字母+数字)"
                  value={newPassword}
                  onChange={(e) => setNewPassword(e.target.value)}
                />
              </div>
            ),
          },
          {
            key: 'question',
            label: '安全问题',
            children: (
              <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
                <Paragraph type="secondary" style={{ marginBottom: 0, fontSize: 13 }}>
                  请输入初始化时设置的安全问题答案。
                </Paragraph>
                <Input
                  placeholder="安全问题答案"
                  value={questionAnswer}
                  onChange={(e) => setQuestionAnswer(e.target.value)}
                />
                <Input.Password
                  placeholder="新密码(≥8 位,字母+数字)"
                  value={newPassword}
                  onChange={(e) => setNewPassword(e.target.value)}
                />
              </div>
            ),
          },
        ]}
      />
      {err && <Alert type="error" message={err} style={{ marginTop: 12 }} showIcon />}
      <div style={{ marginTop: 16, textAlign: 'right' }}>
        <Button onClick={handleClose} style={{ marginRight: 8 }}>
          取消
        </Button>
        <Button type="primary" loading={busy} onClick={onSubmit}>
          重置密码
        </Button>
      </div>
    </Modal>
  );
}

export function LockScreen() {
  const { unlock, lockUntil, failedAttempts, resetByRecovery, resetByQuestion } = useSecurity();
  const [pw, setPw] = useState('');
  const [err, setErr] = useState('');
  const [busy, setBusy] = useState(false);
  const [resetOpen, setResetOpen] = useState(false);

  const { remainingMs, nowBlocked } = useCountdownTick(lockUntil);

  // 解除锁定状态后清空旧错误提示,便于重新输入。
  useEffect(() => {
    if (!nowBlocked) {
      setErr((prev) => (prev && /请于|尝试过多|尝试次数过多/.test(prev) ? '' : prev));
    }
  }, [nowBlocked]);

  const onUnlock = async () => {
    if (nowBlocked) return;
    setErr('');
    setBusy(true);
    try {
      await unlock(pw);
      setPw('');
    } catch (e) {
      const msg = e instanceof Error ? e.message : '解锁失败';
      setErr(msg);
      setPw('');
    } finally {
      setBusy(false);
    }
  };

  return (
    <div style={overlayStyle}>
      <div style={cardStyle}>
        <h3 style={titleStyle}>工资核算助手已锁定</h3>
        <div style={subtitleStyle}>请输入启动密码以继续使用</div>

        <Input.Password
          placeholder="请输入启动密码"
          value={pw}
          onChange={(e) => setPw(e.target.value)}
          onPressEnter={onUnlock}
          disabled={nowBlocked}
          autoComplete="off"
          size="large"
        />

        {err && <Alert type="error" message={err} style={{ marginTop: 12 }} showIcon />}

        {nowBlocked && (
          <Alert
            type="warning"
            message={`尝试次数过多,请于 ${formatRemaining(remainingMs)} 后重试`}
            description={
              <Text type="secondary" style={{ fontSize: 12 }}>
                当前累计失败 {failedAttempts} 次。锁定结束后可继续尝试。
              </Text>
            }
            style={{ marginTop: 12 }}
            showIcon
          />
        )}

        <Button
          type="primary"
          block
          size="large"
          style={{ marginTop: 14 }}
          loading={busy}
          onClick={onUnlock}
          disabled={nowBlocked}
        >
          解锁
        </Button>

        <div style={footerLinkStyle}>
          <a
            onClick={() => setResetOpen(true)}
            style={{ color: '#1677ff', cursor: 'pointer' }}
          >
            忘记密码?
          </a>
        </div>
      </div>

      <ResetPasswordModal
        open={resetOpen}
        onClose={() => setResetOpen(false)}
        resetByRecovery={resetByRecovery}
        resetByQuestion={resetByQuestion}
      />
    </div>
  );
}

export default LockScreen;
