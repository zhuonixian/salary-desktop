import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react';
import {
  changePassword,
  getSecurityStatus,
  isSecurityInitialized,
  lockApp,
  migrateLegacyResources,
  resetPasswordByQuestion,
  resetPasswordByRecovery,
  revealSensitiveData,
  setupSecurity,
  unlock as apiUnlock,
  updateIdleSettings,
  updateSensitiveRevealSettings,
} from '@/api';
import type { SecurityStatus } from '@/types';

// 最大尝试次数与后端 security.rs MAX_FAILED_ATTEMPTS 对齐。
// 这里仅用于错误消息文案，不参与锁定判定逻辑（以后端 lock_until 为准）。
const MAX_FAILED_ATTEMPTS = 5;

export interface SecurityContextValue {
  isInitialized: boolean;
  isLocked: boolean;
  isSensitiveRevealed: boolean;
  sensitiveRevealExpiresAt: number | null;
  idleLockEnabled: boolean;
  idleTimeoutSeconds: number;
  sensitiveRevealSeconds: number;
  failedAttempts: number;
  lockUntil: string | null;
  migrationStatus: string | null;
  refreshStatus: () => Promise<void>;
  setup: (password: string, recoveryCode: string, question: string, answer: string) => Promise<void>;
  unlock: (password: string) => Promise<void>;
  lock: () => Promise<void>;
  revealSensitive: (password: string) => Promise<void>;
  clearSensitiveReveal: () => void;
  changePassword: (oldPassword: string, newPassword: string) => Promise<void>;
  resetByRecovery: (code: string, newPassword: string) => Promise<void>;
  resetByQuestion: (answer: string, newPassword: string) => Promise<void>;
  updateIdle: (enabled: boolean, seconds: number) => Promise<void>;
  updateReveal: (seconds: number) => Promise<void>;
  runMigration: () => Promise<void>;
}

const SecurityContext = createContext<SecurityContextValue | null>(null);

export function SecurityProvider({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<SecurityStatus | null>(null);
  const [isInitialized, setIsInitialized] = useState<boolean>(false);
  const [isLocked, setIsLocked] = useState<boolean>(true);
  const [revealExpiresAt, setRevealExpiresAt] = useState<number | null>(null);

  // refreshStatus 自身不向上抛错（避免 Provider mount 失败或阻塞性 UI）；
  // 调用方需要捕获错误时应直接 await 具体的 API。
  const refreshStatus = useCallback(async () => {
    try {
      const s = await getSecurityStatus();
      setStatus(s);
      setIsInitialized(s.initialized);
      // 已初始化时遵循后端判定，未初始化时强制未锁（让 SetupSecurity 显示）。
      setIsLocked(s.initialized ? s.locked : false);
    } catch (err) {
      console.error('[SecurityContext] refreshStatus 失败:', err);
    }
  }, []);

  // 启动时探测一次：已初始化则强制锁屏一次（要求用户主动 unlock）；
  // 未初始化则保持 unlocked，让 SetupSecurity 渲染。
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const init = await isSecurityInitialized();
        if (cancelled) return;
        setIsInitialized(init);
        if (init) {
          await refreshStatus();
          if (cancelled) return;
          setIsLocked(true);
        } else {
          setIsLocked(false);
        }
      } catch (err) {
        console.error('[SecurityContext] 初始化探测失败:', err);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [refreshStatus]);

  // 敏感解锁过期计时：在到期时清除 revealExpiresAt。
  // 用剩余毫秒做 setTimeout，避免每秒轮询。
  useEffect(() => {
    if (revealExpiresAt === null) return;
    const delay = Math.max(0, revealExpiresAt - Date.now());
    const timer = window.setTimeout(() => setRevealExpiresAt(null), delay);
    return () => window.clearTimeout(timer);
  }, [revealExpiresAt]);

  const setup = useCallback(async (password: string, recoveryCode: string, question: string, answer: string) => {
    await setupSecurity(password, recoveryCode, question, answer);
    await refreshStatus();
    setIsLocked(false);
  }, [refreshStatus]);

  const unlock = useCallback(async (password: string) => {
    const r = await apiUnlock(password);
    if (r.unlocked) {
      setIsLocked(false);
      await refreshStatus();
      return;
    }
    // 失败：刷新前端状态（更新 failed_attempts / lock_until），再抛错给 LockScreen。
    try {
      await refreshStatus();
    } catch (err) {
      console.error('[SecurityContext] unlock 失败后刷新状态出错:', err);
    }
    const remaining = Math.max(0, MAX_FAILED_ATTEMPTS - r.failed_attempts);
    throw new Error(`密码错误，剩余 ${remaining} 次尝试`);
  }, [refreshStatus]);

  const lock = useCallback(async () => {
    await lockApp();
    setIsLocked(true);
    setRevealExpiresAt(null);
    await refreshStatus();
  }, [refreshStatus]);

  const revealSensitive = useCallback(async (password: string) => {
    const r = await revealSensitiveData(password);
    const ts = Date.parse(r.expires_at);
    // Date.parse 失败时返回 NaN，此时立刻让 useEffect 清空（delay=0）。
    setRevealExpiresAt(Number.isFinite(ts) ? ts : Date.now());
  }, []);

  const clearSensitiveReveal = useCallback(() => setRevealExpiresAt(null), []);

  const changePasswordCb = useCallback(async (oldPassword: string, newPassword: string) => {
    await changePassword(oldPassword, newPassword);
  }, []);

  const resetByRecovery = useCallback(async (code: string, newPassword: string) => {
    await resetPasswordByRecovery(code, newPassword);
  }, []);

  const resetByQuestion = useCallback(async (answer: string, newPassword: string) => {
    await resetPasswordByQuestion(answer, newPassword);
  }, []);

  const updateIdle = useCallback(async (enabled: boolean, seconds: number) => {
    await updateIdleSettings(enabled, seconds);
    await refreshStatus();
  }, [refreshStatus]);

  const updateReveal = useCallback(async (seconds: number) => {
    await updateSensitiveRevealSettings(seconds);
    await refreshStatus();
  }, [refreshStatus]);

  const runMigration = useCallback(async () => {
    await migrateLegacyResources();
    await refreshStatus();
  }, [refreshStatus]);

  const value = useMemo<SecurityContextValue>(() => ({
    isInitialized,
    isLocked,
    // 当 revealExpiresAt 非 null 时认为已解锁；过期清理由上方 useEffect 的 setTimeout 负责。
    // 不在此处调用 Date.now()，避免 render 期间的不纯调用（react-hooks/purity）。
    isSensitiveRevealed: revealExpiresAt !== null,
    sensitiveRevealExpiresAt: revealExpiresAt,
    idleLockEnabled: status?.idle_lock_enabled ?? true,
    idleTimeoutSeconds: status?.idle_timeout_seconds ?? 300,
    sensitiveRevealSeconds: status?.sensitive_reveal_seconds ?? 300,
    failedAttempts: status?.failed_attempts ?? 0,
    lockUntil: status?.lock_until ?? null,
    migrationStatus: status?.migration_status ?? null,
    refreshStatus,
    setup,
    unlock,
    lock,
    revealSensitive,
    clearSensitiveReveal,
    changePassword: changePasswordCb,
    resetByRecovery,
    resetByQuestion,
    updateIdle,
    updateReveal,
    runMigration,
  }), [
    isInitialized,
    isLocked,
    revealExpiresAt,
    status,
    refreshStatus,
    setup,
    unlock,
    lock,
    revealSensitive,
    clearSensitiveReveal,
    changePasswordCb,
    resetByRecovery,
    resetByQuestion,
    updateIdle,
    updateReveal,
    runMigration,
  ]);

  return <SecurityContext.Provider value={value}>{children}</SecurityContext.Provider>;
}

// react-refresh 要求文件只 export 组件；Context + hook 同文件是 React 官方推荐模式，
// 这里仅在开发热重载时让 HMR 退化为全量刷新，不影响生产构建。
// eslint-disable-next-line react-refresh/only-export-components
export function useSecurity(): SecurityContextValue {
  const ctx = useContext(SecurityContext);
  if (!ctx) {
    throw new Error('useSecurity 必须在 <SecurityProvider> 内部使用');
  }
  return ctx;
}
