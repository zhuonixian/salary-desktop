use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db;
use crate::errors::{AppError, AppResult};
use crate::models::{
    DataBackupResult, DataRestoreResult, DataSafetyCheckResult, DataSafetyStatus, DataTableCount,
};

const DATABASE_FILE: &str = "salary.db";
const INVOICE_DIR: &str = "invoices";
const MANIFEST_FILE: &str = "backup_manifest.json";

#[derive(Debug, Serialize, Deserialize)]
struct BackupManifest {
    app: String,
    version: u32,
    created_at: String,
    database_file: String,
    invoice_dir: String,
    database_size: u64,
    invoice_dir_size: u64,
}

pub fn get_status(conn: &Connection, app_data_dir: &Path) -> AppResult<DataSafetyStatus> {
    let database_path = app_data_dir.join(DATABASE_FILE);
    let invoice_dir = app_data_dir.join(INVOICE_DIR);
    let table_counts = collect_table_counts(conn)?;

    Ok(DataSafetyStatus {
        app_data_dir: app_data_dir.to_string_lossy().to_string(),
        database_path: database_path.to_string_lossy().to_string(),
        database_exists: database_path.exists(),
        database_size: file_size(&database_path),
        invoice_dir: invoice_dir.to_string_lossy().to_string(),
        invoice_dir_exists: invoice_dir.exists(),
        invoice_dir_size: dir_size(&invoice_dir)?,
        last_backup_at: db::get_setting(conn, "last_data_backup_at")?,
        last_backup_path: db::get_setting(conn, "last_data_backup_path")?,
        last_restore_at: db::get_setting(conn, "last_data_restore_at")?,
        table_counts,
    })
}

pub fn backup_database(
    conn: &Connection,
    app_data_dir: &Path,
    target_dir: &Path,
) -> AppResult<DataBackupResult> {
    create_backup(conn, app_data_dir, target_dir, "salary-backup")
}

pub fn restore_database(
    conn: &mut Connection,
    app_data_dir: &Path,
    backup_dir: &Path,
) -> AppResult<DataRestoreResult> {
    validate_backup(backup_dir)?;

    let auto_backup_parent = app_data_dir.join("backups");
    fs::create_dir_all(&auto_backup_parent)?;
    let safety_backup = create_backup(
        conn,
        app_data_dir,
        &auto_backup_parent,
        "auto-before-restore",
    )?;

    let database_path = app_data_dir.join(DATABASE_FILE);
    let backup_database_path = backup_dir.join(DATABASE_FILE);
    let invoice_dir = app_data_dir.join(INVOICE_DIR);
    let backup_invoice_dir = backup_dir.join(INVOICE_DIR);

    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    let old_conn = std::mem::replace(conn, Connection::open_in_memory()?);
    drop(old_conn);

    fs::copy(&backup_database_path, &database_path)?;
    if invoice_dir.exists() {
        fs::remove_dir_all(&invoice_dir)?;
    }
    if backup_invoice_dir.exists() {
        copy_dir_recursive(&backup_invoice_dir, &invoice_dir)?;
    } else {
        fs::create_dir_all(&invoice_dir)?;
    }

    let app_data_dir_str = app_data_dir.to_string_lossy().to_string();
    *conn = db::init_db(&app_data_dir_str)?;

    let restored_at = Utc::now().to_rfc3339();
    db::set_setting(conn, "last_data_restore_at", &restored_at)?;
    db::set_setting(
        conn,
        "last_data_restore_path",
        &backup_dir.to_string_lossy(),
    )?;

    Ok(DataRestoreResult {
        success: true,
        restored_at,
        restored_from: backup_dir.to_string_lossy().to_string(),
        safety_backup_dir: safety_backup.backup_dir,
        restart_recommended: true,
    })
}

pub fn verify_database(conn: &Connection) -> AppResult<DataSafetyCheckResult> {
    let checked_at = Utc::now().to_rfc3339();
    let integrity_check: String = conn.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    let mut messages = Vec::new();

    if integrity_check == "ok" {
        messages.push("数据库完整性检查通过".to_string());
    } else {
        messages.push(format!("数据库完整性异常: {integrity_check}"));
    }

    let required_tables = [
        "employees",
        "attendance_records",
        "salary_monthly_results",
        "invoices",
        "reimbursement_claims",
        "operation_logs",
        "app_settings",
    ];
    for table in required_tables {
        if table_exists(conn, table)? {
            messages.push(format!("表 {table} 存在"));
        } else {
            messages.push(format!("表 {table} 缺失"));
        }
    }

    Ok(DataSafetyCheckResult {
        ok: integrity_check == "ok"
            && required_tables
                .iter()
                .all(|t| table_exists(conn, t).unwrap_or(false)),
        checked_at,
        integrity_check,
        messages,
    })
}

pub fn compact_database(conn: &Connection) -> AppResult<bool> {
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;")?;
    Ok(true)
}

pub fn open_app_data_dir(app_data_dir: &Path) -> AppResult<bool> {
    let status = if cfg!(target_os = "windows") {
        Command::new("explorer").arg(app_data_dir).status()
    } else if cfg!(target_os = "macos") {
        Command::new("open").arg(app_data_dir).status()
    } else {
        Command::new("xdg-open").arg(app_data_dir).status()
    }?;

    if status.success() {
        Ok(true)
    } else {
        Err(AppError::General(format!(
            "打开数据目录失败: {}",
            app_data_dir.display()
        )))
    }
}

fn create_backup(
    conn: &Connection,
    app_data_dir: &Path,
    target_dir: &Path,
    prefix: &str,
) -> AppResult<DataBackupResult> {
    if !target_dir.exists() {
        return Err(AppError::InvalidParam(format!(
            "目标目录不存在: {}",
            target_dir.display()
        )));
    }
    if !target_dir.is_dir() {
        return Err(AppError::InvalidParam(format!(
            "目标路径不是目录: {}",
            target_dir.display()
        )));
    }

    let created_at = Utc::now().to_rfc3339();
    let safe_time = Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let backup_dir = target_dir.join(format!("{prefix}-{safe_time}-{}", Uuid::new_v4().simple()));
    fs::create_dir_all(&backup_dir)?;

    let database_backup_path = backup_dir.join(DATABASE_FILE);
    let database_backup_path_str = database_backup_path.to_string_lossy().to_string();
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    conn.execute("VACUUM INTO ?1", params![database_backup_path_str])?;

    let invoice_dir = app_data_dir.join(INVOICE_DIR);
    let invoice_backup_dir = backup_dir.join(INVOICE_DIR);
    if invoice_dir.exists() {
        copy_dir_recursive(&invoice_dir, &invoice_backup_dir)?;
    } else {
        fs::create_dir_all(&invoice_backup_dir)?;
    }

    let database_size = file_size(&database_backup_path);
    let invoice_dir_size = dir_size(&invoice_backup_dir)?;
    let manifest = BackupManifest {
        app: "salary-desktop".to_string(),
        version: 1,
        created_at: created_at.clone(),
        database_file: DATABASE_FILE.to_string(),
        invoice_dir: INVOICE_DIR.to_string(),
        database_size,
        invoice_dir_size,
    };
    let manifest_path = backup_dir.join(MANIFEST_FILE);
    fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;

    Ok(DataBackupResult {
        success: true,
        backup_dir: backup_dir.to_string_lossy().to_string(),
        database_path: database_backup_path.to_string_lossy().to_string(),
        invoice_dir: invoice_backup_dir.to_string_lossy().to_string(),
        manifest_path: manifest_path.to_string_lossy().to_string(),
        database_size,
        invoice_dir_size,
        created_at,
    })
}

fn validate_backup(backup_dir: &Path) -> AppResult<()> {
    if !backup_dir.is_dir() {
        return Err(AppError::InvalidParam(format!(
            "备份目录不存在: {}",
            backup_dir.display()
        )));
    }

    let manifest_path = backup_dir.join(MANIFEST_FILE);
    let database_path = backup_dir.join(DATABASE_FILE);
    if !manifest_path.exists() {
        return Err(AppError::InvalidParam(
            "备份清单 backup_manifest.json 缺失".to_string(),
        ));
    }
    if !database_path.exists() {
        return Err(AppError::InvalidParam(
            "备份数据库 salary.db 缺失".to_string(),
        ));
    }

    let manifest: BackupManifest = serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
    if manifest.app != "salary-desktop" {
        return Err(AppError::InvalidParam(
            "备份清单不是 salary-desktop 数据".to_string(),
        ));
    }

    let backup_conn = Connection::open(&database_path)?;
    let integrity_check: String =
        backup_conn.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity_check != "ok" {
        return Err(AppError::InvalidParam(format!(
            "备份数据库完整性异常: {integrity_check}"
        )));
    }
    Ok(())
}

fn collect_table_counts(conn: &Connection) -> AppResult<Vec<DataTableCount>> {
    let tables = [
        ("employees", "员工"),
        ("attendance_records", "考勤"),
        ("salary_monthly_results", "工资结果"),
        ("invoices", "发票"),
        ("reimbursement_claims", "报销单"),
        ("operation_logs", "操作日志"),
    ];
    let mut counts = Vec::new();
    for (table, label) in tables {
        let count = if table_exists(conn, table)? {
            let sql = format!("SELECT COUNT(*) FROM {table}");
            conn.query_row(&sql, [], |row| row.get::<_, i64>(0))?
        } else {
            0
        };
        counts.push(DataTableCount {
            table_name: table.to_string(),
            label: label.to_string(),
            count,
        });
    }
    Ok(counts)
}

fn table_exists(conn: &Connection, table: &str) -> AppResult<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
        params![table],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn file_size(path: &Path) -> u64 {
    fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn dir_size(path: &Path) -> AppResult<u64> {
    if !path.exists() {
        return Ok(0);
    }
    let mut total = 0;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        if meta.is_dir() {
            total += dir_size(&entry.path())?;
        } else {
            total += meta.len();
        }
    }
    Ok(total)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> AppResult<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path: PathBuf = dst.join(entry.file_name());
        let meta = entry.metadata()?;
        if meta.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("salary-desktop-{name}-{}", Uuid::new_v4().simple()))
    }

    #[test]
    fn test_backup_database_creates_manifest_and_consistent_db() {
        let app_dir = temp_dir("backup-app");
        let backup_parent = temp_dir("backup-out");
        fs::create_dir_all(&app_dir).unwrap();
        fs::create_dir_all(&backup_parent).unwrap();
        fs::create_dir_all(app_dir.join(INVOICE_DIR)).unwrap();
        fs::write(app_dir.join(INVOICE_DIR).join("sample.txt"), "invoice").unwrap();

        let conn = db::init_db(&app_dir.to_string_lossy()).unwrap();
        conn.execute(
            "INSERT INTO employees (employee_no, name, status, created_at, updated_at) VALUES ('E001', '张三', 'active', 'now', 'now')",
            [],
        )
        .unwrap();

        let result = backup_database(&conn, &app_dir, &backup_parent).unwrap();
        assert!(Path::new(&result.manifest_path).exists());
        assert!(Path::new(&result.database_path).exists());
        assert!(Path::new(&result.invoice_dir).join("sample.txt").exists());

        let backup_conn = Connection::open(&result.database_path).unwrap();
        let employee_count: i64 = backup_conn
            .query_row("SELECT COUNT(*) FROM employees", [], |row| row.get(0))
            .unwrap();
        assert_eq!(employee_count, 1);

        let _ = fs::remove_dir_all(app_dir);
        let _ = fs::remove_dir_all(backup_parent);
    }

    #[test]
    fn test_verify_database_reports_ok() {
        let app_dir = temp_dir("verify-app");
        fs::create_dir_all(&app_dir).unwrap();
        let conn = db::init_db(&app_dir.to_string_lossy()).unwrap();

        let result = verify_database(&conn).unwrap();
        assert!(result.ok);
        assert_eq!(result.integrity_check, "ok");
        assert!(result.messages.iter().any(|m| m.contains("employees")));

        let _ = fs::remove_dir_all(app_dir);
    }
}
