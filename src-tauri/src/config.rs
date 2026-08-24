//! LLM 配置命令（阶段 5 / 技术设计 §7、§10）
//!
//! 敏感字段（api_key）**优先存 OS keyring**，绝不落 SQLite 明文、绝不经前端暴露。
//! `llm_config` 表只存非敏感配置 + 密钥引用标记。
//!
//! keyring 兜底（P2-11）：Linux 无 secret-service（gnome-keyring/libsecret）等场景下
//! keyring 写入失败时，降级用 `aidms_core::crypto`（Argon2id 派生密钥 + AES-256-GCM）
//! 加密后写本地文件（home 下 `.aidms_keyring_fallback.b64`），避免明文落盘。
use std::path::{Path, PathBuf};

use keyring::Entry;
use serde::Deserialize;
use tauri::State;
use aidms_core::config as core_config;

use crate::DbState;

const KEYRING_SVC: &str = "aidms";
const KEYRING_API_KEY: &str = "llm_api_key";
/// keyring 不可用时的本地加密兜底文件（固定路径，便于 `load_api_key` 无状态读取）
const FALLBACK_FILE: &str = ".aidms_keyring_fallback.b64";

fn fallback_file() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".into());
    Path::new(&home).join(FALLBACK_FILE)
}

/// 设备绑定口令：主机名 + 应用常量（非高强度但避免明文；正式方案仍应接系统 keychain）。
///
/// ⚠️ **风险标注（P2-11）**：本口令为「主机名 + 固定盐」派生，攻击者若同时拿到
/// 加密兜底文件与主机名即可离线爆破还原明文密钥（熵仅约 20-30 bit）。这是 keyring
/// 不可用（Linux 无 secret-service）时的**降级**方案，仅用于避免明文落盘；高安全
/// 场景应安装 gnome-keyring/libsecret 走系统 keychain，并考虑在 README 告知该限制。
fn device_passphrase() -> String {
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown".into());
    format!("aidms-local-v1|{host}")
}

/// 本地加密兜底：crypto 加密后写 home 下固定文件
fn store_key_fallback(key: &str) -> Result<(), String> {
    let b64 = aidms_core::crypto::encrypt_secret(key, &device_passphrase())?;
    std::fs::write(fallback_file(), b64).map_err(|e| format!("本地加密兜底文件写入失败: {e}"))?;
    Ok(())
}

/// 读取本地加密兜底密钥（keyring 不可用时）
fn load_key_fallback() -> Option<String> {
    let b64 = std::fs::read_to_string(fallback_file()).ok()?;
    aidms_core::crypto::decrypt_secret(b64.trim(), &device_passphrase()).ok()
}

#[derive(Debug, Deserialize)]
pub struct SaveLlmConfigInput {
    /// 'ollama' | 'openai_compat'
    pub provider: String,
    pub base_url: String,
    pub embed_model: Option<String>,
    pub gen_model: Option<String>,
    /// 仅本次传入，存 keyring，不落 DB
    pub api_key: Option<String>,
    pub enabled: bool,
}

#[tauri::command]
pub fn save_llm_config(state: State<DbState>, input: SaveLlmConfigInput) -> Result<(), String> {
    let conn = state.0.lock().unwrap();
    let provider = core_config::Provider::from_str(&input.provider)
        .ok_or_else(|| format!("未知 provider: {}", input.provider))?;
    let cfg = core_config::LlmConfig {
        provider: Some(provider),
        base_url: Some(input.base_url),
        embed_model: input.embed_model,
        gen_model: input.gen_model,
        enabled: input.enabled,
        has_api_key: false,
    };
    // 密钥存 keyring；keyring 失败（Linux 无 secret-service）降级本地加密兜底（P2-11）
    let mut has_key = false;
    if let Some(key) = &input.api_key {
        if !key.is_empty() {
            match Entry::new(KEYRING_SVC, KEYRING_API_KEY)
                .and_then(|entry| entry.set_password(key))
            {
                Ok(()) => has_key = true,
                Err(e) => {
                    crate::log::error(
                        "[config]",
                        &format!("keyring 不可用，改用本地加密兜底: {e}"),
                    );
                    store_key_fallback(key).map_err(|e2| {
                        format!(
                            "密钥存储失败：系统 keychain 不可用且本地加密兜底失败（请安装 gnome-keyring 或 libsecret），{e2}"
                        )
                    })?;
                    has_key = true;
                }
            }
        }
    }
    core_config::upsert_config(&conn, &cfg, has_key).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_llm_config(state: State<DbState>) -> Result<core_config::LlmConfig, String> {
    let conn = state.0.lock().unwrap();
    core_config::get_config(&conn)
        .map_err(|e| e.to_string())
        .map(|o| o.unwrap_or_default())
}

#[tauri::command]
pub fn set_llm_enabled(state: State<DbState>, enabled: bool) -> Result<(), String> {
    let conn = state.0.lock().unwrap();
    core_config::set_enabled(&conn, enabled).map_err(|e| e.to_string())
}

/// 读取 keyring 中的 api_key（仅 Rust 侧调 LLM 用，绝不暴露前端）。
/// keyring 不可用时回退到本地加密兜底文件（P2-11）。
pub fn load_api_key() -> Option<String> {
    let entry = Entry::new(KEYRING_SVC, KEYRING_API_KEY).ok()?;
    match entry.get_password() {
        Ok(k) => Some(k),
        Err(_) => load_key_fallback(),
    }
}
