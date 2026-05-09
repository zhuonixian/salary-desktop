# Tauri v2 桌面应用白屏问题排查交接文档

## 项目概述

- **项目名称**: salary-desktop（工资核算助手）
- **技术栈**: Tauri v2 + React 19 + TypeScript + Vite + SQLite (rusqlite) + TailwindCSS
- **目标平台**: Windows 11 (x64)
- **构建方式**: GitHub Actions CI → NSIS 安装包
- **仓库地址**: github.com:zhuonixian/salary-desktop.git
- **当前版本**: v0.1.0

## 当前问题

打包后的 Windows exe 打开后**白屏**，只显示诊断文本，React 应用未渲染。

### 现象

1. exe 启动正常，Rust 后端初始化成功（数据库、插件均 OK）
2. HTML 页面加载正常（`<div id="diag">` 显示内容）
3. inline `<script>` 正常执行
4. JS bundle 文件可访问（XHR 返回 status=200, length=1287580）
5. **但 JS bundle 作为 module 加载后无任何输出，React 未渲染到 `#root`**
6. F12 DevTools 无法打开

### 用户看到的界面内容

```
Step 1: Inline JS works!
Step 2: JS file status=200 length=1287580
Step 3: Module loading...
```

之后无任何变化，既无错误信息也无 React 渲染输出。

### Rust 后端日志（正常）

```
[08:25:03] main() started
[08:25:03] exe: Ok("I:\\work_tools\\salary-desktop\\salary-desktop.exe")
[08:25:03] cwd: Ok("I:\\work_tools\\salary-desktop")
[08:25:03] calling app_lib::run()
[08:25:03] lib::run() entered
[08:25:03] creating Tauri Builder...
[08:25:03] adding dialog plugin...
[08:25:03] adding fs plugin...
[08:25:03] adding log plugin...
[08:25:03] setting up setup callback...
[08:25:03] registering invoke handler...
[08:25:03] calling builder.run()...
[08:25:03] setup() callback entered
[08:25:03] app_data_dir: C:\Users\Administrator\AppData\Roaming\com.salary.desktop
[08:25:03] initializing database at: C:\Users\Administrator\AppData\Roaming\com.salary.desktop
[08:25:03] database initialized OK
[08:25:03] setup() complete, all OK
```

## 已排除的原因

| 已排除 | 证据 |
|--------|------|
| WebView2 未安装 | 注册表确认安装，版本 122.0.2365.92 |
| JS 引擎不工作 | inline script 正常执行，XHR 正常工作 |
| JS 文件无法加载 | XHR status=200, 1.28MB 完整加载 |
| Rust 后端崩溃 | 日志显示 setup() complete, 所有插件正常 |
| Tauri 插件配置错误 | 已修复并验证（dialog/fs 插件配置已移除） |
| 路由模式错误 | 已使用 HashRouter（非 BrowserRouter） |
| Vite base 路径错误 | 已设置 `base: './'` |

## 当前诊断代码

最新版本（commit 87ee1a8）在 `index.html` 中添加了：

- `window.onerror` — 捕获同步 JS 错误
- `window.addEventListener('unhandledrejection')` — 捕获异步错误
- 定时器监控 `#root` 是否有子元素（React 渲染检测）
- 10 秒超时判定

**此版本尚未在 Windows 上测试**（刚推送到 CI，等待构建）。

## 可能的原因猜测

### 1. `crossorigin` 属性问题

Vite 构建输出的 `dist/index.html`：
```html
<script type="module" crossorigin src="./assets/index-CCDINBXX.js"></script>
<link rel="stylesheet" crossorigin href="./assets/index-Duw9aGv_.css">
```

Tauri 使用自定义协议（`tauri://localhost` 或 `https://tauri.localhost`）加载前端资源。`crossorigin` 属性可能导致 WebView2 将模块加载视为跨域请求而静默失败。

### 2. CSP (Content Security Policy) 问题

`tauri.conf.json` 中设置了 `"csp": null`（禁用 CSP），但 WebView2 可能有内置的安全限制影响 `type="module"` 脚本执行。

### 3. ES Module 加载兼容性

WebView2 基于 Chromium，理论上支持 ES modules，但在 Tauri 自定义协议下的行为可能有差异。

### 4. React 19 渲染问题

React 19 的 `createRoot` API 可能在某些 WebView2 环境下有兼容性问题。

## 关键文件路径

```
salary-desktop/
├── index.html                          # 源 HTML（含诊断脚本）
├── vite.config.ts                      # Vite 配置（base: './'）
├── src/
│   ├── main.tsx                        # React 入口
│   └── App.tsx                         # 路由配置（HashRouter）
├── dist/
│   ├── index.html                      # 构建产物（含 crossorigin 属性）
│   └── assets/
│       ├── index-CCDINBXX.js           # JS bundle (1.28MB)
│       └── index-Duw9aGv_.css          # CSS
├── src-tauri/
│   ├── tauri.conf.json                 # Tauri 配置
│   ├── src/
│   │   ├── main.rs                     # Rust 入口（含 panic hook）
│   │   └── lib.rs                      # Tauri Builder 构建（含 diag 日志）
│   └── Cargo.toml                      # Rust 依赖
├── .github/
│   └── workflows/
│       └── build.yml                   # CI 构建配置（仅 Windows）
└── docs/
    └── troubleshooting-white-screen.md # 本文档
```

## 架构要点

### Tauri v2 前端加载机制

1. Rust 启动 → 创建 WebView2 窗口
2. WebView2 通过自定义协议加载 `dist/` 目录下的文件
3. 协议可能是 `tauri://localhost` 或 `https://tauri.localhost`（取决于 Tauri 版本和配置）
4. `frontendDist: "../dist"` 指向构建产物目录

### Vite 构建配置

```typescript
// vite.config.ts
export default defineConfig({
  base: './',  // 相对路径，Tauri 必须
  plugins: [react()],
  resolve: {
    alias: { '@': path.resolve(__dirname, 'src') }
  }
})
```

### React 入口

```typescript
// src/main.tsx
import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import './index.css'

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
)
```

## 已经历的修复历史

1. **Rust 编译错误（40+）** — glob import 名称冲突 → 改为模块级导入
2. **calamine API 变更** — `DataType` → `Data` 枚举
3. **CI 构建：缺少 tauri 脚本** — 添加 `"tauri": "tauri"` 到 scripts
4. **CI 构建：TS7 baseUrl 弃用** — 添加 `ignoreDeprecations: "6.0"`
5. **CI 构建：Release 权限不足** — 添加 `permissions: contents: write`
6. **CI 构建：WiX 不支持中文 productName** — 改为 "salary-desktop"
7. **exe 无响应（无日志）** — tauri.conf.json 中 plugins 配置格式错误 → 移除
8. **白屏（当前）** — JS bundle 加载成功但不执行

## 建议的排查方向

### 方向 A：移除 crossorigin 属性

修改 Vite 配置，避免在构建产物中添加 `crossorigin`：

```typescript
// vite.config.ts - 可能需要自定义插件
build: {
  rollupOptions: {
    output: {
      // 控制输出格式
    }
  }
}
```

或用 `transformIndexHtml` 插件在构建后移除 `crossorigin`。

### 方向 B：验证 WebView2 module 支持

创建最小测试页面，仅包含：
```html
<script type="module">
  document.getElementById('root').textContent = 'Module works!';
</script>
```

### 方向 C：在 Windows 本地开发调试

```powershell
# 在 Windows 上执行
cd salary-desktop
npm install
npm run tauri dev
```

`npm run tauri dev` 使用 Vite dev server (http://localhost:5173)，F12 DevTools 完全可用，可以直接看到 Console 错误。

### 方向 D：检查 Tauri 自定义协议

在 inline 诊断中添加：
```javascript
d.textContent += 'Protocol: ' + window.location.protocol + '\n';
d.textContent += 'Host: ' + window.location.host + '\n';
d.textContent += 'Href: ' + window.location.href + '\n';
```

确认 WebView2 实际使用的协议和 URL 格式。

## 环境

- **开发机**: Linux (Ubuntu), 当前用于编码和 CI 触发
- **目标机**: Windows 11, Administrator 用户, WebView2 122.0.2365.92
- **安装路径**: `I:\work_tools\salary-desktop\`
- **日志路径**: `%TEMP%\salary-desktop-startup.log`
