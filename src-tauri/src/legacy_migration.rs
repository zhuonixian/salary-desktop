//! 旧版（明文）资源迁移：把 Task 4 之前归档的明文发票图片加密、把
//! `app_settings` 中的百度 token / sidecar 配置等敏感字段从明文迁移到加密存储。
//!
//! Task 9 实现具体逻辑；本任务先提供空 `run` stub 让 `security_commands::migrate_legacy_resources`
//! 能编译通过、命令层不抛错。

use crate::errors::AppResult;
use crate::security::SecurityState;
use rusqlite::Connection;

/// 迁移入口。Task 9 会读取 `legacy_migration_state`、扫描明文发票、调用 OCR/备份加密、
/// 通过 `app.emit` 推送进度事件。当前直接返回 Ok 表示"无需迁移 / 已完成"。
pub fn run(_conn: &Connection, _sec: &SecurityState, _app: &tauri::AppHandle) -> AppResult<()> {
    Ok(())
}
