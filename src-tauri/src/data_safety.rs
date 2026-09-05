use std::fs;
use std::io::Cursor;
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
use crate::security::{self, SecurityState, BACKUP_MAGIC};

const DATABASE_FILE: &str = "salary.db";
const INVOICE_DIR: &str = "invoices";
/// 业务附件归档目录（第七阶段）：随备份/恢复/体检一并覆盖（spec 4.6）
const ATTACHMENT_DIR: &str = "attachments";
const MANIFEST_FILE: &str = "backup_manifest.json";

#[derive(Debug, Serialize, Deserialize)]
struct BackupManifest {
    app: String,
    version: u32,
    created_at: String,
    database_file: String,
    invoice_dir: String,
    #[serde(default)]
    attachment_dir: Option<String>,
    database_size: u64,
    invoice_dir_size: u64,
    #[serde(default)]
    attachment_dir_size: Option<u64>,
}

pub fn get_status(conn: &Connection, app_data_dir: &Path) -> AppResult<DataSafetyStatus> {
    let database_path = app_data_dir.join(DATABASE_FILE);
    let invoice_dir = app_data_dir.join(INVOICE_DIR);
    let table_counts = collect_table_counts(conn)?;

    // 第七阶段安全联动统计（spec 8）：附件/资金表/迁移状态/孤儿文件
    let (attachment_count, attachment_encrypted_count) =
        if table_exists(conn, "business_attachments")? {
            conn.query_row(
                "SELECT COUNT(*), COALESCE(SUM(encrypted), 0) FROM business_attachments",
                [],
                |row| {
                    let total: i64 = row.get(0)?;
                    let encrypted: i64 = row.get(1)?;
                    Ok((total, encrypted))
                },
            )?
        } else {
            (0, 0)
        };
    let (attachment_orphan_count, attachment_missing_count) =
        attachment_disk_stats(conn, app_data_dir)?;
    let stage7_migration_status: Option<String> = db::get_setting(conn, "stage7_migration_status")?;
    let stage7_pending_count: Option<i64> =
        db::get_setting(conn, "stage7_migration_pending_count")?
            .and_then(|v| v.parse::<i64>().ok());

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
        attachment_count,
        attachment_encrypted_count,
        attachment_orphan_count,
        attachment_missing_count,
        stage7_migration_status,
        stage7_pending_count,
    })
}

/// 附件磁盘一致性统计（不产生告警消息，仅计数）：
/// 孤儿 = 磁盘上有、business_attachments 无引用；缺失 = 有记录、磁盘上没有。
fn attachment_disk_stats(conn: &Connection, app_data_dir: &Path) -> AppResult<(i64, i64)> {
    let dir = app_data_dir.join(ATTACHMENT_DIR);
    if !dir.exists() {
        return Ok((0, 0));
    }
    let referenced: Vec<String> = {
        let mut stmt = conn.prepare("SELECT file_path FROM business_attachments")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    let mut on_disk: Vec<String> = Vec::new();
    collect_files_light(&dir, &mut on_disk)?;
    let orphans = on_disk.iter().filter(|p| !referenced.contains(*p)).count() as i64;
    let missing = referenced
        .iter()
        .filter(|p| !Path::new(p).is_file())
        .count() as i64;
    Ok((orphans, missing))
}

pub fn backup_database(
    conn: &Connection,
    app_data_dir: &Path,
    target_dir: &Path,
    encrypt: bool,
    sec: &SecurityState,
) -> AppResult<DataBackupResult> {
    let dir_result = create_backup(conn, app_data_dir, target_dir, "salary-backup")?;

    if !encrypt {
        return Ok(dir_result);
    }

    // 加密模式:把 backup_dir 整树打包成 packed payload,加密后写单个 .enc 文件,
    // 然后删除临时 backup_dir。最终交付物是 .enc 文件。
    let dek = sec
        .dek()
        .ok_or_else(|| AppError::InvalidParam("请先解锁应用".into()))?;
    let backup_dir_path = PathBuf::from(&dir_result.backup_dir);

    let payload = pack_backup_dir(&backup_dir_path)?;
    let (cipher, nonce) = security::encrypt_bytes(&payload, &dek)?;

    let safe_time = Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let enc_file_name = format!("salary-backup-{safe_time}-{}.enc", Uuid::new_v4().simple());
    let enc_path = target_dir.join(&enc_file_name);

    let mut buf = Vec::with_capacity(BACKUP_MAGIC.len() + 12 + cipher.len());
    buf.extend_from_slice(BACKUP_MAGIC);
    buf.extend_from_slice(&nonce);
    buf.extend_from_slice(&cipher);
    fs::write(&enc_path, &buf)?;

    // 删除中间产物 backup_dir
    let _ = fs::remove_dir_all(&backup_dir_path);

    let created_at = dir_result.created_at.clone();
    let total_size = file_size(&enc_path);
    Ok(DataBackupResult {
        success: true,
        backup_dir: enc_path.to_string_lossy().to_string(),
        database_path: enc_path.to_string_lossy().to_string(),
        invoice_dir: enc_path.to_string_lossy().to_string(),
        manifest_path: enc_path.to_string_lossy().to_string(),
        database_size: total_size,
        invoice_dir_size: 0,
        created_at,
    })
}

pub fn restore_database(
    conn: &mut Connection,
    app_data_dir: &Path,
    backup_path: &Path,
    sec: &SecurityState,
) -> AppResult<DataRestoreResult> {
    // 1. 判断加密 / 明文。加密 → 先解密解包到临时 backup_dir,再走目录流程。
    let effective_backup_dir: PathBuf = if is_encrypted_backup(backup_path)? {
        let dek = sec
            .dek()
            .ok_or_else(|| AppError::InvalidParam("请先输入启动密码解锁应用".into()))?;
        let temp_unpack = app_data_dir.join(format!("restore-unpack-{}", Uuid::new_v4().simple()));
        fs::create_dir_all(&temp_unpack)?;
        unpack_encrypted_backup(backup_path, &dek, &temp_unpack)?;
        temp_unpack
    } else {
        backup_path.to_path_buf()
    };

    validate_backup(&effective_backup_dir)?;

    let auto_backup_parent = app_data_dir.join("backups");
    fs::create_dir_all(&auto_backup_parent)?;
    let safety_backup = create_backup(
        conn,
        app_data_dir,
        &auto_backup_parent,
        "auto-before-restore",
    )?;

    let database_path = app_data_dir.join(DATABASE_FILE);
    let backup_database_path = effective_backup_dir.join(DATABASE_FILE);
    let invoice_dir = app_data_dir.join(INVOICE_DIR);
    let backup_invoice_dir = effective_backup_dir.join(INVOICE_DIR);
    let attachment_dir = app_data_dir.join(ATTACHMENT_DIR);
    let backup_attachment_dir = effective_backup_dir.join(ATTACHMENT_DIR);

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
    // 业务附件目录：与发票目录同规则恢复（旧版备份无 attachments/ 时建空目录兜底）
    if attachment_dir.exists() {
        fs::remove_dir_all(&attachment_dir)?;
    }
    if backup_attachment_dir.exists() {
        copy_dir_recursive(&backup_attachment_dir, &attachment_dir)?;
    } else {
        fs::create_dir_all(&attachment_dir)?;
    }

    let app_data_dir_str = app_data_dir.to_string_lossy().to_string();
    *conn = db::init_db(&app_data_dir_str)?;

    let restored_at = Utc::now().to_rfc3339();
    db::set_setting(conn, "last_data_restore_at", &restored_at)?;
    db::set_setting(
        conn,
        "last_data_restore_path",
        &backup_path.to_string_lossy(),
    )?;

    // 解密恢复场景下,临时解包目录清理(已 copy 完毕)
    if effective_backup_dir != backup_path {
        let _ = fs::remove_dir_all(&effective_backup_dir);
    }

    Ok(DataRestoreResult {
        success: true,
        restored_at,
        restored_from: backup_path.to_string_lossy().to_string(),
        safety_backup_dir: safety_backup.backup_dir,
        restart_recommended: true,
    })
}

/// 判断给定的文件是否是加密备份:是文件且前 8 字节 = BACKUP_MAGIC。
fn is_encrypted_backup(path: &Path) -> AppResult<bool> {
    if !path.is_file() {
        return Ok(false);
    }
    let head = read_head(path, BACKUP_MAGIC.len())?;
    Ok(head.as_slice() == BACKUP_MAGIC.as_slice())
}

fn read_head(path: &Path, n: usize) -> AppResult<Vec<u8>> {
    use std::io::Read;
    let mut f = fs::File::open(path)?;
    let mut buf = vec![0u8; n];
    let read = f.read(&mut buf)?;
    buf.truncate(read);
    Ok(buf)
}

/// 加密备份解密 → 解包到 out_dir。
fn unpack_encrypted_backup(enc_path: &Path, dek: &[u8; 32], out_dir: &Path) -> AppResult<()> {
    let data = fs::read(enc_path)?;
    if data.len() < BACKUP_MAGIC.len() + 12 {
        return Err(AppError::InvalidParam("加密备份文件损坏".into()));
    }
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&data[BACKUP_MAGIC.len()..BACKUP_MAGIC.len() + 12]);
    let cipher = &data[BACKUP_MAGIC.len() + 12..];
    let plain = security::decrypt_bytes(cipher, &nonce, dek)?;
    unpack_payload(&plain, out_dir)
}

// ===== 备份打包格式(packed format) =====
//
// 不引入 zip 依赖,采用自描述的简单拼接格式。加密前整体打包,解密后整体解包。
//
// ```text
// file_count: u32 LE
// repeated for each file:
//   relpath_len: u32 LE
//   relpath_utf8: [u8; relpath_len]    (以 '/' 分隔, 不含前导 /)
//   data_len:    u64 LE
//   data:        [u8; data_len]
// ```

fn pack_backup_dir(backup_dir: &Path) -> AppResult<Vec<u8>> {
    let mut files: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    collect_files(backup_dir, backup_dir, &mut files)?;

    let mut out = Vec::new();
    let count = files.len() as u32;
    out.extend_from_slice(&count.to_le_bytes());
    for (rel, data) in &files {
        let rel_str = rel
            .to_str()
            .ok_or_else(|| AppError::General("备份文件路径含非 UTF-8 字符".into()))?
            .replace('\\', "/");
        let rel_bytes = rel_str.as_bytes();
        out.extend_from_slice(&(rel_bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(rel_bytes);
        out.extend_from_slice(&(data.len() as u64).to_le_bytes());
        out.extend_from_slice(data);
    }
    Ok(out)
}

fn collect_files(root: &Path, current: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) -> AppResult<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let meta = entry.metadata()?;
        if meta.is_dir() {
            collect_files(root, &path, out)?;
        } else {
            let rel = path
                .strip_prefix(root)
                .map_err(|e| AppError::General(e.to_string()))?
                .to_path_buf();
            let data = fs::read(&path)?;
            out.push((rel, data));
        }
    }
    Ok(())
}

fn unpack_payload(payload: &[u8], out_dir: &Path) -> AppResult<()> {
    fs::create_dir_all(out_dir)?;
    let mut cursor = Cursor::new(payload);
    use std::io::Read;
    let mut count_buf = [0u8; 4];
    cursor.read_exact(&mut count_buf)?;
    let count = u32::from_le_bytes(count_buf);

    for _ in 0..count {
        let mut len_buf = [0u8; 4];
        cursor.read_exact(&mut len_buf)?;
        let relpath_len = u32::from_le_bytes(len_buf) as usize;
        if relpath_len > 4096 {
            return Err(AppError::General("备份 manifest 路径异常长".into()));
        }
        let mut relpath_buf = vec![0u8; relpath_len];
        cursor.read_exact(&mut relpath_buf)?;
        let relpath_str = std::str::from_utf8(&relpath_buf)
            .map_err(|e| AppError::General(format!("备份路径 UTF-8 异常: {e}")))?;
        // 防路径穿越:不允许 .. 段
        if relpath_str.contains("..") || relpath_str.starts_with('/') {
            return Err(AppError::InvalidParam(format!(
                "备份包含非法路径: {relpath_str}"
            )));
        }

        let mut data_len_buf = [0u8; 8];
        cursor.read_exact(&mut data_len_buf)?;
        let data_len = u64::from_le_bytes(data_len_buf) as usize;
        let mut data = vec![0u8; data_len];
        cursor.read_exact(&mut data)?;

        let dst = out_dir.join(relpath_str);
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&dst, &data)?;
    }
    Ok(())
}

/// 数据体检：数据库完整性 + 必备表存在性 +（提供 app_data_dir 时）附件目录一致性。
/// 附件检查覆盖双向偏差，均为 warning 级（不影响 ok 结论，供用户清理/补救）：
/// - 孤儿文件：磁盘上有、business_attachments 无引用（含 .enc.tmp 残留）；
/// - 缺失文件：business_attachments 有记录、磁盘上没有（未随备份恢复/被手动删除）。
pub fn verify_database(
    conn: &Connection,
    app_data_dir: Option<&Path>,
) -> AppResult<DataSafetyCheckResult> {
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
        "business_attachments",
    ];
    for table in required_tables {
        if table_exists(conn, table)? {
            messages.push(format!("表 {table} 存在"));
        } else {
            messages.push(format!("表 {table} 缺失"));
        }
    }

    if let Some(dir) = app_data_dir {
        check_attachment_consistency(conn, dir, &mut messages)?;
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

/// 附件目录一致性体检（spec 4.6：数据体检必须覆盖 attachments/）。
fn check_attachment_consistency(
    conn: &Connection,
    app_data_dir: &Path,
    messages: &mut Vec<String>,
) -> AppResult<()> {
    let dir = app_data_dir.join(ATTACHMENT_DIR);
    if !dir.exists() {
        return Ok(()); // 从未上传过附件：无可体检内容
    }

    let referenced: Vec<String> = {
        let mut stmt = conn.prepare("SELECT file_path FROM business_attachments")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    let mut on_disk: Vec<String> = Vec::new();
    collect_files_light(&dir, &mut on_disk)?;

    let orphans = on_disk.iter().filter(|p| !referenced.contains(*p)).count();
    let missing = referenced
        .iter()
        .filter(|p| !Path::new(p).is_file())
        .count();

    if orphans > 0 {
        messages.push(format!(
            "附件目录发现 {orphans} 个孤儿文件（数据库无引用，含加密残留 .enc.tmp，可手动清理）"
        ));
    }
    if missing > 0 {
        messages.push(format!(
            "{missing} 个附件记录对应的文件缺失（可能未随备份恢复或被手动删除）"
        ));
    }
    if orphans == 0 && missing == 0 {
        messages.push("附件目录一致性检查通过".to_string());
    }
    Ok(())
}

/// 轻量递归收集目录下全部文件绝对路径（不读文件内容，区别于打包用 collect_files）。
fn collect_files_light(dir: &Path, out: &mut Vec<String>) -> AppResult<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files_light(&path, out)?;
        } else {
            out.push(path.to_string_lossy().to_string());
        }
    }
    Ok(())
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

    // 业务附件目录（第七阶段）：与发票目录同规则打包
    let attachment_dir = app_data_dir.join(ATTACHMENT_DIR);
    let attachment_backup_dir = backup_dir.join(ATTACHMENT_DIR);
    if attachment_dir.exists() {
        copy_dir_recursive(&attachment_dir, &attachment_backup_dir)?;
    } else {
        fs::create_dir_all(&attachment_backup_dir)?;
    }

    let database_size = file_size(&database_backup_path);
    let invoice_dir_size = dir_size(&invoice_backup_dir)?;
    let attachment_dir_size = dir_size(&attachment_backup_dir)?;
    let manifest = BackupManifest {
        app: "salary-desktop".to_string(),
        version: 1,
        created_at: created_at.clone(),
        database_file: DATABASE_FILE.to_string(),
        invoice_dir: INVOICE_DIR.to_string(),
        attachment_dir: Some(ATTACHMENT_DIR.to_string()),
        database_size,
        invoice_dir_size,
        attachment_dir_size: Some(attachment_dir_size),
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
        // 第七阶段资金表（spec 8：数据安全状态增加资金表统计；旧库缺表时按 0 计）
        ("fund_accounts", "资金账户"),
        ("fund_documents", "资金单据"),
        ("business_attachments", "业务附件"),
        ("bank_reconciliation_allocations", "银行核销记录"),
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
    use crate::security::{self, SecurityState};

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("salary-desktop-{name}-{}", Uuid::new_v4().simple()))
    }

    /// 构造一份有 1 个员工 + 1 张发票的 app_dir,返回 (app_dir, conn)。
    fn seed_app(name: &str) -> (PathBuf, Connection) {
        let app_dir = temp_dir(name);
        fs::create_dir_all(&app_dir).unwrap();
        fs::create_dir_all(app_dir.join(INVOICE_DIR)).unwrap();
        fs::write(
            app_dir.join(INVOICE_DIR).join("sample.txt"),
            "invoice content",
        )
        .unwrap();
        let conn = db::init_db(&app_dir.to_string_lossy()).unwrap();
        conn.execute(
            "INSERT INTO employees (employee_no, name, status, created_at, updated_at) \
             VALUES ('E001', '张三', 'active', 'now', 'now')",
            [],
        )
        .unwrap();
        (app_dir, conn)
    }

    fn init_security(conn: &Connection) -> SecurityState {
        let state = SecurityState::new();
        security::setup(
            conn,
            &state,
            "Abcd1234",
            "RC-AAAA-BBBB-CCCC-DDDD-EEEE-FFFF-GGGG",
            "你小学班主任姓什么？",
            "王",
        )
        .expect("setup");
        state
    }

    #[test]
    fn test_backup_database_creates_manifest_and_consistent_db() {
        let (app_dir, conn) = seed_app("backup-app");
        let backup_parent = temp_dir("backup-out");
        fs::create_dir_all(&backup_parent).unwrap();
        let sec = SecurityState::new(); // 未初始化, 仅占位

        let result = backup_database(&conn, &app_dir, &backup_parent, false, &sec).unwrap();
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

    /// 加密备份的产物必须是单个 .enc 文件,且前 8 字节 = BACKUP_MAGIC。
    #[test]
    fn backup_with_encrypt_produces_enc_file_with_magic() {
        let (app_dir, conn) = seed_app("backup-enc-app");
        let backup_parent = temp_dir("backup-enc-out");
        fs::create_dir_all(&backup_parent).unwrap();
        let sec = init_security(&conn);

        let result = backup_database(&conn, &app_dir, &backup_parent, true, &sec).unwrap();

        // 加密模式下 backup_dir 字段指向最终 .enc 文件
        let enc_path = Path::new(&result.backup_dir);
        assert!(
            enc_path.is_file(),
            "加密备份应产出文件而非目录: {}",
            enc_path.display()
        );
        let head = fs::read(enc_path).unwrap();
        assert!(head.len() >= 8, "加密文件过短: {} bytes", head.len());
        assert_eq!(
            &head[..8],
            crate::security::BACKUP_MAGIC.as_slice(),
            "加密备份必须以 BACKUP_MAGIC 开头"
        );

        let _ = fs::remove_dir_all(app_dir);
        let _ = fs::remove_dir_all(backup_parent);
    }

    /// 旧版明文备份(目录形式)仍能被 restore_database 恢复。
    #[test]
    fn restore_handles_plain_backup() {
        let (src_app, src_conn) = seed_app("plain-src");
        let backup_parent = temp_dir("plain-backup");
        let dst_app = temp_dir("plain-dst");
        fs::create_dir_all(&backup_parent).unwrap();
        fs::create_dir_all(&dst_app).unwrap();
        let sec = SecurityState::new();

        // 1. 用 src 数据库生成明文备份目录
        let backup_result =
            backup_database(&src_conn, &src_app, &backup_parent, false, &sec).unwrap();
        let backup_dir = PathBuf::from(&backup_result.backup_dir);
        assert!(backup_dir.is_dir(), "明文备份应是目录");

        // 2. 在 dst_app 上初始化空 DB(便于验证恢复覆盖)
        let mut dst_conn = db::init_db(&dst_app.to_string_lossy()).unwrap();

        // 3. 恢复
        let r = restore_database(&mut dst_conn, &dst_app, &backup_dir, &sec).unwrap();
        assert!(r.success);

        // 4. 验证 dst_app/salary.db 中有员工数据
        let dst_db = dst_app.join(DATABASE_FILE);
        let check_conn = Connection::open(&dst_db).unwrap();
        let count: i64 = check_conn
            .query_row("SELECT COUNT(*) FROM employees", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1, "明文备份恢复后必须有 1 条员工记录");

        let _ = fs::remove_dir_all(src_app);
        let _ = fs::remove_dir_all(backup_parent);
        let _ = fs::remove_dir_all(dst_app);
    }

    /// 加密备份 + DEK 已加载 → 恢复成功。
    #[test]
    fn restore_handles_encrypted_backup() {
        let (src_app, src_conn) = seed_app("enc-src");
        let backup_parent = temp_dir("enc-backup");
        let dst_app = temp_dir("enc-dst");
        fs::create_dir_all(&backup_parent).unwrap();
        fs::create_dir_all(&dst_app).unwrap();

        let sec = init_security(&src_conn);

        // 1. 加密备份
        let backup_result =
            backup_database(&src_conn, &src_app, &backup_parent, true, &sec).unwrap();
        let enc_file = PathBuf::from(&backup_result.backup_dir);
        assert!(enc_file.is_file(), "加密备份应产出 .enc 文件");

        // 2. dst_app 上初始化空 DB,然后恢复(同一份 SecurityState,DEK 仍在内存)
        let mut dst_conn = db::init_db(&dst_app.to_string_lossy()).unwrap();
        let r = restore_database(&mut dst_conn, &dst_app, &enc_file, &sec).unwrap();
        assert!(r.success);

        // 3. 验证 dst_app/salary.db 中有员工数据
        let dst_db = dst_app.join(DATABASE_FILE);
        let check_conn = Connection::open(&dst_db).unwrap();
        let count: i64 = check_conn
            .query_row("SELECT COUNT(*) FROM employees", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1, "加密备份恢复后必须有 1 条员工记录");

        let _ = fs::remove_dir_all(src_app);
        let _ = fs::remove_dir_all(backup_parent);
        let _ = fs::remove_dir_all(dst_app);
    }

    /// DEK 未加载时,加密备份恢复必须失败。
    #[test]
    fn restore_encrypted_without_dek_fails() {
        let (src_app, src_conn) = seed_app("enc-nodek-src");
        let backup_parent = temp_dir("enc-nodek-backup");
        let dst_app = temp_dir("enc-nodek-dst");
        fs::create_dir_all(&backup_parent).unwrap();
        fs::create_dir_all(&dst_app).unwrap();

        let sec_loaded = init_security(&src_conn);
        // 加密备份
        let backup_result =
            backup_database(&src_conn, &src_app, &backup_parent, true, &sec_loaded).unwrap();
        let enc_file = PathBuf::from(&backup_result.backup_dir);

        // 切换到一个全新的 SecurityState(DEK 未加载)
        let sec_locked = SecurityState::new();
        let mut dst_conn = db::init_db(&dst_app.to_string_lossy()).unwrap();
        let r = restore_database(&mut dst_conn, &dst_app, &enc_file, &sec_locked);
        assert!(r.is_err(), "DEK 未加载时加密备份恢复必须失败");

        let _ = fs::remove_dir_all(src_app);
        let _ = fs::remove_dir_all(backup_parent);
        let _ = fs::remove_dir_all(dst_app);
    }

    #[test]
    fn test_verify_database_reports_ok() {
        let app_dir = temp_dir("verify-app");
        fs::create_dir_all(&app_dir).unwrap();
        let conn = db::init_db(&app_dir.to_string_lossy()).unwrap();

        let result = verify_database(&conn, Some(&app_dir)).unwrap();
        assert!(result.ok);
        assert_eq!(result.integrity_check, "ok");
        assert!(result.messages.iter().any(|m| m.contains("employees")));

        let _ = fs::remove_dir_all(app_dir);
    }

    /// 附件体检：孤儿文件（磁盘有/DB 无引用）与缺失文件（DB 有记录/磁盘无文件）都要报告。
    #[test]
    fn test_verify_database_reports_attachment_orphans_and_missing() {
        let app_dir = temp_dir("verify-att-app");
        fs::create_dir_all(&app_dir).unwrap();
        let conn = db::init_db(&app_dir.to_string_lossy()).unwrap();

        // 磁盘上：一个被 DB 引用的文件 + 一个孤儿文件
        let att_dir = app_dir
            .join(ATTACHMENT_DIR)
            .join("fund_document")
            .join("2026-08");
        fs::create_dir_all(&att_dir).unwrap();
        let referenced = att_dir.join("20260905120000_kept.pdf");
        fs::write(&referenced, b"kept").unwrap();
        let orphan = att_dir.join("20260905120001_orphan.pdf");
        fs::write(&orphan, b"orphan").unwrap();
        fs::write(att_dir.join("leftover.enc.tmp"), b"leftover").unwrap();

        conn.execute(
            "INSERT INTO business_attachments
                (entity_type, entity_id, file_name, file_path, encrypted, file_size, belong_month, uploaded_by, created_at)
             VALUES ('fund_document', 1, 'kept.pdf', ?1, 1, 4, '2026-08', NULL, 'now')",
            params![referenced.to_string_lossy()],
        )
        .unwrap();

        let result = verify_database(&conn, Some(&app_dir)).unwrap();
        assert!(result.ok, "孤儿/缺失文件是 warning，不改变 ok 结论");
        let orphan_msg = result
            .messages
            .iter()
            .find(|m| m.contains("孤儿文件"))
            .expect("应报告孤儿文件");
        assert!(
            orphan_msg.contains('2'),
            "孤儿文件应为 2 个（orphan + .enc.tmp 残留）: {orphan_msg}"
        );
        assert!(
            !result.messages.iter().any(|m| m.contains("文件缺失")),
            "文件都在磁盘上时不应报告缺失: {:?}",
            result.messages
        );

        // 引用文件被删 → 报告缺失
        fs::remove_file(&referenced).unwrap();
        let result = verify_database(&conn, Some(&app_dir)).unwrap();
        assert!(
            result.messages.iter().any(|m| m.contains("文件缺失")),
            "应报告缺失文件: {:?}",
            result.messages
        );

        let _ = fs::remove_dir_all(app_dir);
    }

    /// Task 16（spec 8 安全联动）：数据安全状态须包含资金表计数、附件统计（含加密/孤儿/缺失）、
    /// 第七阶段迁移状态与待归集数量。
    #[test]
    fn test_status_reports_fund_tables_attachments_and_migration() {
        let app_dir = temp_dir("status-fund-app");
        fs::create_dir_all(&app_dir).unwrap();
        let conn = db::init_db(&app_dir.to_string_lossy()).unwrap();

        // 一个资金账户 + 一条已登记加密附件 + 一个磁盘孤儿文件
        conn.execute(
            "INSERT INTO fund_accounts
                (account_code, name, account_type, gl_account_code, opening_balance,
                 strict_reconciliation, created_at, updated_at)
             VALUES ('BANK-DS', '数据安全测试户', 'bank', '1002', 0, 1, 'now', 'now')",
            [],
        )
        .unwrap();
        let att_dir = app_dir
            .join(ATTACHMENT_DIR)
            .join("fund_document")
            .join("2026-08");
        fs::create_dir_all(&att_dir).unwrap();
        let referenced = att_dir.join("20260905130000_doc.pdf");
        fs::write(&referenced, b"cipher").unwrap();
        let orphan = att_dir.join("20260905130001_orphan.pdf");
        fs::write(&orphan, b"orphan").unwrap();
        conn.execute(
            "INSERT INTO business_attachments
                (entity_type, entity_id, file_name, file_path, encrypted, file_size, belong_month, uploaded_by, created_at)
             VALUES ('fund_document', 1, 'doc.pdf', ?1, 1, 6, '2026-08', NULL, 'now')",
            params![referenced.to_string_lossy()],
        )
        .unwrap();

        let status = get_status(&conn, &app_dir).unwrap();

        // 资金表计数进入 table_counts
        let fund_count = status
            .table_counts
            .iter()
            .find(|t| t.table_name == "fund_accounts")
            .expect("table_counts 应包含 fund_accounts");
        assert_eq!(fund_count.count, 1);
        assert!(status
            .table_counts
            .iter()
            .any(|t| t.table_name == "fund_documents"));

        // 附件统计：总数 / 加密 / 孤儿 / 缺失
        assert_eq!(status.attachment_count, 1);
        assert_eq!(status.attachment_encrypted_count, 1);
        assert_eq!(status.attachment_orphan_count, 1, "磁盘孤儿文件应统计");
        assert_eq!(status.attachment_missing_count, 0);

        // 迁移状态（init_db 自动迁移完成）
        assert_eq!(status.stage7_migration_status.as_deref(), Some("done"));
        assert_eq!(status.stage7_pending_count, Some(0));

        // 引用文件被删 → 缺失统计 +1
        fs::remove_file(&referenced).unwrap();
        let status = get_status(&conn, &app_dir).unwrap();
        assert_eq!(status.attachment_missing_count, 1);

        let _ = fs::remove_dir_all(app_dir);
    }

    /// 备份/恢复覆盖 attachments 目录（spec 4.6）：明文备份含附件文件，
    /// 恢复后附件文件原样还原；DB 记录中的绝对路径随 dst 目录一致性由上层迁移保证，
    /// 本测试验证文件层备份/恢复闭环。
    #[test]
    fn test_backup_and_restore_cover_attachments_dir() {
        let (src_app, src_conn) = seed_app("att-src");
        let backup_parent = temp_dir("att-backup");
        let dst_app = temp_dir("att-dst");
        fs::create_dir_all(&backup_parent).unwrap();
        fs::create_dir_all(&dst_app).unwrap();
        let sec = SecurityState::new();

        // src 中放一个已登记的附件文件（内容加密与否不影响文件层备份）
        let att_dir = src_app
            .join(ATTACHMENT_DIR)
            .join("fund_document")
            .join("2026-08");
        fs::create_dir_all(&att_dir).unwrap();
        let att_file = att_dir.join("20260905120000_voucher.pdf");
        fs::write(&att_file, b"cipher-bytes-here").unwrap();
        src_conn
            .execute(
                "INSERT INTO business_attachments
                    (entity_type, entity_id, file_name, file_path, encrypted, file_size, belong_month, uploaded_by, created_at)
                 VALUES ('fund_document', 1, 'voucher.pdf', ?1, 1, 16, '2026-08', NULL, 'now')",
                params![att_file.to_string_lossy()],
            )
            .unwrap();

        // 明文备份：backup_dir 内必须含 attachments 树
        let backup_result =
            backup_database(&src_conn, &src_app, &backup_parent, false, &sec).unwrap();
        let backup_dir = PathBuf::from(&backup_result.backup_dir);
        let backed_up = backup_dir
            .join(ATTACHMENT_DIR)
            .join("fund_document")
            .join("2026-08")
            .join("20260905120000_voucher.pdf");
        assert!(
            backed_up.is_file(),
            "备份必须包含 attachments 目录: {}",
            backed_up.display()
        );
        assert_eq!(fs::read(&backed_up).unwrap(), b"cipher-bytes-here");

        // dst 恢复：附件文件还原
        let mut dst_conn = db::init_db(&dst_app.to_string_lossy()).unwrap();
        let r = restore_database(&mut dst_conn, &dst_app, &backup_dir, &sec).unwrap();
        assert!(r.success);
        let restored = dst_app
            .join(ATTACHMENT_DIR)
            .join("fund_document")
            .join("2026-08")
            .join("20260905120000_voucher.pdf");
        assert!(
            restored.is_file(),
            "恢复后附件文件必须还原: {}",
            restored.display()
        );
        assert_eq!(fs::read(&restored).unwrap(), b"cipher-bytes-here");

        // 恢复后的 DB 中附件记录一致
        let check_conn = Connection::open(dst_app.join(DATABASE_FILE)).unwrap();
        let count: i64 = check_conn
            .query_row("SELECT COUNT(*) FROM business_attachments", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1, "附件记录应随备份恢复");

        let _ = fs::remove_dir_all(src_app);
        let _ = fs::remove_dir_all(backup_parent);
        let _ = fs::remove_dir_all(dst_app);
    }
}
