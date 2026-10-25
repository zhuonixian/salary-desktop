// 资金账户页面与其抽取组件共用的轻量工具（保持与原页面内实现完全一致）。

/** 把未知异常转成可展示的文案（Error 取 message，其余走 String）。 */
export const errText = (e: unknown): string => (e instanceof Error ? e.message : String(e));

/** 金额格式化：千分位 + 固定两位小数（zh-CN）。 */
export const fmtAmount = (value: number): string =>
  (Number(value) || 0).toLocaleString('zh-CN', { minimumFractionDigits: 2, maximumFractionDigits: 2 });
