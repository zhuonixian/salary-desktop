use crate::errors::{AppError, AppResult};
use aes_gcm::{aead::{Aead, KeyInit}, Aes256Gcm, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use chrono::{TimeDelta, Utc};
use rand::{RngCore, rngs::OsRng};
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use zeroize::Zeroizing;

pub const BACKUP_MAGIC: &[u8; 8] = b"SLRYSFE1";

pub fn gen_salt() -> [u8; 16] {
    let mut s = [0u8; 16];
    OsRng.fill_bytes(&mut s);
    s
}

pub fn gen_dek() -> [u8; 32] {
    let mut k = [0u8; 32];
    OsRng.fill_bytes(&mut k);
    k
}

pub fn derive_kek(secret: &str, salt: &[u8]) -> AppResult<[u8; 32]> {
    let params = Params::new(64 * 1024, 3, 1, Some(32))
        .map_err(|e| crate::errors::AppError::General(e.to_string()))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut out = [0u8; 32];
    argon
        .hash_password_into(secret.as_bytes(), salt, &mut out)
        .map_err(|e| crate::errors::AppError::General(e.to_string()))?;
    Ok(out)
}

pub fn wrap_dek(dek: &[u8; 32], kek: &[u8; 32]) -> AppResult<(Vec<u8>, [u8; 12])> {
    encrypt_bytes(dek, kek)
}

pub fn unwrap_dek(wrapped: &[u8], kek: &[u8; 32], nonce: &[u8; 12]) -> Option<[u8; 32]> {
    decrypt_bytes(wrapped, nonce, kek).ok().and_then(|v| {
        if v.len() == 32 {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&v);
            Some(arr)
        } else {
            None
        }
    })
}

pub fn encrypt_bytes(plain: &[u8], dek: &[u8; 32]) -> AppResult<(Vec<u8>, [u8; 12])> {
    let cipher = Aes256Gcm::new_from_slice(dek)
        .map_err(|e| crate::errors::AppError::General(e.to_string()))?;
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(nonce, plain)
        .map_err(|e| crate::errors::AppError::General(e.to_string()))?;
    Ok((ct, nonce_bytes))
}

pub fn decrypt_bytes(cipher: &[u8], nonce: &[u8; 12], dek: &[u8; 32]) -> AppResult<Vec<u8>> {
    let cipher_obj = Aes256Gcm::new_from_slice(dek)
        .map_err(|e| crate::errors::AppError::General(e.to_string()))?;
    let nonce = Nonce::from_slice(nonce);
    cipher_obj
        .decrypt(nonce, cipher)
        .map_err(|_| crate::errors::AppError::InvalidParam("解密失败".into()))
}

pub fn encrypt_file(src: &Path, dst: &Path, dek: &[u8; 32]) -> AppResult<()> {
    let plain = std::fs::read(src)?;
    let (ct, nonce) = encrypt_bytes(&plain, dek)?;
    let mut buf = Vec::with_capacity(12 + ct.len());
    buf.extend_from_slice(&nonce);
    buf.extend_from_slice(&ct);
    std::fs::write(dst, buf)?;
    Ok(())
}

pub fn decrypt_file(src: &Path, dst: &Path, dek: &[u8; 32]) -> AppResult<()> {
    let data = std::fs::read(src)?;
    if data.len() < 12 {
        return Err(crate::errors::AppError::InvalidParam("加密文件损坏".into()));
    }
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&data[..12]);
    let plain = decrypt_bytes(&data[12..], &nonce, dek)?;
    std::fs::write(dst, plain)?;
    Ok(())
}

pub fn validate_password_strength(p: &str) -> AppResult<()> {
    if p.len() < 8 {
        return Err(crate::errors::AppError::InvalidParam(
            "密码至少 8 位且同时包含字母和数字".into(),
        ));
    }
    let has_letter = p.chars().any(|c| c.is_ascii_alphabetic());
    let has_digit = p.chars().any(|c| c.is_ascii_digit());
    if !has_letter || !has_digit {
        return Err(crate::errors::AppError::InvalidParam(
            "密码至少 8 位且同时包含字母和数字".into(),
        ));
    }
    Ok(())
}

// 保留 Zeroizing 引用（后续 Task 4 用到）
#[allow(dead_code)]
pub type ZeroizedKey = Zeroizing<[u8; 32]>;

// ===== Task 4: SecurityState 状态机 + setup/unlock/lock =====

/// 内存中的安全上下文。OnceLock 延迟创建 Mutex，避免 lib 加载阶段状态污染。
pub struct SecurityState {
    inner: OnceLock<Mutex<SecurityInner>>,
}

#[derive(Default)]
struct SecurityInner {
    dek: Option<ZeroizedKey>,
}

impl SecurityState {
    pub fn new() -> Self {
        Self {
            inner: OnceLock::new(),
        }
    }

    fn inner(&self) -> &Mutex<SecurityInner> {
        self.inner.get_or_init(|| Mutex::new(SecurityInner::default()))
    }

    pub fn is_dek_loaded(&self) -> bool {
        self.inner()
            .lock()
            .map(|g| g.dek.is_some())
            .unwrap_or(false)
    }

    pub fn dek(&self) -> Option<ZeroizedKey> {
        self.inner()
            .lock()
            .ok()
            .and_then(|g| g.dek.clone())
    }

    pub fn clear_dek(&self) {
        if let Ok(mut g) = self.inner().lock() {
            g.dek = None;
        }
    }

    fn set_dek(&self, dek: [u8; 32]) {
        if let Ok(mut g) = self.inner().lock() {
            g.dek = Some(Zeroizing::new(dek));
        }
    }
}

impl Default for SecurityState {
    fn default() -> Self {
        Self::new()
    }
}

/// 解锁命令返回给前端的结果。Task 6 会迁移到 models.rs。
#[derive(serde::Serialize)]
pub struct UnlockResult {
    pub unlocked: bool,
    pub failed_attempts: u32,
    pub lock_until: Option<String>,
}

/// 连续解锁失败上限。达到后写入 lock_until，5 分钟内拒绝任何解锁尝试。
const MAX_ATTEMPTS: u32 = 5;
const LOCK_SECS: i64 = 5 * 60;

/// 判断是否已执行过 setup（security_state 中存在唯一一行）。
pub fn is_initialized(conn: &Connection) -> bool {
    conn.query_row("SELECT COUNT(*) FROM security_state", [], |r| r.get::<_, i64>(0))
        .map(|c| c > 0)
        .unwrap_or(false)
}

/// 首次启用安全模块：派生 DEK + 三个 KEK（密码 / 找回码 / 答案），
/// 把同一份 DEK 用三个 KEK 各 wrap 一次写入 security_state。
/// 成功后 DEK 直接进内存,避免强制用户立刻 unlock。
pub fn setup(
    conn: &Connection,
    state: &SecurityState,
    password: &str,
    recovery_code: &str,
    question: &str,
    answer: &str,
) -> AppResult<()> {
    validate_password_strength(password)?;
    if is_initialized(conn) {
        return Err(AppError::InvalidParam("安全配置已初始化".into()));
    }

    let dek = gen_dek();
    let now = Utc::now().to_rfc3339();

    // 密码 KEK + wrapped DEK
    let pw_salt = gen_salt();
    let pw_kek = derive_kek(password, &pw_salt)?;
    let (pw_wrapped, pw_nonce) = wrap_dek(&dek, &pw_kek)?;

    // 找回码 KEK + wrapped DEK
    let rc_salt = gen_salt();
    let rc_kek = derive_kek(recovery_code, &rc_salt)?;
    let (rc_wrapped, rc_nonce) = wrap_dek(&dek, &rc_kek)?;

    // 安全问题答案 KEK + wrapped DEK
    let q_salt = gen_salt();
    let q_kek = derive_kek(answer, &q_salt)?;
    let (q_wrapped, q_nonce) = wrap_dek(&dek, &q_kek)?;

    // password_hash / security_answer_hash 列在 schema 中是 NOT NULL,
    // 实际密码校验通过 unwrap_dek 是否成功判断,因此这两列仅作"存在性记录":
    // 分别存密码 KEK 与答案 KEK 的 hex,KEK 不同则 hex 不同,可作完整性旁证。
    let pw_hash = hex::encode(pw_kek);
    let ans_hash = hex::encode(q_kek);

    conn.execute(
        "INSERT INTO security_state (
            id, password_hash, password_kek_salt,
            wrapped_dek_by_password, wrapped_dek_by_password_nonce,
            recovery_kek_salt, wrapped_dek_by_recovery, wrapped_dek_by_recovery_nonce,
            security_question, question_kek_salt,
            wrapped_dek_by_question, wrapped_dek_by_question_nonce,
            security_answer_hash, idle_timeout_seconds, idle_lock_enabled,
            sensitive_reveal_seconds, failed_attempts, lock_until,
            created_at, updated_at)
         VALUES (1, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 300, 1, 300, 0, NULL, ?, ?)",
        params![
            pw_hash,
            hex::encode(pw_salt),
            hex::encode(&pw_wrapped),
            hex::encode(pw_nonce),
            hex::encode(rc_salt),
            hex::encode(&rc_wrapped),
            hex::encode(rc_nonce),
            question,
            hex::encode(q_salt),
            hex::encode(&q_wrapped),
            hex::encode(q_nonce),
            ans_hash,
            now,
            now,
        ],
    )?;

    state.set_dek(dek);
    Ok(())
}

/// 用密码尝试解锁。成功 → DEK 载入内存;失败 → 失败计数 +1,达到上限写入 lock_until。
/// lock_until 未到期时直接拒绝(连正确密码也不试)。
pub fn unlock(conn: &Connection, state: &SecurityState, password: &str) -> AppResult<UnlockResult> {
    let row = conn.query_row(
        "SELECT password_kek_salt, wrapped_dek_by_password, wrapped_dek_by_password_nonce,
                failed_attempts, lock_until
         FROM security_state WHERE id = 1",
        [],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, u32>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        },
    );
    let (pw_salt_hex, wrapped_hex, nonce_hex, mut attempts, lock_until) = match row {
        Ok(v) => v,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return Err(AppError::NotFound("安全配置未初始化".into()));
        }
        Err(e) => return Err(e.into()),
    };

    // lock_until 未到期 → 直接拒绝,并清空内存中的 DEK 防止残留可用
    if let Some(until) = &lock_until {
        if let Ok(t) = chrono::DateTime::parse_from_rfc3339(until) {
            if Utc::now() < t.with_timezone(&Utc) {
                state.clear_dek();
                return Ok(UnlockResult {
                    unlocked: false,
                    failed_attempts: attempts,
                    lock_until: Some(until.clone()),
                });
            }
        }
    }

    let salt = hex::decode(&pw_salt_hex).map_err(|e| AppError::General(e.to_string()))?;
    let kek = derive_kek(password, &salt)?;
    let wrapped = hex::decode(&wrapped_hex).map_err(|e| AppError::General(e.to_string()))?;
    let nonce_bytes = hex::decode(&nonce_hex).map_err(|e| AppError::General(e.to_string()))?;
    if nonce_bytes.len() != 12 {
        state.clear_dek();
        return Err(AppError::General("密码 nonce 损坏".into()));
    }
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&nonce_bytes);

    if let Some(dek) = unwrap_dek(&wrapped, &kek, &nonce) {
        // GCM tag 校验通过 ≡ 密码正确
        state.set_dek(dek);
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE security_state
             SET failed_attempts = 0, lock_until = NULL, updated_at = ?
             WHERE id = 1",
            params![now],
        )?;
        return Ok(UnlockResult {
            unlocked: true,
            failed_attempts: 0,
            lock_until: None,
        });
    }

    // 失败:清空内存中的 DEK,再计数 +1,达上限写 lock_until
    state.clear_dek();
    attempts += 1;
    let now = Utc::now();
    let lock_until_str = if attempts >= MAX_ATTEMPTS {
        Some((now + TimeDelta::seconds(LOCK_SECS)).to_rfc3339())
    } else {
        None
    };
    conn.execute(
        "UPDATE security_state
         SET failed_attempts = ?, lock_until = ?, updated_at = ?
         WHERE id = 1",
        params![attempts, lock_until_str, now.to_rfc3339()],
    )?;
    Ok(UnlockResult {
        unlocked: false,
        failed_attempts: attempts,
        lock_until: lock_until_str,
    })
}

/// 锁屏:清空内存中的 DEK,所有加密字段保持在 DB 中。
pub fn lock(state: &SecurityState) {
    state.clear_dek();
}

#[cfg(test)]
mod state_tests {
    use super::*;
    use crate::db::tests::setup_db;
    use rusqlite::Connection;

    fn fresh() -> (Connection, SecurityState) {
        (setup_db(), SecurityState::new())
    }

    #[test]
    fn setup_then_unlock_round_trip() {
        let (conn, state) = fresh();
        assert!(!is_initialized(&conn));
        setup(
            &conn,
            &state,
            "Abcd1234",
            "RC-AAAA-BBBB-CCCC-DDDD-EEEE-FFFF-GGGG",
            "你小学班主任姓什么？",
            "王",
        )
        .unwrap();
        assert!(is_initialized(&conn));
        let r = unlock(&conn, &state, "Abcd1234").unwrap();
        assert!(r.unlocked);
        assert!(state.dek().is_some());
    }

    #[test]
    fn unlock_wrong_password_increments_failures() {
        let (conn, state) = fresh();
        setup(&conn, &state, "Abcd1234", "RC-AAAA", "Q", "A").unwrap();
        let r = unlock(&conn, &state, "wrong").unwrap();
        assert!(!r.unlocked);
        assert_eq!(r.failed_attempts, 1);
        assert!(state.dek().is_none());
    }

    #[test]
    fn unlock_locks_after_5_failures() {
        let (conn, state) = fresh();
        setup(&conn, &state, "Abcd1234", "RC-AAAA", "Q", "A").unwrap();
        for _ in 0..5 {
            let _ = unlock(&conn, &state, "wrong").unwrap();
        }
        // 第 6 次:即便输入正确密码也必须被拒绝,且 lock_until 已设置
        let r = unlock(&conn, &state, "Abcd1234").unwrap();
        assert!(!r.unlocked);
        assert!(r.lock_until.is_some());
    }

    #[test]
    fn lock_clears_dek() {
        let (conn, state) = fresh();
        setup(&conn, &state, "Abcd1234", "RC", "Q", "A").unwrap();
        unlock(&conn, &state, "Abcd1234").unwrap();
        assert!(state.dek().is_some());
        lock(&state);
        assert!(state.dek().is_none());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_kek_deterministic() {
        let salt = gen_salt();
        let k1 = derive_kek("password", &salt).unwrap();
        let k2 = derive_kek("password", &salt).unwrap();
        assert_eq!(k1, k2);
    }

    #[test]
    fn derive_kek_different_secret_diverges() {
        let salt = gen_salt();
        let k1 = derive_kek("password", &salt).unwrap();
        let k2 = derive_kek("password2", &salt).unwrap();
        assert_ne!(k1, k2);
    }

    #[test]
    fn wrap_unwrap_round_trip() {
        let dek = gen_dek();
        let kek = derive_kek("pw", &gen_salt()).unwrap();
        let (wrapped, nonce) = wrap_dek(&dek, &kek).unwrap();
        let unwrapped = unwrap_dek(&wrapped, &kek, &nonce).expect("must unwrap");
        assert_eq!(unwrapped, dek);
    }

    #[test]
    fn unwrap_wrong_kek_returns_none() {
        let dek = gen_dek();
        let kek1 = derive_kek("pw1", &gen_salt()).unwrap();
        let kek2 = derive_kek("pw2", &gen_salt()).unwrap();
        let (wrapped, nonce) = wrap_dek(&dek, &kek1).unwrap();
        assert!(unwrap_dek(&wrapped, &kek2, &nonce).is_none());
    }

    #[test]
    fn encrypt_decrypt_bytes_round_trip() {
        let dek = gen_dek();
        let plain = b"hello salary desktop";
        let (cipher, nonce) = encrypt_bytes(plain, &dek).unwrap();
        let recovered = decrypt_bytes(&cipher, &nonce, &dek).unwrap();
        assert_eq!(recovered, plain);
    }

    #[test]
    fn decrypt_with_wrong_dek_fails() {
        let dek1 = gen_dek();
        let dek2 = gen_dek();
        let (cipher, nonce) = encrypt_bytes(b"secret", &dek1).unwrap();
        assert!(decrypt_bytes(&cipher, &nonce, &dek2).is_err());
    }

    #[test]
    fn encrypt_decrypt_file_round_trip() {
        let tmp = std::env::temp_dir().join(format!("sec_test_{}.bin", std::process::id()));
        let plain = b"binary content \x00\x01\xff";
        std::fs::write(&tmp, plain).unwrap();
        let enc = tmp.with_extension("enc");
        let dec = tmp.with_extension("dec");
        let dek = gen_dek();
        encrypt_file(&tmp, &enc, &dek).unwrap();
        decrypt_file(&enc, &dec, &dek).unwrap();
        assert_eq!(std::fs::read(&dec).unwrap(), plain);
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_file(&enc);
        let _ = std::fs::remove_file(&dec);
    }

    #[test]
    fn password_strength_rules() {
        assert!(validate_password_strength("short").is_err());
        assert!(validate_password_strength("abcdefgh").is_err()); // no digit
        assert!(validate_password_strength("12345678").is_err()); // no letter
        assert!(validate_password_strength("abcd1234").is_ok());
        assert!(validate_password_strength("Abcd1234").is_ok());
    }
}
