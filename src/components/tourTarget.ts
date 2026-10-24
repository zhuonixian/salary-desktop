/**
 * antd v6 Tour 的 target 需要元素 getter（不支持字符串选择器）。
 * 延迟到 Tour 渲染时按 CSS selector 查找；找不到返回 null → antd 居中展示该步，不依赖 DOM 也安全。
 * 类型上按 () => HTMLElement 收窄（antd 类型不接受 HTMLElement|null 联合，运行时 null 走居中兜底）。
 */
export const tourTarget = (selector: string): (() => HTMLElement) => () =>
  document.querySelector<HTMLElement>(selector) as HTMLElement;
