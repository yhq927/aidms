//! RAG 问答命令（阶段 5 / 技术设计 §7、§3.7、§12）
//!
//! 流程：复用 `aidms_core::rag` 检索上下文 + 构造隔离 prompt → 经**唯一出网客户端**
//! （SSRF 防护）调 LLM → 经 **Tauri Channel 流式**回前端。支持 `cancel_rag` 中途停止。
//!
//! 安全：① 仅 Rust 侧出网，前端不直连 LLM；② 密钥从 keyring 读取，不落日志；
//! ③ 检索数据以边界标记隔离（最佳努力缓解提示注入）；④ 请求体不带 `tools` 参数。
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use futures::StreamExt;
use serde::Deserialize;
use tauri::{ipc::Channel, State};
use url::Url;

use aidms_core::config as core_config;
use aidms_core::rag;

use crate::commands;
use crate::config::load_api_key;
use crate::net::SafeHttpClient;
use crate::DbState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskRequest {
    pub query: String,
    pub entity_ids: Option<Vec<i64>>,
    pub doc_types: Option<Vec<String>>,
    pub tag_ids: Option<Vec<i64>>,
    /// 已配置向量且启用语义时传 true（不可达自动降级全文）
    pub use_semantic: bool,
}

/// 引用清单条目（对应回答中的 [资料N]，N=index，点击可跳转原文）
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Citation {
    /// 1-based 引用编号，对应回答文本中的 [资料N]
    pub index: usize,
    pub doc_id: i64,
    pub title: String,
}

/// 构造 OpenAI 兼容请求体（stream=true，分角色 system/user，不带 tools）
fn build_chat_request(model: &str, system: &str, user: &str) -> String {
    serde_json::json!({
        "model": model,
        "stream": true,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user}
        ]
    })
    .to_string()
}

#[tauri::command]
pub async fn ask_rag(
    state: State<'_, DbState>,
    req: AskRequest,
    on_token: Channel<String>,
    on_cites: Channel<Vec<Citation>>,
) -> Result<(), String> {
    // 重置取消标志（克隆 Arc 以便后续 async 段使用）
    let cancel: Arc<AtomicBool> = state.2.clone();
    cancel.store(false, Ordering::SeqCst);

    // —— 段 1：锁内只读配置（llm_config 非敏感，不发起网络），随后立即释放锁 ——
    // R5-P1：修复「持 Mutex<Connection> 期间做同步嵌入（网络）」——嵌入移到锁外，
    // 锁只在实际 DB 访问时短暂持有；use_semantic=true 的问答不再阻塞其它 DB 命令。
    let cfg = {
        let conn = state.0.lock().unwrap();
        core_config::get_config(&conn)
            .map_err(|e| e.to_string())?
            .ok_or("未配置 AI，请先在配置页设置")?
    };
    if !cfg.enabled {
        return Err("AI 能力未启用，请在配置页开启".into());
    }
    let base_url = cfg.base_url.clone().ok_or("base_url 未配置")?;
    let model = cfg.gen_model.clone().ok_or("生成模型未配置")?;

    // —— 段 2：锁外嵌入（网络调用，不占 DB 锁；spawn_blocking 避免阻塞 async worker）——
    // use_semantic=true 时嵌入查询向量（先锁内读配置 → 释放 → 锁外嵌入）；
    // 不可达返回 None → 自动降级全文，安全。
    let query_vec = if req.use_semantic {
        let embed_cfg = {
            let conn = state.0.lock().unwrap();
            commands::read_embed_config(&conn)
        };
        match embed_cfg {
            Some(ecfg) => {
                let q = req.query.clone();
                tokio::task::spawn_blocking(move || commands::embed_query_with_config(ecfg, &q))
                    .await
                    .map_err(|e| format!("嵌入任务失败: {e}"))?
            }
            None => None,
        }
    } else {
        None
    };

    // —— 段 3：重新取锁：检索上下文 + 构造隔离 prompt + 引用清单（仅 DB 访问）——
    let (system, user_msg, cites) = {
        let conn = state.0.lock().unwrap();
        let chunks = rag::retrieve_context(
            &conn,
            &req.query,
            req.entity_ids.as_deref(),
            req.doc_types.as_deref(),
            req.tag_ids.as_deref(),
            8,
            query_vec,
            req.use_semantic,
        )
        .map_err(|e| e.to_string())?;
        let (sys, user) = rag::build_messages(&req.query, &chunks);
        // 引用清单：index ↔ doc_id（前端 [资料N] 点击跳转原文，P1-4）
        let cites: Vec<Citation> = chunks
            .iter()
            .map(|c| Citation {
                index: c.index,
                doc_id: c.doc_id,
                title: c.title.clone(),
            })
            .collect();
        (sys, user, cites)
    };

    // —— 异步段：调 LLM 流式 ——
    // 先把引用清单发给前端（回答未开始时即可渲染可点击出处）
    let _ = on_cites.send(cites);
    let api_key = load_api_key().unwrap_or_default();
    let host = Url::parse(&base_url)
        .map_err(|e| format!("base_url 解析失败: {e}"))?
        .host_str()
        .map(|h| h.to_string())
        .ok_or("base_url 缺少 host")?;
    let mut allowed = std::collections::HashSet::new();
    allowed.insert(host);
    let client = SafeHttpClient::new(allowed);

    let body = build_chat_request(&model, &system, &user_msg);
    // P2-6：chat URL 归一抽公共函数（net::chat_url_for，与 embed_url_for 同一文件维护）
    let url = crate::net::chat_url_for(&base_url);

    let resp = client.post_stream(&url, &body, &api_key).await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let txt = resp.text().await.unwrap_or_default();
        return Err(format!("LLM 返回错误 {status}: {}", txt.chars().take(300).collect::<String>()));
    }

    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    // R5-P2-1：流式读取采用「读间隔超时」（60s 无新数据才超时）而非总超时——
    // 长答案/慢 prefill 不再被 120s 总超时截断；服务端挂起 60s 无数据时报错退出（可取消）。
    let read_timeout = std::time::Duration::from_secs(60);
    loop {
        let next_chunk = tokio::time::timeout(read_timeout, stream.next())
            .await
            .map_err(|_| "流式读取超时（60 秒无新数据），请检查 LLM 服务后重试".to_string())?;
        let Some(chunk) = next_chunk else {
            break; // 流结束（服务端发送 [DONE] 或关闭连接）
        };
        if cancel.load(Ordering::SeqCst) {
            break; // 用户停止生成
        }
        let bytes = chunk.map_err(|e| format!("流式读取错误: {e}"))?;
        buf.push_str(&String::from_utf8_lossy(&bytes));
        // 逐行解析 SSE：data: {json}
        while let Some(pos) = buf.find('\n') {
            let mut line: String = buf.drain(..=pos).collect();
            line.truncate(line.trim_end_matches(['\n', '\r']).len());
            let line = line.trim();
            if let Some(data) = line.strip_prefix("data:") {
                let data = data.trim();
                if data == "[DONE]" {
                    return Ok(());
                }
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                    if let Some(tok) = v["choices"][0]["delta"]["content"].as_str() {
                        if !tok.is_empty() {
                            // Channel 关闭（前端卸载）即终止
                            if on_token.send(tok.to_string()).is_err() {
                                return Ok(());
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub fn cancel_rag(state: State<DbState>) {
    state.2.store(true, Ordering::SeqCst);
}
