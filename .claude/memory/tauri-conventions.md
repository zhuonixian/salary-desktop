---
name: tauri-conventions
description: Tauri 2 命令命名、State 管理、asset 协议、文件路径约定
---

# Tauri 2 约定

## 命令命名

- **后端**：`#[tauri::command] pub fn xxx_yyy_zzz(args..., state: tauri::State<'_, Mutex<Connection>>) -> Result<T, AppError>`
- **前端**：`invoke<T>('xxx_yyy_zzz', { snake_case_param: value })` — 命令名和参数键都是 snake_case
- **注册**：`src-tauri/src/lib.rs` 的 `tauri::generate_handler![commands::xxx, ...]`

## State / 数据库锁

- 单一全局 `Mutex<Connection>` 通过 `app.manage(Mutex::new(conn))` 注册
- 短任务：`let conn = state.lock().map_err(|e| AppError::General(e.to_string()))?;`
- 长任务（HTTP / 图片复制）：避免持锁跨网络/磁盘 —— 用 `InvoiceOcrDbOps` trait 模式，trait impl 内部短暂 lock 后释放
- 参见 `src-tauri/src/invoice.rs:InvoiceOcrDbOps` 实现

## 文件路径

- **app_data_dir**：通过 `app.path().app_data_dir()` 获取，命令层传 `app: tauri::AppHandle` 参数
  - Linux: `~/.local/share/com.salary.desktop/`
  - Windows: `%APPDATA%\com.salary.desktop\`
  - macOS: `~/Library/Application Support/com.salary.desktop/`
- **DB 路径**：`{app_data_dir}/salary.db`
- **发票归档**：`{app_data_dir}/invoices/{belong_month}/{timestamp}_{sanitized_filename}`
  - `belong_month` 经 `sanitize_invoice_filename` 净化（仅字母数字/`-`/`_`）
  - 文件名同样净化

## asset 协议（图片预览）

`src-tauri/tauri.conf.json` 已配置：
```json
"security": {
  "csp": null,
  "assetProtocol": {
    "enable": true,
    "scope": ["**"]
  }
}
```
+ `Cargo.toml` 加 `tauri = { features = ["protocol-asset"] }`

前端预览本地文件：
```ts
import { convertFileSrc } from '@tauri-apps/api/core';
const url = convertFileSrc(localFilePath);  // 转 tauri://localhost/...
// <img src={url}> 或 <iframe src={url}>
```

## Tauri 插件

`src-tauri/src/lib.rs` 注册：
- `tauri_plugin_dialog::init()` — 文件选择/保存对话框
- `tauri_plugin_fs::init()` — 文件系统访问
- `tauri_plugin_log::Builder` — 日志，输出到 LogDir + stdout，LevelFilter::Info

## 应用启动诊断

`lib.rs` 的 `diag(msg)` 函数写日志到 `std::env::temp_dir().join("salary-desktop-startup.log")`，用于排查启动白屏问题（见 `docs/troubleshooting-white-screen.md`）。

## 数据库初始化

`db::init_db(app_data_dir)` 在 setup 回调中调用：
- WAL 模式（`PRAGMA journal_mode=WAL`）
- 外键开启（`PRAGMA foreign_keys=ON`）
- `create_tables()` 用单 `execute_batch` 含 `IF NOT EXISTS`，idempotent
- `insert_default_data()` 用 `if count == 0` 守卫，idempotent
- 新增表/索引时追加到这两个函数末尾，不要新建 execute_batch

## Python OCR Sidecar

- 位于 `python-ocr/`，随包分发（`tauri.conf.json` 的 `bundle.resources`）
- 调用方式：`std::process::Command::new("python3")`（Windows 用 `py -3` / `python`）
- 用途：考勤 OCR 本地识别（PaddleOCR），发票 OCR 不走这里（直接调百度）
