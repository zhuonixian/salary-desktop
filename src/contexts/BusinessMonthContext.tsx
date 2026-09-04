import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useState,
  type ReactNode,
} from 'react';
import dayjs from 'dayjs';
import type { Dayjs } from 'dayjs';

const STORAGE_KEY = 'salary-desktop.business-month';
const MONTH_PATTERN = /^\d{4}-(0[1-9]|1[0-2])$/;

interface BusinessMonthContextValue {
  month: Dayjs;
  monthStr: string;
  setMonth: (month: Dayjs) => void;
}

const BusinessMonthContext = createContext<BusinessMonthContextValue | null>(null);

function getInitialMonth(): Dayjs {
  try {
    const storedMonth = localStorage.getItem(STORAGE_KEY);
    if (storedMonth && MONTH_PATTERN.test(storedMonth)) {
      const parsedMonth = dayjs(`${storedMonth}-01`);
      if (parsedMonth.isValid() && parsedMonth.format('YYYY-MM') === storedMonth) {
        return parsedMonth;
      }
    }
  } catch {
    // localStorage 不可用时仍允许应用使用当前月份启动。
  }

  return dayjs();
}

export function BusinessMonthProvider({ children }: { children: ReactNode }) {
  const [month, setMonthState] = useState<Dayjs>(getInitialMonth);

  const setMonth = useCallback((nextMonth: Dayjs) => {
    if (!nextMonth.isValid()) return;

    setMonthState(nextMonth);

    try {
      localStorage.setItem(STORAGE_KEY, nextMonth.format('YYYY-MM'));
    } catch {
      // 持久化失败不阻断当前会话内的月份切换。
    }
  }, []);

  const value = useMemo<BusinessMonthContextValue>(() => ({
    month,
    monthStr: month.format('YYYY-MM'),
    setMonth,
  }), [month, setMonth]);

  return (
    <BusinessMonthContext.Provider value={value}>
      {children}
    </BusinessMonthContext.Provider>
  );
}

// Context 与 hook 同文件便于业务页面统一引用；开发热更新时允许退化为全量刷新。
// eslint-disable-next-line react-refresh/only-export-components
export function useBusinessMonth(): BusinessMonthContextValue {
  const context = useContext(BusinessMonthContext);
  if (!context) {
    throw new Error('useBusinessMonth 必须在 BusinessMonthProvider 内使用');
  }
  return context;
}
