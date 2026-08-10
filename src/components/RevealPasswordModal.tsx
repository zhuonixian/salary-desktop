import { useEffect, useState, type CSSProperties } from 'react';
import { Alert, Form, Input, Modal } from 'antd';

interface RevealPasswordModalProps {
  open: boolean;
  onClose: () => void;
  // 校验失败时 throw Error,Modal 内显示错误文案;成功 resolve 后由组件内自动关闭。
  onSubmit: (password: string) => Promise<void>;
}

const descStyle: CSSProperties = {
  marginBottom: 12,
  color: '#666',
  fontSize: 13,
};

// 二次输入启动密码以临时查看敏感数据。
// 与 LockScreen 不同:这里只是验证身份后开启 reveal 窗口,不会修改锁定状态。
export function RevealPasswordModal({ open, onClose, onSubmit }: RevealPasswordModalProps) {
  const [pw, setPw] = useState('');
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState('');

  // 每次打开重置表单状态,避免上一次的密码/错误残留。
  useEffect(() => {
    if (open) {
      setPw('');
      setErr('');
      setBusy(false);
    }
  }, [open]);

  const submit = async () => {
    if (busy) return;
    const password = pw;
    if (!password) {
      setErr('请输入启动密码');
      return;
    }
    setErr('');
    setBusy(true);
    try {
      await onSubmit(password);
      // 成功:交由 useEffect 在 open 变化时清空,这里同步清一次防止闪现旧值。
      setPw('');
      onClose();
    } catch (e) {
      const msg = e instanceof Error ? e.message : '验证失败';
      setErr(msg);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open={open}
      onCancel={onClose}
      title="查看敏感数据"
      okText="确认"
      cancelText="取消"
      confirmLoading={busy}
      maskClosable={false}
      destroyOnClose
      onOk={submit}
    >
      <div style={descStyle}>请输入启动密码以临时查看脱敏数据,有效期由系统设置控制。</div>
      <Form layout="vertical">
        <Form.Item label="启动密码" required>
          <Input.Password
            value={pw}
            onChange={(e) => setPw(e.target.value)}
            onPressEnter={submit}
            placeholder="请输入启动密码"
            autoFocus
            autoComplete="off"
          />
        </Form.Item>
      </Form>
      {err && <Alert type="error" message={err} style={{ marginTop: 8 }} showIcon />}
    </Modal>
  );
}

export default RevealPasswordModal;
