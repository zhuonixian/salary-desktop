import { useState, type CSSProperties, type ReactElement } from 'react';
import { EyeInvisibleOutlined, EyeOutlined } from '@ant-design/icons';

import { useSecurity } from '@/contexts/SecurityContext';

import { RevealPasswordModal } from './RevealPasswordModal';

export type SensitiveType = 'id_card' | 'bank_card' | 'amount' | 'phone' | 'address' | 'raw';

const wrapperStyle: CSSProperties = {
  display: 'inline-flex',
  alignItems: 'center',
  gap: 4,
};

const iconStyle: CSSProperties = {
  cursor: 'pointer',
  color: '#888',
  fontSize: 14,
};

// 把任意类型按规则脱敏。空值返回空串,避免出现 `****` 与无值混淆。
function mask(type: SensitiveType, value: string): string {
  if (!value) return '';
  switch (type) {
    case 'id_card':
      return value.length >= 8
        ? `${value.slice(0, 6)}********${value.slice(-4)}`
        : '*'.repeat(value.length);
    case 'bank_card': {
      const last = value.replace(/\s+/g, '').slice(-4);
      return `**** **** **** ${last}`;
    }
    case 'amount':
      return '¥ ****';
    case 'phone':
      return value.length === 11
        ? `${value.slice(0, 3)}****${value.slice(-4)}`
        : '*'.repeat(value.length);
    case 'address':
      return value.length > 6 ? `${value.slice(0, 6)}***` : '***';
    case 'raw':
    default:
      return '****';
  }
}

export interface SensitiveTextProps {
  type: SensitiveType;
  value: string | number;
  // false 时只渲染脱敏文本,不带眼睛图标(适合"永远不展示明文"的字段)。
  revealable?: boolean;
}

export function SensitiveText({ type, value, revealable = true }: SensitiveTextProps): ReactElement {
  const { isSensitiveRevealed, revealSensitive } = useSecurity();
  const [modalOpen, setModalOpen] = useState(false);
  // 本组件的"主动收起"开关:用户点击 EyeInvisible 后只在本组件覆盖为脱敏,
  // 全局 reveal 状态保持(其它组件仍可继续查看)。再次点击 EyeOutlined 会重新显示明文。
  // 不调用 clearSensitiveReveal 是因为它会清掉全局窗口、影响同屏其它字段。
  const [localHidden, setLocalHidden] = useState(false);

  const text = String(value ?? '');
  const revealed = isSensitiveRevealed && !localHidden;
  const shown = revealed ? text : mask(type, text);

  if (!revealable) {
    return <span>{shown}</span>;
  }

  const onEyeClick = () => {
    if (revealed) {
      // 当前已显示明文 -> 仅本组件收起为脱敏,全局窗口保持。
      setLocalHidden(true);
      return;
    }
    if (isSensitiveRevealed) {
      // 全局已解锁但本组件被收起 -> 直接恢复明文。
      setLocalHidden(false);
      return;
    }
    // 全局未解锁 -> 弹密码 Modal。
    setModalOpen(true);
  };

  return (
    <span style={wrapperStyle}>
      <span>{shown}</span>
      {revealed ? (
        <EyeInvisibleOutlined onClick={onEyeClick} style={iconStyle} title="隐藏敏感数据" />
      ) : (
        <EyeOutlined
          onClick={onEyeClick}
          style={iconStyle}
          title={isSensitiveRevealed ? '显示敏感数据' : '查看敏感数据'}
        />
      )}
      <RevealPasswordModal
        open={modalOpen}
        onClose={() => setModalOpen(false)}
        onSubmit={async (pw) => {
          await revealSensitive(pw);
        }}
      />
    </span>
  );
}

export default SensitiveText;
