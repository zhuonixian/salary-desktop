import type { ReactElement, ReactNode } from 'react';
import { EyeInvisibleOutlined, EyeOutlined } from '@ant-design/icons';
import { Statistic } from 'antd';
import type { StatisticProps } from 'antd';

import { useSecurity } from '@/contexts/SecurityContext';

import { RevealPasswordModal } from './RevealPasswordModal';
import { useState } from 'react';

// Statistic 的 value 只接受 string | number；要在金额脱敏时切换明文/掩码，
// 必须在调用方决定 value。此封装根据 SecurityContext 切换 value 并叠加眼睛图标。
export interface SensitiveStatisticProps extends Omit<StatisticProps, 'value'> {
  value: number | string;
  // 掩码文本，默认按金额脱敏显示 '¥ ****'。
  maskText?: string;
  prefix?: ReactNode;
}

export function SensitiveStatistic({
  value,
  maskText = '¥ ****',
  ...rest
}: SensitiveStatisticProps): ReactElement {
  const { isSensitiveRevealed, revealSensitive } = useSecurity();
  const [modalOpen, setModalOpen] = useState(false);
  const [localHidden, setLocalHidden] = useState(false);

  const revealed = isSensitiveRevealed && !localHidden;
  const shown = revealed ? value : maskText;

  const eyeIcon = revealed ? (
    <EyeInvisibleOutlined
      onClick={() => setLocalHidden(true)}
      style={{ cursor: 'pointer', color: '#888', fontSize: 14 }}
      title="隐藏敏感数据"
    />
  ) : (
    <EyeOutlined
      onClick={() => {
        if (isSensitiveRevealed) {
          setLocalHidden(false);
        } else {
          setModalOpen(true);
        }
      }}
      style={{ cursor: 'pointer', color: '#888', fontSize: 14 }}
      title={isSensitiveRevealed ? '显示敏感数据' : '查看敏感数据'}
    />
  );

  return (
    <>
      <Statistic
        {...rest}
        value={shown}
        prefix={
          <>
            {rest.prefix}
            {eyeIcon}
          </>
        }
      />
      <RevealPasswordModal
        open={modalOpen}
        onClose={() => setModalOpen(false)}
        onSubmit={async (pw) => {
          await revealSensitive(pw);
        }}
      />
    </>
  );
}

export default SensitiveStatistic;
