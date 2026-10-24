import { useEffect, useState } from 'react';
import { Tour } from 'antd';
import type { TourProps } from 'antd';

/**
 * 页面操作指引（第十五阶段）：antd Tour 封装。
 * - 首次进入自动弹出：localStorage 记忆（tour:{storageKey}=done 后不再自动弹）
 * - reopenTick 递增时重新打开：页面 header「指引」按钮复看
 * - 步骤锚点用 CSS selector（data-tour="xxx"）；无匹配元素时 antd 居中展示，不依赖 DOM 也安全
 */
interface PageTourProps {
  storageKey: string;
  steps: TourProps['steps'];
  reopenTick: number;
}

const storageKeyOf = (key: string) => `tour:${key}`;

const PageTour: React.FC<PageTourProps> = ({ storageKey, steps, reopenTick }) => {
  const [open, setOpen] = useState(false);

  // 首次进入自动弹出（同步读 localStorage，避免闪烁）
  useEffect(() => {
    if (localStorage.getItem(storageKeyOf(storageKey)) !== 'done') {
      setOpen(true);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 页面按钮触发复看
  useEffect(() => {
    if (reopenTick > 0) {
      setOpen(true);
    }
  }, [reopenTick]);

  const handleClose = () => {
    localStorage.setItem(storageKeyOf(storageKey), 'done');
    setOpen(false);
  };

  return (
    <Tour
      open={open}
      onClose={handleClose}
      steps={steps}
      indicatorsRender={(current, total) => (
        <span style={{ color: 'rgba(255,255,255,0.65)' }}>
          {current + 1} / {total}
        </span>
      )}
    />
  );
};

export default PageTour;
