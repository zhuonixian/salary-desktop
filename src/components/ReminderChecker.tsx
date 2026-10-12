import { useCallback, useEffect, useRef } from 'react';
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from '@tauri-apps/plugin-notification';
import { listen } from '@tauri-apps/api/event';
import {
  getDueNotifications,
  getLocalNotifySettings,
  getNotifyMemory,
  markNotified,
  updateTrayBadge,
} from '@/api';
import { useSecurity } from '@/contexts/SecurityContext';
import type { DueNotification } from '@/types';

/** 单轮最多弹出的系统通知条数，超出部分合并到末条「…等 N 项待处理」 */
const MAX_NOTIFICATIONS_PER_ROUND = 4;
/** 例行检查周期：30 分钟 */
const CHECK_INTERVAL_MS = 30 * 60 * 1000;
/** 挂载后首次检查延迟：5 秒（避开应用启动加载高峰） */
const FIRST_CHECK_DELAY_MS = 5 * 1000;
/** 系统通知固定标题（应用名）；正文来自后端，不含金额等敏感数字 */
const NOTIFY_TITLE = '工资核算助手';
/** 托盘菜单「立即检查」事件（Rust 侧 emit） */
const TRAY_CHECK_EVENT = 'tray-check-reminders';

/**
 * 提醒触达检查器（无 UI 组件）：周期性拉取四源聚合提醒（征期/账期/月结/备份），
 * 对当日新提醒弹 OS 系统通知并刷新托盘待办数。守卫：安全初始化且未锁屏才检查。
 * 全部后端/插件调用静默容错，任何失败不影响应用主流程。
 */
const ReminderChecker: React.FC = () => {
  const { isInitialized, isLocked } = useSecurity();

  // 守卫值 ref：interval 与托盘事件回调里读 ref 取最新值，避免闭包捕获过期状态漏检查
  const guardRef = useRef(false);
  // 系统通知权限：本会话仅首次检查时请求一次，被拒后后续静默跳过（仅托盘 badge 触达）
  const permissionRequestedRef = useRef(false);

  useEffect(() => {
    guardRef.current = isInitialized && !isLocked;
  }, [isInitialized, isLocked]);

  const runCheck = useCallback(async () => {
    if (!guardRef.current) return;

    // 1. 本地通知总开关：关闭则本轮完全静默（含托盘 badge 不刷新）
    try {
      const settings = await getLocalNotifySettings();
      if (!settings.enabled) return;
    } catch (e: unknown) {
      console.error('[ReminderChecker] 读取本地通知设置失败:', e);
      return;
    }

    // 2. 拉取四源聚合提醒（catch 均直接 return，无需初始值）
    let dueList: DueNotification[];
    try {
      dueList = await getDueNotifications();
    } catch (e: unknown) {
      console.error('[ReminderChecker] 获取聚合提醒失败:', e);
      return;
    }

    // 3. 刷新托盘 tooltip 待办数（无论是否有新提醒都刷新，失败不影响后续步骤）
    try {
      await updateTrayBadge(dueList.length);
    } catch (e: unknown) {
      console.error('[ReminderChecker] 刷新托盘待办数失败:', e);
    }

    if (dueList.length === 0) return;

    // 4. 与提醒记忆做差集：仅取当日尚未提醒过的条目（catch 直接 return，无需初始值）
    let freshKeys: string[];
    try {
      const memory = await getNotifyMemory();
      const remembered = new Set(memory?.keys ?? []);
      freshKeys = dueList.map((item) => item.key).filter((key) => !remembered.has(key));
    } catch (e: unknown) {
      console.error('[ReminderChecker] 读取提醒记忆失败:', e);
      return;
    }
    if (freshKeys.length === 0) return;

    // 5. 发系统通知：单轮至多 4 条，超出合并到末条；权限仅本会话请求一次
    const itemByKey = new Map(dueList.map((item) => [item.key, item] as const));
    const toSend = freshKeys
      .slice(0, MAX_NOTIFICATIONS_PER_ROUND)
      .map((key) => itemByKey.get(key));
    const overflowCount = freshKeys.length - toSend.length;
    try {
      if (!(await isPermissionGranted())) {
        if (permissionRequestedRef.current) return;
        permissionRequestedRef.current = true;
        if ((await requestPermission()) !== 'granted') return;
      }
      for (let i = 0; i < toSend.length; i++) {
        const item = toSend[i];
        if (!item) continue;
        const body =
          overflowCount > 0 && i === toSend.length - 1
            ? `${item.body}…等 ${overflowCount + 1} 项待处理`
            : item.body;
        await sendNotification({ title: NOTIFY_TITLE, body });
      }
    } catch (e: unknown) {
      // Linux 无通知守护等场景静默失败，不阻断提醒记忆写入
      console.error('[ReminderChecker] 发送系统通知失败:', e);
    }

    // 6. 合并写入去重指纹（后端按日自动重置）
    try {
      await markNotified(freshKeys);
    } catch (e: unknown) {
      console.error('[ReminderChecker] 记录提醒记忆失败:', e);
    }
  }, []);

  useEffect(() => {
    // 挂载后延迟 5 秒首次检查；此后每 30 分钟例行检查
    const firstTimer = window.setTimeout(() => void runCheck(), FIRST_CHECK_DELAY_MS);
    const interval = window.setInterval(() => void runCheck(), CHECK_INTERVAL_MS);

    // 托盘菜单「立即检查」：注册失败（如浏览器预览）仅记录，周期检查兜底
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    listen(TRAY_CHECK_EVENT, () => void runCheck())
      .then((fn) => {
        if (cancelled) {
          fn();
          return;
        }
        unlisten = fn;
      })
      .catch((e: unknown) => {
        console.error('[ReminderChecker] 注册托盘检查事件失败:', e);
      });

    return () => {
      cancelled = true;
      window.clearTimeout(firstTimer);
      window.clearInterval(interval);
      unlisten?.();
    };
  }, [runCheck]);

  return null;
};

export default ReminderChecker;
