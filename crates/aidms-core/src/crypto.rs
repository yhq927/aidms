//! keyring 不可用时的本地密钥加密兜底（纯 Rust，可 `cargo test`）。
//!
//! 设计目标：当运行环境无 secret-service（如多数 Linux 桌面未启用）导致
//! `keyring` crate 失败时，仍能以「设备绑定口令派生密钥 + AES-256-GCM」方式
//! 把 API Key 等敏感串加密落盘，避免明文存储。
//!
//! 输出格式（base64）：`salt[16] || nonce[12] || ciphertext`
//! 口令策略：调用方传入一个「设备绑定」口令（如机器 ID + 用户固定串），
//! 由 [`derive_key`] 经 Argon2id 派生 32 字节密钥，再用 AES-256-GCM 加密。

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use rand::RngCore;

const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;

/// 用设备绑定口令加密明文 secret，返回 base64 字符串。
pub fn encrypt_secret(plaintext: &str, passphrase: &str) -> Result<String, String> {
    let mut salt = [0u8; SALT_LEN];
    rand::thread_rng().fill_bytes(&mut salt);
    let mut nonce = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce);

    let key = derive_key(passphrase, &salt)?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext.as_bytes())
        .map_err(|e| format!("加密失败: {e}"))?;

    let mut buf = Vec::with_capacity(SALT_LEN + NONCE_LEN + ct.len());
    buf.extend_from_slice(&salt);
    buf.extend_from_slice(&nonce);
    buf.extend_from_slice(&ct);
    Ok(B64.encode(buf))
}

/// 解密 [`encrypt_secret`] 产出物；口令错误会返回 Err。
pub fn decrypt_secret(b64: &str, passphrase: &str) -> Result<String, String> {
    let raw = B64
        .decode(b64)
        .map_err(|e| format!("base64 解码失败: {e}"))?;
    if raw.len() < SALT_LEN + NONCE_LEN {
        return Err("密文长度不足".into());
    }
    let (salt, rest) = raw.split_at(SALT_LEN);
    let (nonce, ct) = rest.split_at(NONCE_LEN);

    let key = derive_key(passphrase, salt)?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    let pt = cipher
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|e| format!("解密失败（口令错误或数据被篡改）: {e}"))?;
    String::from_utf8(pt).map_err(|e| format!("明文非 UTF-8: {e}"))
}

/// Argon2id 派生 32 字节密钥。
fn derive_key(passphrase: &str, salt: &[u8]) -> Result<[u8; 32], String> {
    let mut key = [0u8; 32];
    argon2::Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| format!("密钥派生失败: {e}"))?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_encrypt_decrypt() {
        let pw = "device-binding-passphrase";
        let secret = "sk-xxxx-敏感-API-Key-内容";
        let enc = encrypt_secret(secret, pw).unwrap();
        // 密文不应泄露明文
        assert!(!enc.contains(secret));
        let dec = decrypt_secret(&enc, pw).unwrap();
        assert_eq!(dec, secret);
    }

    #[test]
    fn wrong_passphrase_fails() {
        let enc = encrypt_secret("topsecret", "right").unwrap();
        assert!(decrypt_secret(&enc, "wrong").is_err());
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let enc = encrypt_secret("topsecret", "pw").unwrap();
        let mut raw = B64.decode(&enc).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 0xFF; // 篡改密文末字节
        let tampered = B64.encode(raw);
        assert!(decrypt_secret(&tampered, "pw").is_err());
    }
}
