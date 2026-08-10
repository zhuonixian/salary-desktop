use crate::errors::AppResult;
use aes_gcm::{aead::{Aead, KeyInit}, Aes256Gcm, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use rand::{RngCore, rngs::OsRng};
use std::path::Path;
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
