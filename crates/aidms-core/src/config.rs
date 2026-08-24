//! LLM 配置（阶段 5 / 技术设计 §7）
//!
//! 持久化到 `llm_config` 表（id=1 单例）。⚠️ **敏感字段（api_key）绝不落 SQLite 明文**：
//! 表内仅存 `api_key_ref`（引用标记），真实密钥由 src-tauri 经 OS keyring 存储。
//! 本模块只负责非敏感的 provider/base_url/model/enabled 的读写。
use rusqlite::{params, Connection, Result};
use serde::{Deserialize, Serialize};

/// AI 能力提供方
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provider {
    /// 本地 Ollama（base_url 通常为 http://127.0.0.1:11434）
    Ollama,
    /// OpenAI 兼容云端 API
    OpenAiCompat,
}

impl Provider {
    pub fn as_str(&self) -> &'static str {
        match self {
            Provider::Ollama => "ollama",
            Provider::OpenAiCompat => "openai_compat",
        }
    }
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "ollama" => Some(Provider::Ollama),
            "openai_compat" => Some(Provider::OpenAiCompat),
            _ => None,
        }
    }
}

/// LLM 配置（非敏感字段）。`enabled=1` 后语义检索/问答才启用。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LlmConfig {
    pub provider: Option<Provider>,
    pub base_url: Option<String>,
    pub embed_model: Option<String>,
    pub gen_model: Option<String>,
    pub enabled: bool,
    /// 仅标记是否已存密钥引用，不含密钥本身
    pub has_api_key: bool,
}

/// 读取当前配置（无记录返回 None）
pub fn get_config(conn: &Connection) -> Result<Option<LlmConfig>> {
    let mut stmt = conn.prepare(
        "SELECT provider, base_url, embed_model, gen_model, enabled, api_key_ref
         FROM llm_config WHERE id = 1",
    )?;
    let row = stmt.query_row([], |r| {
        let provider_s: Option<String> = r.get(0)?;
        Ok((
            provider_s,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, Option<String>>(5)?,
        ))
    });
    match row {
        Ok((ps, base, emb, gen, enabled, keyref)) => Ok(Some(LlmConfig {
            provider: ps.as_deref().and_then(Provider::from_str),
            base_url: base,
            embed_model: emb,
            gen_model: gen,
            enabled: enabled != 0,
            has_api_key: keyref.is_some(),
        })),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

/// 写入/更新配置（id=1 单例 upsert）。`has_api_key` 由调用方据 keyring 是否存有密钥决定。
pub fn upsert_config(conn: &Connection, cfg: &LlmConfig, has_api_key: bool) -> Result<()> {
    // 先确保存在一行
    conn.execute(
        "INSERT OR IGNORE INTO llm_config (id, provider, base_url, embed_model, gen_model, enabled, api_key_ref)
         VALUES (1, NULL, NULL, NULL, NULL, 0, NULL)",
        [],
    )?;
    conn.execute(
        "UPDATE llm_config SET
            provider = ?1, base_url = ?2, embed_model = ?3, gen_model = ?4,
            enabled = ?5, api_key_ref = ?6
         WHERE id = 1",
        params![
            cfg.provider.map(|p| p.as_str()),
            cfg.base_url,
            cfg.embed_model,
            cfg.gen_model,
            cfg.enabled as i64,
            if has_api_key { Some("keyring:llm_api_key") } else { None },
        ],
    )?;
    Ok(())
}

/// 仅切换启用状态
pub fn set_enabled(conn: &Connection, enabled: bool) -> Result<()> {
    conn.execute("UPDATE llm_config SET enabled = ?1 WHERE id = 1", params![enabled as i64])?;
    Ok(())
}
