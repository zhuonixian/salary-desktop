//! 旧版（明文）资源迁移：把 Task 4 之前归档的明文发票图片加密、把
//! `app_settings` 中的百度 token 等敏感字段从明文迁移到加密存储。
//!
//! 迁移触发时机：旧版用户首次启动新版 → SetupSecurity 向导设置密码后，
//! 前端调用 `migrate_legacy_resources` 命令；本模块在后台异步遍历：
//! - `invoices WHERE image_encrypted = 0 AND image_path != ''` → 就地加密 + 更新标志位
//! - `app_settings.baidu_access_token`（旧明文）→ 加密写 enc + nonce → 删旧 key
//! 通过 `app.emit("legacy-migration-progress"|"legacy-migration-completed", ...)` 推送进度。
//!
//! 单条发票加密失败 → 记录日志后跳过（不中断整体迁移）；整体 panic 时
//! `legacy_migration_state.status` 保持 `'in_progress'`，下次启动前端可通过
//! `get_legacy_migration_status` 检测并提示续传。

use crate::errors::{AppError, AppResult};
use crate::security::SecurityState;
use chrono::Utc;
use rusqlite::{params, Connection};
use tauri::Emitter;

/// 迁移入口：加密所有明文发票图片 + OCR token，全程通过 `legacy_migration_state`
/// 表记录进度，并经 `app.emit` 推送事件。
///
/// 用泛型 `R: Runtime` 是为了让单元测试可以传入 `MockRuntime` 而非真实的 `Wry`。
pub fn run<R: tauri::Runtime>(
    conn: &Connection,
    sec: &SecurityState,
    app: &tauri::AppHandle<R>,
) -> AppResult<()> {
    let dek = sec
        .dek()
        .ok_or_else(|| AppError::InvalidParam("DEK 未加载".into()))?;
    let now = Utc::now().to_rfc3339();

    // 初始化（或重置）迁移记录：每次启动如需续传都会重置 status=in_progress
    conn.execute(
        "INSERT OR REPLACE INTO legacy_migration_state
            (id, status, total_invoices, processed_invoices, token_migrated, started_at, completed_at)
         VALUES (1, 'in_progress', 0, 0, 0, ?, NULL)",
        params![now],
    )?;

    // 统计待迁移的明文发票（image_path 非空）
    let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM invoices
         WHERE image_encrypted = 0 AND image_path IS NOT NULL AND image_path != ''",
        [],
        |r| r.get(0),
    )?;
    conn.execute(
        "UPDATE legacy_migration_state SET total_invoices = ?",
        params![total],
    )?;

    // 收集所有待迁移行（先读完再写，避免在迭代中修改同一表）
    let mut stmt = conn.prepare(
        "SELECT id, image_path FROM invoices
         WHERE image_encrypted = 0 AND image_path IS NOT NULL AND image_path != ''",
    )?;
    let rows: Vec<(i64, String)> = stmt
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
        .filter_map(Result::ok)
        .collect();
    drop(stmt);

    let mut processed: i64 = 0;
    for (id, path) in rows {
        let p = std::path::Path::new(&path);
        // 文件不存在则跳过（不中断；旧库可能 image_path 已被外部清理）
        if !p.exists() {
            log::warn!("跳过发票 {}：图片文件不存在 {}", id, path);
            continue;
        }
        let tmp = p.with_extension("legacy_migration.tmp");
        if let Err(e) = crate::security::encrypt_file(p, &tmp, &dek) {
            // 单条失败：清理临时文件、记录错误后继续
            log::error!("加密发票 {} 失败: {}", id, e);
            let _ = std::fs::remove_file(&tmp);
            continue;
        }
        // rename 原子覆盖原文件
        std::fs::rename(&tmp, p)?;
        conn.execute(
            "UPDATE invoices SET image_encrypted = 1 WHERE id = ?",
            params![id],
        )?;
        processed += 1;
        conn.execute(
            "UPDATE legacy_migration_state SET processed_invoices = ?",
            params![processed],
        )?;
        let _ = app.emit(
            "legacy-migration-progress",
            serde_json::json!({ "total": total, "processed": processed }),
        );
    }

    // 加密 OCR token（旧明文 → enc + nonce，删旧 key）
    let plain_token: Option<String> = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = 'baidu_access_token'",
            [],
            |r| r.get::<_, String>(0),
        )
        .ok();
    let mut token_migrated: i64 = 0;
    if let Some(token) = plain_token {
        let (cipher, nonce) = crate::security::encrypt_bytes(token.as_bytes(), &dek)?;
        use base64::Engine;
        let enc_b64 = base64::engine::general_purpose::STANDARD.encode(&cipher);
        let nonce_b64 = base64::engine::general_purpose::STANDARD.encode(nonce);
        conn.execute(
            "INSERT INTO app_settings (key, value) VALUES ('baidu_access_token_enc', ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![enc_b64],
        )?;
        conn.execute(
            "INSERT INTO app_settings (key, value) VALUES ('baidu_access_token_nonce', ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![nonce_b64],
        )?;
        conn.execute(
            "DELETE FROM app_settings WHERE key = 'baidu_access_token'",
            [],
        )?;
        token_migrated = 1;
    }

    let now2 = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE legacy_migration_state
         SET status = 'completed', token_migrated = ?, completed_at = ?",
        params![token_migrated, now2],
    )?;
    let _ = app.emit(
        "legacy-migration-completed",
        serde_json::json!({ "processed": processed, "total": total, "token_migrated": token_migrated != 0 }),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::setup_db;
    use crate::security::{self, SecurityState};

    /// setup_db 不创建 app_settings（OCR token 迁移测试需要这张表），这里补上。
    fn ensure_app_settings(conn: &Connection) {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS app_settings (key TEXT PRIMARY KEY, value TEXT);",
        )
        .expect("create app_settings");
    }

    /// 构造一个已 setup 完毕、DEK 已加载的内存 DB + SecurityState，并补上 app_settings。
    fn fresh_with_dek() -> (Connection, SecurityState) {
        let conn = setup_db();
        ensure_app_settings(&conn);
        let state = SecurityState::new();
        security::setup(
            &conn,
            &state,
            "Abcd1234",
            "RC-AAAA-BBBB-CCCC-DDDD-EEEE-FFFF-GGGG",
            "你小学班主任姓什么？",
            "王",
        )
        .expect("setup");
        (conn, state)
    }

    /// 写入一条明文发票（image_encrypted=0），返回 id 与图片绝对路径。
    fn insert_plain_invoice(conn: &Connection, plain_bytes: &[u8]) -> (i64, String) {
        let tmp = std::env::temp_dir().join(format!(
            "legacy_migration_plain_{}_{}.bin",
            std::process::id(),
            rand_u64()
        ));
        std::fs::write(&tmp, plain_bytes).unwrap();
        let path = tmp.to_string_lossy().to_string();
        conn.execute(
            "INSERT INTO invoices
                (invoice_code, invoice_number, belong_month, status, image_path,
                 image_encrypted, created_at, updated_at)
             VALUES (NULL, ?1, '2026-08', 'normal', ?2, 0, 'now', 'now')",
            params![format!("NUM-{}", rand_u64()), path],
        )
        .unwrap();
        (conn.last_insert_rowid(), path)
    }

    fn rand_u64() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64
    }

    fn read_status(conn: &Connection) -> (String, i64, i64, i64) {
        conn.query_row(
            "SELECT status, total_invoices, processed_invoices, token_migrated
             FROM legacy_migration_state WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap()
    }

    fn make_test_app() -> tauri::AppHandle<tauri::test::MockRuntime> {
        // MockRuntime 不需要真实窗口管理器，emit 在事件循环未跑时会安静失败被忽略。
        tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app")
            .handle()
            .clone()
    }

    #[test]
    fn migrate_encrypts_plain_invoices() {
        let (conn, state) = fresh_with_dek();
        let app = make_test_app();

        let plain1 = b"invoice image bytes 1 \x00\xff";
        let plain2 = b"invoice image bytes 2 \xaa\xbb";
        let (_id1, path1) = insert_plain_invoice(&conn, plain1);
        let (_id2, path2) = insert_plain_invoice(&conn, plain2);

        run(&conn, &state, &app).expect("migration ok");

        // 所有发票 image_encrypted=1
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM invoices WHERE image_encrypted = 0",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "不应还有明文发票");

        // 文件前 12 字节是随机 nonce（即不再等于原 plain 头部）
        for (path, plain) in [(&path1, plain1), (&path2, plain2)] {
            let on_disk = std::fs::read(path).unwrap();
            assert!(on_disk.len() > 12, "加密文件应包含 nonce + 密文");
            assert_ne!(
                &on_disk[..12],
                &plain[..12],
                "前 12 字节必须是随机 nonce，不等于明文头部"
            );
            // 能用 DEK 解密回原文
            let dek = state.dek().unwrap();
            let mut nonce = [0u8; 12];
            nonce.copy_from_slice(&on_disk[..12]);
            let recovered =
                crate::security::decrypt_bytes(&on_disk[12..], &nonce, &dek).expect("decrypt");
            assert_eq!(recovered.as_slice(), plain);
        }

        // 状态：completed, total=2, processed=2, token_migrated=0（无 OCR token）
        let (status, total, processed, token_migrated) = read_status(&conn);
        assert_eq!(status, "completed");
        assert_eq!(total, 2);
        assert_eq!(processed, 2);
        assert_eq!(token_migrated, 0);

        // 清理
        let _ = std::fs::remove_file(&path1);
        let _ = std::fs::remove_file(&path2);
    }

    #[test]
    fn migrate_skips_already_encrypted() {
        let (conn, state) = fresh_with_dek();
        let app = make_test_app();

        // 一张已加密发票（image_encrypted=1）
        let (id_enc, path_enc) = insert_plain_invoice(&conn, b"already-encrypted");
        conn.execute(
            "UPDATE invoices SET image_encrypted = 1 WHERE id = ?",
            params![id_enc],
        )
        .unwrap();
        // 一张明文发票
        let (_id_plain, path_plain) = insert_plain_invoice(&conn, b"plain-to-encrypt");

        run(&conn, &state, &app).expect("migration ok");

        // 已加密的文件内容不变（未被重新加密）
        let unchanged = std::fs::read(&path_enc).unwrap();
        assert_eq!(unchanged.as_slice(), b"already-encrypted");
        // 明文文件已被加密
        let now_enc = std::fs::read(&path_plain).unwrap();
        assert!(now_enc.len() > 12);

        // total 只统计 image_encrypted=0 → 1
        let (_, total, processed, _) = read_status(&conn);
        assert_eq!(total, 1);
        assert_eq!(processed, 1);

        let _ = std::fs::remove_file(&path_enc);
        let _ = std::fs::remove_file(&path_plain);
    }

    #[test]
    fn migrate_encrypts_plain_ocr_token() {
        let (conn, state) = fresh_with_dek();
        let app = make_test_app();

        // 写入旧明文 token
        conn.execute(
            "INSERT INTO app_settings (key, value) VALUES ('baidu_access_token', 'token-PLAIN-xyz')",
            [],
        )
        .unwrap();

        run(&conn, &state, &app).expect("migration ok");

        // 旧明文 key 已删除
        let plain: Option<String> = conn
            .query_row(
                "SELECT value FROM app_settings WHERE key = 'baidu_access_token'",
                [],
                |r| r.get::<_, String>(0),
            )
            .ok();
        assert!(plain.is_none(), "旧明文 token 必须被清除");

        // 新 enc + nonce key 已写入
        let enc: String = conn
            .query_row(
                "SELECT value FROM app_settings WHERE key = 'baidu_access_token_enc'",
                [],
                |r| r.get(0),
            )
            .expect("enc key must exist");
        let nonce_b64: String = conn
            .query_row(
                "SELECT value FROM app_settings WHERE key = 'baidu_access_token_nonce'",
                [],
                |r| r.get(0),
            )
            .expect("nonce key must exist");
        assert!(!enc.is_empty());
        assert!(!nonce_b64.is_empty());

        // 解密回原 token
        use base64::Engine;
        let cipher = base64::engine::general_purpose::STANDARD
            .decode(&enc)
            .unwrap();
        let nonce_bytes = base64::engine::general_purpose::STANDARD
            .decode(&nonce_b64)
            .unwrap();
        assert_eq!(nonce_bytes.len(), 12, "nonce 必须 12 字节");
        let mut nonce = [0u8; 12];
        nonce.copy_from_slice(&nonce_bytes);
        let dek = state.dek().unwrap();
        let plain_token = crate::security::decrypt_bytes(&cipher, &nonce, &dek).expect("decrypt");
        assert_eq!(plain_token, b"token-PLAIN-xyz");

        // token_migrated=1
        let (_, _, _, token_migrated) = read_status(&conn);
        assert_eq!(token_migrated, 1);
    }

    #[test]
    fn migrate_records_progress_through_state_table() {
        let (conn, state) = fresh_with_dek();
        let app = make_test_app();

        // 3 张明文发票
        let (_a, p1) = insert_plain_invoice(&conn, b"img-a");
        let (_b, p2) = insert_plain_invoice(&conn, b"img-b");
        let (_c, p3) = insert_plain_invoice(&conn, b"img-c");

        run(&conn, &state, &app).expect("migration ok");

        let (status, total, processed, token_migrated) = read_status(&conn);
        assert_eq!(status, "completed");
        assert_eq!(total, 3);
        assert_eq!(processed, 3);
        assert_eq!(token_migrated, 0);

        for p in [p1, p2, p3] {
            let _ = std::fs::remove_file(p);
        }
    }
}
