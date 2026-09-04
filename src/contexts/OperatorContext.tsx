import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react';
import { getCurrentOperator, getOperatorProfiles, setCurrentOperator } from '@/api';
import type { OperatorProfile } from '@/types';

interface OperatorContextValue {
  /** 当前操作人（后端会话权威；未选择/已失效为 null） */
  operator: OperatorProfile | null;
  /** 全部操作人档案（含停用，供基础资料页展示） */
  operators: OperatorProfile[];
  loading: boolean;
  /** 重新拉取当前操作人与档案列表（基础资料变更后同步会话视图） */
  reload: () => Promise<void>;
  /** 切换当前操作人（后端校验存在且启用） */
  selectOperator: (operatorId: number) => Promise<void>;
}

const OperatorContext = createContext<OperatorContextValue | null>(null);

export function OperatorProvider({ children }: { children: ReactNode }) {
  const [operator, setOperator] = useState<OperatorProfile | null>(null);
  const [operators, setOperators] = useState<OperatorProfile[]>([]);
  const [loading, setLoading] = useState(true);

  const reload = useCallback(async () => {
    setLoading(true);
    try {
      // get_current_operator 未选择/失效返回 null 不报错；两个请求互不依赖。
      const [current, profiles] = await Promise.all([getCurrentOperator(), getOperatorProfiles()]);
      setOperator(current);
      setOperators(profiles);
    } catch {
      // 拉取失败不阻断应用启动，保持未选择状态由页面引导重试。
      setOperator(null);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  const selectOperator = useCallback(async (operatorId: number) => {
    const profile = await setCurrentOperator(operatorId);
    setOperator(profile);
    // 会话变更后同步档案列表（updated_at 等字段可能已变化）。
    try {
      setOperators(await getOperatorProfiles());
    } catch {
      // 列表刷新失败不影响已生效的当前操作人。
    }
  }, []);

  const value = useMemo<OperatorContextValue>(() => ({
    operator,
    operators,
    loading,
    reload,
    selectOperator,
  }), [operator, operators, loading, reload, selectOperator]);

  return (
    <OperatorContext.Provider value={value}>
      {children}
    </OperatorContext.Provider>
  );
}

// Context 与 hook 同文件便于业务页面统一引用；开发热更新时允许退化为全量刷新。
// eslint-disable-next-line react-refresh/only-export-components
export function useOperator(): OperatorContextValue {
  const context = useContext(OperatorContext);
  if (!context) {
    throw new Error('useOperator 必须在 OperatorProvider 内使用');
  }
  return context;
}
