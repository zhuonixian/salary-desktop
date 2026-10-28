#!/usr/bin/env node
/**
 * scripts/web_e2e.mjs —— 本项目首个 Web 逻辑 E2E 运行器
 *
 * 自包含单文件，无新增 npm 依赖：
 *   - Node 22 原生 fetch / WebSocket
 *   - /usr/bin/google-chrome 无头 + CDP（--remote-debugging-port）
 *   - 本地 vite dev server（随机端口 20000-40000，避开 5173）
 *
 * 关键坑（实测）：
 *   - spawn chrome / vite 必须净化 env（删除 DISPLAY 与 DBUS_SESSION_BUS_ADDRESS），
 *     否则无头 Chrome 无输出挂起。
 *   - React 受控输入框需原生 value setter + 派发 input 事件。
 *
 * 场景：
 *   A 启动与解锁：锁屏出现 → 任意密码解锁（预览 mock）→ 侧边菜单文本可见
 *   B 五页导航冒烟：bank-transactions / fund-documents / notes-instruments /
 *     fund-accounts / salary 各页标题渲染（stage19 重构回归）
 *   C 凭证打印：/#/vouchers → 演示凭证行 → 行级「打印」→ 预览 Modal
 *     （凭证字号 + 借贷金额 + 合计行）→ 关闭
 *
 * 输出契约：
 *   E2E|<场景名>|OK|<耗时ms>|<一句话说明>
 *   E2E|<场景名>|FAIL|<耗时ms>|<失败原因>
 *   E2E_SUMMARY|pass=<n>|fail=<m>
 * 全过退出码 0，否则 1。失败不中断后续场景。
 */

import { spawn } from 'node:child_process';
import { rmSync } from 'node:fs';
import { setTimeout as sleep } from 'node:timers/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const PROJECT_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const CHROME_BIN = '/usr/bin/google-chrome';

const SCENARIO_A = 'A_启动与解锁';
const SCENARIO_B = 'B_五页导航冒烟';
const SCENARIO_C = 'C_凭证打印';
const ALL_SCENARIOS = [SCENARIO_A, SCENARIO_B, SCENARIO_C];

/* ==================== 小工具 ==================== */

// 20000-40000 随机端口（避开 5173 与常用端口段）。
// 进程内互斥：vite 与 chrome CDP 从同一池抽取，必须去重，否则 ~1/20000 概率相撞导致 CDP 探活失败
const usedPorts = new Set();
const randPort = () => {
  let p;
  do { p = 20000 + Math.floor(Math.random() * 20001); } while (usedPorts.has(p));
  usedPorts.add(p);
  return p;
};

// 输出契约要求单行：压掉竖线/换行并限长
const oneLine = (s) => String(s ?? '')
  .replace(/[|\r\n\t]+/g, ' ')
  .replace(/\s+/g, ' ')
  .trim()
  .slice(0, 300);

// 净化 spawn env：DISPLAY / DBUS_SESSION_BUS_ADDRESS 会让无头 Chrome 挂起（实测踩坑）
function cleanEnv() {
  const env = { ...process.env };
  delete env.DISPLAY;
  delete env.DBUS_SESSION_BUS_ADDRESS;
  return env;
}

/* ==================== 基建：vite / chrome / CDP ==================== */

async function startVite() {
  const port = randPort();
  const child = spawn(path.join(PROJECT_ROOT, 'node_modules/.bin/vite'), [
    '--port', String(port), '--strictPort',
  ], {
    cwd: PROJECT_ROOT,
    env: cleanEnv(),
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let errTail = '';
  child.stderr.on('data', (d) => { errTail = (errTail + d.toString()).slice(-2000); });
  child.stdout.on('data', () => {});
  const deadline = Date.now() + 30000;
  while (Date.now() < deadline) {
    if (child.exitCode !== null) {
      throw new Error(`vite 提前退出 code=${child.exitCode}；stderr 尾部: ${oneLine(errTail.slice(-300))}`);
    }
    try {
      const res = await fetch(`http://127.0.0.1:${port}/`, { signal: AbortSignal.timeout(2000) });
      if (res.ok) return { child, port };
    } catch { /* 未就绪，继续轮询 */ }
    await sleep(400);
  }
  child.kill('SIGKILL');
  throw new Error(`vite 30s 未就绪；stderr 尾部: ${oneLine(errTail.slice(-300))}`);
}

async function startChrome(startUrl) {
  const port = randPort();
  const userDataDir = `/tmp/e2e-${Date.now()}-${Math.floor(Math.random() * 1e6)}`;
  const child = spawn(CHROME_BIN, [
    '--headless',
    '--no-sandbox',
    '--disable-gpu',
    '--disable-dev-shm-usage',
    `--remote-debugging-port=${port}`,
    `--user-data-dir=${userDataDir}`,
    startUrl,
  ], { env: cleanEnv(), stdio: ['ignore', 'ignore', 'pipe'] });
  child.stderr.on('data', () => {}); // 吞掉 Chrome 噪音日志

  for (let i = 0; i < 40; i++) {
    if (child.exitCode !== null) throw new Error(`chrome 提前退出 code=${child.exitCode}`);
    try {
      const ver = await fetch(`http://127.0.0.1:${port}/json/version`, { signal: AbortSignal.timeout(1500) });
      if (ver.ok) {
        const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
        const page = targets.find((t) => t.type === 'page' && t.webSocketDebuggerUrl);
        if (page) return { child, port, userDataDir, wsUrl: page.webSocketDebuggerUrl };
      }
    } catch { /* 未就绪，继续轮询 */ }
    await sleep(500);
  }
  child.kill('SIGKILL');
  throw new Error('chrome CDP 探活 20s 未就绪');
}

// CDP 连接：原生 WebSocket + JSON-RPC，每条消息独立 id，按 id 匹配回应。
// 看门狗：open 挂起与单条调用无回应都会超时报错，避免「端口可达但 WS 挂起」时无限等待
const CDP_OPEN_TIMEOUT_MS = 10000;
const CDP_SEND_TIMEOUT_MS = 30000;
function connectCdp(wsUrl) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(wsUrl);
    const pending = new Map();
    let nextId = 1;
    let settled = false;

    const openTimer = setTimeout(() => {
      if (settled) return;
      settled = true;
      try { ws.close(); } catch { /* ignore */ }
      reject(new Error(`CDP WebSocket 连接超时(${CDP_OPEN_TIMEOUT_MS}ms)`));
    }, CDP_OPEN_TIMEOUT_MS);

    const failAll = (msg) => {
      for (const { rej, timer } of pending.values()) {
        clearTimeout(timer);
        rej(new Error(msg));
      }
      pending.clear();
    };

    ws.addEventListener('open', () => {
      clearTimeout(openTimer);
      settled = true;
      resolve({
        close: () => { try { ws.close(); } catch { /* ignore */ } },
        send(method, params = {}) {
          const id = nextId++;
          return new Promise((res, rej) => {
            const timer = setTimeout(() => {
              if (pending.delete(id)) rej(new Error(`CDP 调用超时(${CDP_SEND_TIMEOUT_MS}ms): ${method}`));
            }, CDP_SEND_TIMEOUT_MS);
            pending.set(id, { res, rej, timer });
            ws.send(JSON.stringify({ id, method, params }));
          });
        },
      });
    });
    ws.addEventListener('message', (ev) => {
      let msg;
      try { msg = JSON.parse(ev.data); } catch { return; }
      if (msg.id !== undefined && pending.has(msg.id)) {
        const { res, rej, timer } = pending.get(msg.id);
        clearTimeout(timer);
        pending.delete(msg.id);
        if (msg.error) rej(new Error(`CDP ${msg.error.message || 'error'}`));
        else res(msg.result);
      }
    });
    ws.addEventListener('error', (e) => {
      clearTimeout(openTimer);
      const msg = `CDP WebSocket 错误: ${e.message || 'unknown'}`;
      if (!settled) { settled = true; reject(new Error(msg)); }
      failAll(msg);
    });
    ws.addEventListener('close', () => {
      clearTimeout(openTimer);
      failAll('CDP 连接已关闭');
    });
  });
}

/* ==================== 页面操作封装 ==================== */

// 在页面里执行表达式，返回 returnByValue 的结果；页面异常转为 Node 侧 Error
async function evalInPage(cdp, expression) {
  const r = await cdp.send('Runtime.evaluate', {
    expression,
    returnByValue: true,
    awaitPromise: true,
  });
  if (r.exceptionDetails) {
    const d = r.exceptionDetails;
    const desc = (d.exception && d.exception.description) || d.text || '未知异常';
    throw new Error('页面脚本执行失败: ' + String(desc).split('\n')[0].slice(0, 200));
  }
  return r.result && r.result.value;
}

// 轮询等待表达式返回 { ok: true, info }；超时抛错并携带最后状态与页面可见文本片段
async function waitFor(cdp, expression, { timeout = 15000, interval = 400, label = '条件' } = {}) {
  const deadline = Date.now() + timeout;
  let last = '';
  while (Date.now() < deadline) {
    try {
      const v = await evalInPage(cdp, expression);
      if (v && v.ok === true) return v;
      if (v && typeof v.info === 'string' && v.info) last = v.info;
      else if (v != null) last = JSON.stringify(v).slice(0, 160);
    } catch (e) {
      last = String((e && e.message) || e).slice(0, 160); // 导航瞬间求值失败可重试
    }
    await sleep(interval);
  }
  let snippet = '';
  try { snippet = await evalInPage(cdp, SNIPPET); } catch { /* 页面可能正在跳转 */ }
  const parts = [`${label}等待超时(${timeout}ms)`];
  if (last) parts.push(`最后状态: ${last}`);
  if (snippet) parts.push(`页面可见文本: ${snippet}`);
  throw new Error(parts.join('；'));
}

// Hash 路由跳转（不整页刷新，保留应用内存态，如已选操作人）
async function navigateHash(cdp, hash) {
  await evalInPage(cdp, `(() => { window.location.hash = ${JSON.stringify(hash)}; return { ok: true }; })()`);
}

/* ==================== 页面侧断言/动作表达式 ==================== */

const SNIPPET = `(() => (document.body ? document.body.innerText.replace(/\\s+/g, ' ').slice(0, 200) : '<无 body>'))()`;

// 场景 A：锁屏可见（副标题「请输入启动密码以继续使用」）
const LOCK_VISIBLE = `(() => {
  const text = document.body ? document.body.innerText : '';
  const ok = text.includes('请输入启动密码');
  return { ok, info: ok ? '' : '锁屏未见；当前可见文本: ' + text.replace(/\\s+/g, ' ').slice(0, 160) };
})()`;

// React 受控输入：原生 setter + input 事件
const FILL_UNLOCK_PASSWORD = `(() => {
  const input = document.querySelector('input[placeholder="请输入启动密码"]');
  if (!input) return { ok: false, info: '未找到密码输入框(placeholder=请输入启动密码)' };
  const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value').set;
  setter.call(input, 'e2e-test-password');
  input.dispatchEvent(new Event('input', { bubbles: true }));
  return { ok: true, info: '已填入测试密码' };
})()`;

const CLICK_UNLOCK_BUTTON = `(() => {
  // antd 会给两字中文按钮插入空格（渲染为「解 锁」），匹配前先压掉空白
  const norm = (s) => (s || '').replace(/\\s+/g, '');
  const btn = [...document.querySelectorAll('button')].find((b) => norm(b.textContent).includes('解锁'));
  if (!btn) return { ok: false, info: '未找到「解锁」按钮' };
  btn.click();
  return { ok: true, info: '已点击解锁' };
})()`;

const SIDEBAR_VISIBLE = `(() => {
  const text = document.body.innerText;
  const problems = [];
  if (!text.includes('工资核算助手')) problems.push('侧边 logo 文本缺失');
  if (!text.includes('首页仪表盘')) problems.push('菜单项「首页仪表盘」缺失');
  if (text.includes('请输入启动密码')) problems.push('仍停留在锁屏');
  const ok = problems.length === 0;
  return { ok, info: ok ? '' : problems.join('；') + '；可见文本: ' + text.replace(/\\s+/g, ' ').slice(0, 160) };
})()`;

// 预关各页 PageTour（首次进入自动弹出，避免遮罩/文案干扰断言）
const DISMISS_TOURS = `(() => {
  const keys = ['bank-transactions', 'fund-documents', 'notes-instruments', 'vouchers', 'fund-accounts', 'salary'];
  for (const k of keys) { try { localStorage.setItem('tour:' + k, 'done'); } catch (e) { /* ignore */ } }
  return { ok: true, info: '已预置 tour:done' };
})()`;

// 操作人门禁：页头下拉状态 / 打开 / 选项 / 点击
// antd v6 注意：选中值是 .ant-select-content.ant-select-content-has-value 的文本，
// 没有 .ant-select-selection-item / .ant-select-selector（旧版类名，保留兜底）。
const OPERATOR_STATE = `(() => {
  const sel = document.querySelector('.app-header-right .ant-select');
  if (!sel) return { ok: false, info: '页头未找到操作人下拉(.app-header-right .ant-select)' };
  const valueEl = sel.querySelector('.ant-select-content.ant-select-content-has-value')
    || sel.querySelector('.ant-select-selection-item');
  const text = valueEl ? valueEl.textContent.trim() : '';
  if (text) return { ok: true, info: '当前操作人: ' + text };
  const ph = sel.querySelector('.ant-select-placeholder, .ant-select-selection-placeholder');
  const phText = ph ? ph.textContent.trim() : '';
  return { ok: false, info: '操作人未选择（Select 无已选值' + (phText ? '，占位: ' + phText : '') + '）' };
})()`;

const OPEN_OPERATOR_DROPDOWN = `(() => {
  const sel = document.querySelector('.app-header-right .ant-select');
  if (!sel) return { ok: false, info: '页头未找到操作人下拉' };
  // antd v6 的 mousedown 开合处理挂在 .ant-select 根节点（无 .ant-select-selector）
  const trigger = sel.querySelector('.ant-select-selector') || sel;
  for (const type of ['mousedown', 'mouseup', 'click']) {
    trigger.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, button: 0 }));
  }
  return { ok: true, info: '已派发下拉打开事件' };
})()`;

const FIRST_OPERATOR_OPTION = `(() => {
  const drops = [...document.querySelectorAll('.ant-select-dropdown')]
    .filter((d) => !d.classList.contains('ant-select-dropdown-hidden') && getComputedStyle(d).display !== 'none');
  for (const d of drops) {
    const opt = d.querySelector('.ant-select-item-option');
    if (opt) return { ok: true, info: opt.textContent.trim() };
  }
  return { ok: false, info: '操作人下拉未展开或无选项' };
})()`;

const CLICK_FIRST_OPERATOR_OPTION = `(() => {
  const drops = [...document.querySelectorAll('.ant-select-dropdown')]
    .filter((d) => !d.classList.contains('ant-select-dropdown-hidden') && getComputedStyle(d).display !== 'none');
  for (const d of drops) {
    const opt = d.querySelector('.ant-select-item-option');
    if (opt) {
      for (const type of ['mousedown', 'mouseup', 'click']) {
        opt.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, button: 0 }));
      }
      return { ok: true, info: '已点击选项: ' + opt.textContent.trim() };
    }
  }
  return { ok: false, info: '操作人下拉选项消失' };
})()`;

// 页面标题断言：限定在内容区 .app-main，避免侧边栏同名菜单项导致假通过；
// 同时侦测操作人门禁拦截（警告页也渲染在 .app-main 内）。
const pageTextOf = (title) => `(() => {
  const scope = document.querySelector('.app-main') || document.body;
  const text = scope.innerText;
  if (text.includes('尚未选择当前操作人')) {
    return { ok: false, info: '被操作人门禁拦截（页面显示「尚未选择当前操作人」警告）' };
  }
  const ok = text.includes(${JSON.stringify(title)});
  return { ok, info: ok ? '' : '标题 ' + ${JSON.stringify(title)} + ' 未出现在内容区；内容区可见文本: ' + text.replace(/\\s+/g, ' ').slice(0, 160) };
})()`;

// 场景 C：行级打印按钮（凭证页打印按钮只存在于表格行内操作列；
// 不做「页面任意打印按钮」兜底——那会点中弹层内「打印 / 另存 PDF」等其它按钮，削弱断言）
const FIND_PRINT_BUTTON = `(() => {
  const norm = (b) => (b.textContent || '').replace(/\\s+/g, '').trim();
  const inTable = [...document.querySelectorAll('.ant-table-tbody button')].filter((b) => norm(b) === '打印');
  if (inTable.length === 0) {
    const rows = document.querySelectorAll('.ant-table-tbody .ant-table-row').length;
    return { ok: false, info: '未找到表格行内「打印」按钮（表格数据行 ' + rows + ' 行）——凭证打印功能可能未就绪或演示凭证未渲染；可见文本: ' + document.body.innerText.replace(/\\s+/g, ' ').slice(0, 160) };
  }
  return { ok: true, info: '表格行内打印按钮 x' + inTable.length };
})()`;

const CLICK_FIRST_PRINT = `(() => {
  const norm = (b) => (b.textContent || '').replace(/\\s+/g, '').trim();
  const inTable = [...document.querySelectorAll('.ant-table-tbody button')].filter((b) => norm(b) === '打印');
  const target = inTable[0];
  if (!target) return { ok: false, info: '表格行内打印按钮不存在' };
  target.click();
  return { ok: true, info: '已点击打印' };
})()`;

const FIRST_ROW_VOUCHER_NO = `(() => {
  const row = document.querySelector('.ant-table-tbody .ant-table-row');
  if (!row) return { ok: false, info: '表格无数据行', no: '' };
  // 展开行首格是展开图标单元格（空文本），凭证号取第一个非空单元格
  const tds = [...row.querySelectorAll('td')];
  const cell = tds.find((td) => td.textContent.replace(/\\s+/g, '').trim().length > 0);
  const no = cell ? cell.textContent.replace(/\\s+/g, '').trim() : '';
  return { ok: no.length > 0, info: no || '首行所有单元格为空', no };
})()`;

// 打印预览弹层：唯一锁定 VoucherPrintModal——其标题含「打印预览」、正文含「凭证字号」。
// 只查「合计」不行：手工凭证 Modal 同含 借方合计/贷方合计 文本，无法区分。
// antd v6 Modal 无 .ant-modal-content，正文在 .ant-modal（均保留旧类名兜底）
const PREVIEW_MODAL = `(() => {
  const texts = [];
  const wraps = [...document.querySelectorAll('.ant-modal-wrap, .ant-drawer')];
  for (const el of wraps) {
    if (getComputedStyle(el).display === 'none') continue;
    const content = el.querySelector('.ant-modal-content') || el.querySelector('.ant-modal')
      || el.querySelector('.ant-drawer-content') || el;
    if (!content || getComputedStyle(content).display === 'none') continue;
    const t = content.innerText;
    if (t) texts.push(t);
  }
  for (const d of document.querySelectorAll('[role="dialog"]')) {
    if (getComputedStyle(d).display === 'none') continue;
    const t = d.innerText;
    if (t) texts.push(t);
  }
  if (texts.length === 0) return { ok: false, info: '无可见弹层' };
  for (const t of texts) {
    if (t.includes('打印预览') && t.includes('凭证字号')) return { ok: true, info: t };
  }
  return { ok: false, info: '弹层可见但非凭证打印预览（缺「打印预览」或「凭证字号」标识）；弹层文本: ' + texts.join(' | ').replace(/\\s+/g, ' ').slice(0, 160) };
})()`;

const CLOSE_PREVIEW = `(() => {
  const wraps = [...document.querySelectorAll('.ant-modal-wrap')]
    .filter((el) => getComputedStyle(el).display !== 'none');
  for (const w of wraps) {
    const icon = w.querySelector('.ant-modal-close, .ant-drawer-close');
    if (icon) { icon.click(); return { ok: true, info: '点击关闭图标' }; }
    const textBtn = [...w.querySelectorAll('button')].find((b) => /^(关闭|取消|完成|知道了)$/.test((b.textContent || '').replace(/\\s+/g, '')));
    if (textBtn) { textBtn.click(); return { ok: true, info: '点击关闭按钮: ' + textBtn.textContent.trim() }; }
  }
  const drawers = [...document.querySelectorAll('.ant-drawer')]
    .filter((el) => getComputedStyle(el).display !== 'none');
  for (const d of drawers) {
    const icon = d.querySelector('.ant-drawer-close, .ant-modal-close');
    if (icon) { icon.click(); return { ok: true, info: '点击抽屉关闭图标' }; }
  }
  return { ok: false, info: '未找到可见弹层的关闭控件' };
})()`;

const MODAL_GONE = `(() => {
  // antd v6 关闭后 .ant-modal-wrap 保留在 DOM（display:none），以可见性判定
  const visibleModals = [...document.querySelectorAll('.ant-modal-wrap')]
    .filter((el) => getComputedStyle(el).display !== 'none');
  const visibleDrawers = [...document.querySelectorAll('.ant-drawer.ant-drawer-open .ant-drawer-content')]
    .filter((el) => getComputedStyle(el).display !== 'none');
  const rest = visibleModals.length + visibleDrawers.length;
  return { ok: rest === 0, info: rest === 0 ? '' : '仍有 ' + rest + ' 个弹层可见' };
})()`;

/* ==================== 场景实现 ==================== */

// A：锁屏出现 → 输入任意密码解锁（预览 mock）→ 侧边菜单文本可见
async function scenarioA(cdp) {
  await waitFor(cdp, LOCK_VISIBLE, { timeout: 25000, interval: 500, label: '锁屏「请输入启动密码」' });
  const fill = await evalInPage(cdp, FILL_UNLOCK_PASSWORD);
  if (!fill || !fill.ok) throw new Error((fill && fill.info) || '填入密码失败');
  const click = await evalInPage(cdp, CLICK_UNLOCK_BUTTON);
  if (!click || !click.ok) throw new Error((click && click.info) || '点击「解锁」失败');
  await waitFor(cdp, SIDEBAR_VISIBLE, { timeout: 15000, interval: 500, label: '解锁后侧边菜单' });
  return '锁屏出现→任意密码解锁成功→侧边菜单（工资核算助手 / 首页仪表盘）可见';
}

// 操作人门禁前置：到 /#/fund-accounts，在页头下拉选择第一个操作人
async function ensureOperator(cdp) {
  await navigateHash(cdp, '/fund-accounts');
  await waitFor(cdp, pageTextOf('资金账户与基础资料'), { timeout: 15000, interval: 400, label: '页面「资金账户与基础资料」' });
  for (let attempt = 1; attempt <= 2; attempt++) {
    const st = await evalInPage(cdp, OPERATOR_STATE);
    if (st && st.ok) return; // 预览 mock 可能已带默认操作人
    const open = await evalInPage(cdp, OPEN_OPERATOR_DROPDOWN);
    if (!open || !open.ok) throw new Error((open && open.info) || '打开操作人下拉失败');
    await waitFor(cdp, FIRST_OPERATOR_OPTION, { timeout: 6000, interval: 400, label: `操作人下拉选项(第${attempt}次)` });
    const clickOpt = await evalInPage(cdp, CLICK_FIRST_OPERATOR_OPTION);
    if (!clickOpt || !clickOpt.ok) continue;
    try {
      await waitFor(cdp, OPERATOR_STATE, { timeout: 6000, interval: 400, label: `操作人已选择(第${attempt}次)` });
      return;
    } catch { /* 落到下一轮重试 */ }
  }
  throw new Error('两次尝试后页头操作人下拉仍无已选值');
}

// B：五页导航冒烟（stage19 重构回归），逐页断言标题渲染
const SMOKE_PAGES = [
  { hash: '/bank-transactions', title: '银行对账' },
  { hash: '/fund-documents', title: '收付款单' },
  { hash: '/notes-instruments', title: '票据台账' },
  { hash: '/fund-accounts', title: '资金账户与基础资料' },
  { hash: '/salary', title: '月度工资计算' },
];

async function scenarioB(cdp) {
  const okParts = [];
  const failures = [];
  for (const p of SMOKE_PAGES) {
    const t0 = Date.now();
    try {
      await navigateHash(cdp, p.hash);
      await waitFor(cdp, pageTextOf(p.title), { timeout: 15000, interval: 400, label: `页面「${p.title}」` });
      okParts.push(`${p.title} ${Date.now() - t0}ms`);
    } catch (e) {
      failures.push(`「${p.title}」(${p.hash}): ${e.message}`);
    }
  }
  if (failures.length > 0) throw new Error(failures.join(' || '));
  return `5/5 页标题渲染正常: ${okParts.join(' / ')}`;
}

// C：凭证打印。功能未就绪（无行级「打印」按钮）时按契约 FAIL 并说明。
async function scenarioC(cdp) {
  await navigateHash(cdp, '/vouchers');
  await waitFor(cdp, pageTextOf('记账凭证'), { timeout: 15000, interval: 400, label: '页面「记账凭证」' });

  await waitFor(cdp, FIND_PRINT_BUTTON, { timeout: 10000, interval: 500, label: '行级「打印」按钮' });

  const rowNo = await evalInPage(cdp, FIRST_ROW_VOUCHER_NO);
  const voucherNo = rowNo && rowNo.ok ? String(rowNo.no || '') : '';

  const clicked = await evalInPage(cdp, CLICK_FIRST_PRINT);
  if (!clicked || !clicked.ok) throw new Error((clicked && clicked.info) || '点击「打印」失败');

  const modal = await waitFor(cdp, PREVIEW_MODAL, { timeout: 10000, interval: 400, label: '打印预览弹层（含合计行）' });
  const modalText = String((modal && modal.info) || '');

  const problems = [];
  if (voucherNo) {
    const digits = voucherNo.replace(/\D/g, '');
    const matched = modalText.includes(voucherNo) || (digits.length >= 3 && modalText.includes(digits));
    if (!matched) problems.push(`预览未见凭证字号「${voucherNo}」`);
  }
  if (!modalText.includes('借') || !modalText.includes('贷')) problems.push('预览未见借贷金额列');
  if (!/(\d[\d,]*\.\d{2})|\*{3,}/.test(modalText)) problems.push('预览未见金额数字');
  if (problems.length > 0) {
    throw new Error(problems.join('；') + '；弹层文本: ' + modalText.replace(/\s+/g, ' ').slice(0, 160));
  }

  const closed = await evalInPage(cdp, CLOSE_PREVIEW);
  if (!closed || !closed.ok) throw new Error((closed && closed.info) || '关闭预览失败');
  await waitFor(cdp, MODAL_GONE, { timeout: 6000, interval: 300, label: '预览弹层关闭' });

  const masked = modalText.includes('****');
  return `打印预览 OK：凭证字号${voucherNo ? `「${voucherNo}」` : '（首行）'}、借贷金额与合计行可见${masked ? '（金额处于脱敏态 ¥****）' : ''}，弹层已关闭`;
}

/* ==================== 运行与输出契约 ==================== */

let viteProc = null;
let chromeProc = null;
let chromeProfileDir = null;

const results = [];
function record(name, ok, ms, note) {
  results.push({ name, ok });
  console.log(`E2E|${name}|${ok ? 'OK' : 'FAIL'}|${ms}|${oneLine(note)}`);
}
async function run(name, fn) {
  const t0 = Date.now();
  try {
    const note = await fn();
    record(name, true, Date.now() - t0, note);
  } catch (e) {
    record(name, false, Date.now() - t0, (e && e.message) || String(e));
  }
}

async function main() {
  let cdp = null;
  try {
    const vite = await startVite();
    viteProc = vite.child;
    const base = `http://127.0.0.1:${vite.port}/#/`;

    const chrome = await startChrome(base);
    chromeProc = chrome.child;
    chromeProfileDir = chrome.userDataDir;

    cdp = await connectCdp(chrome.wsUrl);
    await cdp.send('Runtime.enable');
    await cdp.send('Page.enable');
    // chrome 已随启动加载该 URL；这里再显式导航一次，保证从全新状态开始
    await cdp.send('Page.navigate', { url: base });

    await run(SCENARIO_A, () => scenarioA(cdp));

    // 门禁前置（非场景）：预关 PageTour + 选择操作人；失败不中断，B/C 会带着原因各自 FAIL
    let setupErr = null;
    try {
      await evalInPage(cdp, DISMISS_TOURS);
      await ensureOperator(cdp);
    } catch (e) {
      setupErr = e;
    }

    await run(SCENARIO_B, async () => {
      if (setupErr) throw new Error(`前置失败（操作人选择）: ${setupErr.message}`);
      return scenarioB(cdp);
    });
    await run(SCENARIO_C, () => scenarioC(cdp));
  } catch (e) {
    // 基建失败：三场景全部记 FAIL，仍满足输出契约
    const msg = '基建失败: ' + ((e && e.message) || String(e));
    for (const name of ALL_SCENARIOS) record(name, false, 0, msg);
  } finally {
    try { if (cdp) cdp.close(); } catch { /* ignore */ }
    try { if (chromeProc) chromeProc.kill('SIGKILL'); } catch { /* ignore */ }
    try { if (viteProc) viteProc.kill('SIGKILL'); } catch { /* ignore */ }
    if (chromeProfileDir) {
      try { rmSync(chromeProfileDir, { recursive: true, force: true }); } catch { /* ignore */ }
    }
  }

  const pass = results.filter((r) => r.ok).length;
  const fail = results.length - pass;
  console.log(`E2E_SUMMARY|pass=${pass}|fail=${fail}`);
  process.exit(fail === 0 ? 0 : 1);
}

// 子进程兜底清理：'exit' 覆盖正常/异常退出；SIGINT(130)/SIGTERM(143) 覆盖中断与 CI 终止。
// SIGTERM 无处理器时 Node 直接终止，finally 与 'exit' 均不执行，vite/chrome 会成孤儿进程（实测发生过）
function killChildren() {
  try { if (chromeProc) chromeProc.kill('SIGKILL'); } catch { /* ignore */ }
  try { if (viteProc) viteProc.kill('SIGKILL'); } catch { /* ignore */ }
}

process.on('exit', killChildren);
process.on('SIGINT', () => {
  killChildren();
  process.exit(130);
});
process.on('SIGTERM', () => {
  killChildren();
  process.exit(143);
});

main().catch((e) => {
  console.error('E2E 运行器异常:', e);
  process.exit(1);
});
