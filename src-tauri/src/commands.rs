use serde::Deserialize;
use std::collections::HashSet;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};
use aidms_core::entities::{self, DocumentFilter, NewDocument, DocumentRow, EntityRow, TagRow, DocumentWithEntities};
use aidms_core::fields::{self, FieldDef};
use aidms_core::export::{self, ExportFormat};
use aidms_core::ingest;
use aidms_core::parse::{self, Kind};
use aidms_core::search::{self, SearchMode, SearchRequest};

use crate::config::load_api_key;
use crate::net::{self, SafeHttpClient};
use crate::DbState;

/// 当前 ISO8601 时间（created_at 由后端统一生成）
fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[tauri::command]
pub fn greet(name: &str) -> String {
    format!("你好，{name}！AIDMS 已就绪")
}

/// 组装高级筛选条件（R15）：顶栏主体 + 多主体并集 + 负责人 + 日期范围 + 来源
fn doc_filter(
    entity_id: Option<i64>,
    doc_type: Option<String>,
    tag_id: Option<i64>,
    entity_ids: Option<Vec<i64>>,
    owner: Option<String>,
    date_from: Option<String>,
    date_to: Option<String>,
    source: Option<String>,
) -> DocumentFilter {
    DocumentFilter {
        entity_id,
        doc_type,
        tag_id,
        entity_ids: entity_ids.unwrap_or_default(),
        owner,
        date_from,
        date_to,
        source,
    }
}

#[tauri::command]
pub fn list_documents(
    state: State<DbState>,
    entity_id: Option<i64>,
    doc_type: Option<String>,
    tag_id: Option<i64>,
entity_ids: Option<Vec<i64>>,
owner: Option<String>,
date_from: Option<String>,
date_to: Option<String>,
source: Option<String>,
) -> Result<Vec<DocumentRow>, String> {
    let conn = state.0.lock().unwrap();
    entities::list_documents(
        &conn,
        &doc_filter(entity_id, doc_type, tag_id, entity_ids, owner, date_from, date_to, source),
    )
    .map_err(|e| e.to_string())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewDocumentInput {
    pub kind: String,
    pub title: String,
    pub doc_type: Option<String>,
    pub source: Option<String>,
    pub content_text: Option<String>,
    pub party: Option<String>,
    pub owner: Option<String>,
    pub date_field: Option<String>,
    pub note: Option<String>,
    pub fields: Option<String>,
    pub status: Option<String>,
    pub created_at: String,
}

/// ⚠️ 兼容保留命令：**勿用于业务条目/文件入库**（会绕过 FTS5/chunk/向量索引，产生不可搜索的
/// 孤立行）。业务条目请走 `submit_parsed`（kind=business），文件导入走 `import_files`。
/// 后端删除会影响旧前端/历史调用兼容，故保留；前端 `documentClient.createDocument` 已标记废弃。
#[tauri::command]
pub fn create_document(state: State<DbState>, doc: NewDocumentInput) -> Result<i64, String> {
    let conn = state.0.lock().unwrap();
    let new = NewDocument {
        kind: doc.kind,
        title: doc.title,
        doc_type: doc.doc_type,
        source: doc.source,
        content_text: doc.content_text,
        party: doc.party,
        owner: doc.owner,
        date_field: doc.date_field,
        note: doc.note,
        fields: doc.fields,
        status: doc.status,
        created_at: doc.created_at,
    };
    entities::create_document(&conn, &new).map_err(|e| e.to_string())
}

/// 删除文档（P2-1）：级联清理 FTS5/vec/chunk/归属/标签/关联/自定义字段值后删主行。
#[tauri::command]
pub fn delete_document(state: State<DbState>, id: i64) -> Result<(), String> {
    let conn = state.0.lock().unwrap();
    entities::delete_document(&conn, id).map_err(|e| e.to_string())
}

/// 关联文档到主体（多对多）
#[tauri::command]
pub fn link_entity(state: State<DbState>, document_id: i64, entity_id: i64) -> Result<(), String> {
    let conn = state.0.lock().unwrap();
    entities::link_entity(&conn, document_id, entity_id).map_err(|e| e.to_string())
}

/// 取消文档与主体的关联
#[tauri::command]
pub fn unlink_entity(state: State<DbState>, document_id: i64, entity_id: i64) -> Result<(), String> {
    let conn = state.0.lock().unwrap();
    entities::unlink_entity(&conn, document_id, entity_id).map_err(|e| e.to_string())
}

// ===================== 阶段 6 多主体（entity）+ 标签 =====================

#[tauri::command]
pub fn list_entities(state: State<DbState>) -> Result<Vec<EntityRow>, String> {
    let conn = state.0.lock().unwrap();
    entities::list_entities(&conn).map_err(|e| e.to_string())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewEntityInput {
    pub name: String,
    pub credit_code: Option<String>,
    pub note: Option<String>,
}

#[tauri::command]
pub fn create_entity(state: State<DbState>, input: NewEntityInput) -> Result<i64, String> {
    let conn = state.0.lock().unwrap();
    entities::create_entity(
        &conn,
        input.name.trim(),
        input.credit_code.as_deref().filter(|s| !s.is_empty()),
        input.note.as_deref().filter(|s| !s.is_empty()),
        &now_iso(),
    )
    .map_err(|e| e.to_string())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEntityInput {
    pub id: i64,
    pub name: String,
    pub credit_code: Option<String>,
    pub note: Option<String>,
}

#[tauri::command]
pub fn update_entity(state: State<DbState>, input: UpdateEntityInput) -> Result<(), String> {
    let conn = state.0.lock().unwrap();
    entities::update_entity(
        &conn,
        input.id,
        input.name.trim(),
        input.credit_code.as_deref().filter(|s| !s.is_empty()),
        input.note.as_deref().filter(|s| !s.is_empty()),
    )
    .map_err(|e| e.to_string())
}

/// 删除主体：仍有资料归属时拒绝（避免悬挂引用），返回中文错误提示由前端拦截展示。
#[tauri::command]
pub fn delete_entity(state: State<DbState>, id: i64) -> Result<(), String> {
    let conn = state.0.lock().unwrap();
    entities::delete_entity_guard(&conn, id)
}

#[tauri::command]
pub fn list_tags(state: State<DbState>) -> Result<Vec<TagRow>, String> {
    let conn = state.0.lock().unwrap();
    entities::list_tags(&conn).map_err(|e| e.to_string())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewTagInput {
    pub name: String,
}

/// 新建标签（P2-1：后端标签 CRUD 补齐；名称重复时返回 UNIQUE 错误由前端提示）
#[tauri::command]
pub fn create_tag(state: State<DbState>, input: NewTagInput) -> Result<i64, String> {
    let conn = state.0.lock().unwrap();
    let name = input.name.trim();
    if name.is_empty() {
        return Err("标签名不能为空".into());
    }
    entities::create_tag(&conn, name).map_err(|e| e.to_string())
}

/// 给文档打标签（P2-1：多对多，幂等）
#[tauri::command]
pub fn add_document_tag(
    state: State<DbState>,
    document_id: i64,
    tag_id: i64,
) -> Result<(), String> {
    let conn = state.0.lock().unwrap();
    entities::add_document_tag(&conn, document_id, tag_id).map_err(|e| e.to_string())
}

/// 取消文档标签（P2-1）
#[tauri::command]
pub fn remove_document_tag(
    state: State<DbState>,
    document_id: i64,
    tag_id: i64,
) -> Result<(), String> {
    let conn = state.0.lock().unwrap();
    entities::remove_document_tag(&conn, document_id, tag_id).map_err(|e| e.to_string())
}

/// 列出某文档的标签 id（P2-3：供前端打标 UI 展示当前已打标签）
#[tauri::command]
pub fn list_document_tags(state: State<DbState>, document_id: i64) -> Result<Vec<i64>, String> {
    let conn = state.0.lock().unwrap();
    entities::list_document_tags(&conn, document_id).map_err(|e| e.to_string())
}

/// 资料列表（含归属主体 id 列表，用于前端「未归类主体」标示）
#[tauri::command]
pub fn list_documents_with_entities(
    state: State<DbState>,
    entity_id: Option<i64>,
    doc_type: Option<String>,
    tag_id: Option<i64>,
entity_ids: Option<Vec<i64>>,
owner: Option<String>,
date_from: Option<String>,
date_to: Option<String>,
source: Option<String>,
) -> Result<Vec<DocumentWithEntities>, String> {
    let conn = state.0.lock().unwrap();
    entities::list_documents_with_entities(
        &conn,
        &doc_filter(entity_id, doc_type, tag_id, entity_ids, owner, date_from, date_to, source),
    )
    .map_err(|e| e.to_string())
}

/// 资料导出（R17）：按三维筛选 + 高级筛选导出 CSV / JSON 文本（含主体标注），前端负责落盘
#[tauri::command]
pub fn export_documents(
    state: State<DbState>,
    entity_id: Option<i64>,
    doc_type: Option<String>,
    tag_id: Option<i64>,
entity_ids: Option<Vec<i64>>,
owner: Option<String>,
date_from: Option<String>,
date_to: Option<String>,
source: Option<String>,
    format: String,
) -> Result<String, String> {
    let fmt = ExportFormat::parse(&format).map_err(|e| e.to_string())?;
    let conn = state.0.lock().unwrap();
    export::export_documents(
        &conn,
        &doc_filter(entity_id, doc_type, tag_id, entity_ids, owner, date_from, date_to, source),
        fmt,
    )
    .map_err(|e| e.to_string())
}

/// 取某业务类型的字段定义（预置 + 用户自定义），供业务表单渲染（R2/R12）
#[tauri::command]
pub fn get_field_defs(state: State<DbState>, biz_type: String) -> Result<Vec<FieldDef>, String> {
    let conn = state.0.lock().unwrap();
    fields::get_field_defs(&conn, &biz_type).map_err(|e| e.to_string())
}

/// 写入某文档的自定义字段值（upsert），写入后自动触发 FTS5 重建（R12）。
/// P2-5：已配置嵌入模型时，字段变更后同步重建该文档向量（字段值拼入 content，
/// 语义检索需保持一致；未配置嵌入则跳过向量重建，仅 FTS5）。
/// R5-P2-4：`fields::set_field_value` 内部事务已提交后，再执行向量重建；
/// 重建失败**不静默**——记录日志 + 写 app_meta 待补偿标记，由后台
/// `rebuild_missing_indexes` 下轮自动补建（字段值已保存，不因索引失败回滚业务）。
#[tauri::command]
pub fn set_field_value(
    state: State<DbState>,
    doc_id: i64,
    field_key: String,
    value: String,
) -> Result<(), String> {
    let conn = state.0.lock().unwrap();
    fields::set_field_value(&conn, doc_id, &field_key, &value)?;
    let embed = embed_closure(&conn);
    if embed.is_some() {
        // 已配置嵌入：重建该文档完整索引（FTS5 + chunk + 向量），幂等
        if let Err(e) = ingest::rebuild_document_index(&conn, doc_id, embed.as_deref()) {
            // P2-4：事务已提交，重建失败会使该文档进入缺索引/缺向量集合（reindex 先清旧），
            // 此处显式标记待补偿，确保下轮后台补偿能补（不静默丢失语义索引状态）。
            crate::log::error(
                "[field]",
                &format!("字段变更后重建索引失败 doc={doc_id}: {e}（已标记待补偿）"),
            );
            let _ = ingest::mark_document_reindex_pending(&conn, doc_id, &e.to_string());
        }
    }
    Ok(())
}

/// 用户新增自定义字段定义（is_preset=0）
#[tauri::command]
pub fn add_field_def(
    state: State<DbState>,
    biz_type: String,
    field_key: String,
    field_label: String,
    field_type: String,
) -> Result<i64, String> {
    let conn = state.0.lock().unwrap();
    fields::add_field_def(&conn, &biz_type, &field_key, &field_label, &field_type)
        .map_err(|e| e.to_string())
}

/// 删除用户自定义字段定义（R12 补删，P2-8）：仅允许删 is_preset=0 的自定义字段，
/// 级联删除该 key 的 field_value 并重建受影响文档 FTS5。预置字段拒绝删除（幂等返回 0）。
#[tauri::command]
pub fn remove_field_def(state: State<DbState>, id: i64) -> Result<usize, String> {
    let conn = state.0.lock().unwrap();
    fields::remove_field_def(&conn, id)
}

/// 阶段 4 融合检索请求（前端 → aidms_core::search）
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequestInput {
    pub query: String,
    /// "keyword" | "semantic" | "hybrid"（默认 hybrid）
    #[serde(default)]
    pub mode: String,
    /// 查询向量（已嵌入）。缺省时若模式需要语义，Rust 侧自动嵌入（需配置嵌入模型）。
    #[serde(default)]
    pub query_vec: Option<Vec<f32>>,
    /// 主体约束（多主体）：仅检索这些主体的资料
    #[serde(default)]
    pub entity_ids: Option<Vec<i64>>,
    /// 类型约束（结果内筛选）
    #[serde(default)]
    pub doc_types: Option<Vec<String>>,
    /// 标签约束
    #[serde(default)]
    pub tag_ids: Option<Vec<i64>>,
    #[serde(default)]
    pub limit: Option<usize>,
}

fn parse_mode(s: &str) -> SearchMode {
    match s {
        "keyword" => SearchMode::Keyword,
        "semantic" => SearchMode::Semantic,
        "hybrid" => SearchMode::Hybrid,
        _ => SearchMode::Hybrid,
    }
}

/// 读取嵌入配置（llm_config 表 id=1 单例，列结构见 0001_init.sql：
/// `id/provider/base_url/api_key_ref/embed_model/gen_model/enabled`）。
/// 返回 (provider, base_url, embed_model, api_key)。api_key 从 OS keyring 读取（config.rs `load_api_key`），
/// 绝不从 SQLite 明文读取（技术设计 §10 密钥不落库）。未配置/未启用返回 None（调用方降级全文）。
/// provider：'ollama' | 'openai_compat'（P0-5 云端模式分路：嵌入端点按 provider 区分）。
///
/// **锁语义（R5-P1）**：本函数仅做 DB 只读（llm_config 非敏感），可在持锁段内调用；
/// 返回的配置随后应**在释放 DB 锁后**交给 [`embed_query_with_config`] 发起网络嵌入，
/// 避免「持 Mutex<Connection> 期间做阻塞 HTTP」（修复 ask_rag / search_documents 的锁内嵌入回归）。
pub(crate) fn read_embed_config(conn: &rusqlite::Connection) -> Option<(String, String, String, String)> {
    let row: (Option<String>, Option<String>, Option<String>) = conn
        .prepare(
            "SELECT provider, base_url, embed_model FROM llm_config
             WHERE id = 1 AND enabled = 1 AND base_url IS NOT NULL AND embed_model IS NOT NULL",
        )
        .ok()?
        .query_row([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .ok()?;
    // provider 缺省按 openai_compat 处理（net::embed_url_for 默认分支）
    let provider = row.0.unwrap_or_else(|| "openai_compat".to_string());
    let base_url = row.1?;
    let model = row.2?;
    if base_url.trim().is_empty() || model.trim().is_empty() {
        return None;
    }
    Some((provider, base_url, model, load_api_key().unwrap_or_default()))
}

/// 构造嵌入闭包：读取 llm_config + keyring，返回可传给 `ingest::ingest` / `rebuild_missing_indexes`
/// 的嵌入回调（文本 → f32 向量）。未配置嵌入（或未启用）返回 None（跳过向量写入，仅 FTS5）。
/// 闭包内每次调用经 [`SafeHttpClient`]（SSRF 白名单 + 禁用自动重定向），密钥仅经 Bearer 发送。
/// P2-6：`SafeHttpClient` 在闭包外**只建一次**再 move 进闭包（避免每 chunk 重建 reqwest client）。
/// `pub(crate)`：供 watch.rs 监控入库复用（P1-2）。
pub(crate) fn embed_closure(
    conn: &rusqlite::Connection,
) -> Option<Box<dyn Fn(&str) -> Result<Vec<f32>, String>>> {
    let (provider, base_url, model, api_key) = read_embed_config(conn)?;
    // P1-3：模型变更时清除维度探测缓存（强制重新探测；相同模型则无操作）
    if ingest::sync_embed_dim_probe_model(conn, &model).is_err() {
        // 缓存同步失败不阻塞嵌入（降级：本轮不缓存，下次循环重新探测）
        crate::log::info("[embed]", "embed_dim_probe 缓存同步失败（忽略，继续）");
    }
    let host = url::Url::parse(&base_url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
        .ok_or_else(|| "嵌入 base_url 解析失败".to_string())
        .ok()?;
    let mut allowed = HashSet::new();
    allowed.insert(host);
    let client = SafeHttpClient::new(allowed);
    Some(Box::new(move |text: &str| {
        // P0-5：按 provider 分路（ollama→/api/embed；openai_compat→/v1/embeddings）
        net::embed_text(&client, &provider, &base_url, &model, &api_key, text)
    }))
}

/// 锁外嵌入查询文本（R5-P1 修复）：**网络调用，须在释放 DB 锁后执行**。
/// 调用方先在锁内 [`read_embed_config`] 读取配置（DB 只读），释放锁后再调本函数；
/// 复用 [`SafeHttpClient`]（SSRF 白名单 + 禁用自动重定向），密钥仅经 Bearer 发送，不写日志。
/// 失败返回 None（调用方降级为全文）。
pub(crate) fn embed_query_with_config(
    cfg: (String, String, String, String),
    query: &str,
) -> Option<Vec<f32>> {
    let (provider, base_url, model, api_key) = cfg;
    let host = url::Url::parse(&base_url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))?;
    let mut allowed = HashSet::new();
    allowed.insert(host);
    let client = SafeHttpClient::new(allowed);
    // P0-5：按 provider 分路（ollama→/api/embed；openai_compat→/embeddings）
    net::embed_text(&client, &provider, &base_url, &model, &api_key, query).ok()
}

/// 阶段 4 融合检索（FTS5 关键词 + vec0 向量 → RRF 融合）
///
/// - 语义/融合模式且前端未传 query_vec 时，自动调用嵌入模型获取查询向量；
///   若未配置或嵌入失败，自动降级为全文（不阻塞搜索）。
/// - R5-P1：嵌入为网络调用，**不持有 DB 锁**——锁内只读嵌入配置 → 释放锁 →
///   `spawn_blocking` 锁外嵌入（阻塞 HTTP 不占 async worker）→ 重新取锁检索。
/// - 返回结构化命中（含 `<mark>` 高亮片段），前端须经 DOMPurify 净化后注入。
#[tauri::command]
pub async fn search_documents(
    state: State<'_, DbState>,
    req: SearchRequestInput,
) -> Result<Vec<search::SearchHit>, String> {
    let mut mode = parse_mode(&req.mode);

    // 语义/融合需要向量：优先用前端传的，否则自动嵌入；都失败则降级全文
    let mut query_vec = req.query_vec.clone();
    if (mode == SearchMode::Semantic || mode == SearchMode::Hybrid) && query_vec.is_none() {
        // 段 1：锁内只读嵌入配置（非敏感，不发起网络），随后立即释放锁
        let embed_cfg = {
            let conn = state.0.lock().unwrap();
            read_embed_config(&conn)
        };
        // 段 2：锁外嵌入（网络调用；spawn_blocking 避免阻塞 async worker 线程）
        query_vec = match embed_cfg {
            Some(cfg) => {
                let q = req.query.clone();
                tokio::task::spawn_blocking(move || embed_query_with_config(cfg, &q))
                    .await
                    .unwrap_or(None)
            }
            None => None,
        };
        if query_vec.is_none() {
            crate::log::info("[search]", "嵌入不可用，降级为全文模式");
            mode = SearchMode::Keyword;
        }
    }

    let request = SearchRequest {
        query: req.query,
        query_vec,
        mode,
        entity_ids: req.entity_ids,
        doc_types: req.doc_types,
        tag_ids: req.tag_ids,
        limit: req.limit.unwrap_or(30),
    };
    // 段 3：重新取锁执行检索（仅 DB 访问）
    let conn = state.0.lock().unwrap();
    search::search(&conn, &request).map_err(|e| e.to_string())
}

// ===================== 阶段 3 入库流水线命令 =====================

/// 拖拽/选择导入时调用：把本次会话授权的来源路径（canonicalize 后，防 symlink 穿越）
/// 记入授权集合，供 `submit_parsed` 校验。返回成功授权数量。
#[tauri::command]
pub fn authorize_sources(state: State<DbState>, paths: Vec<String>) -> Result<usize, String> {
    let mut auth = state.1.lock().unwrap();
    let mut n = 0;
    for p in paths {
        match std::fs::canonicalize(&p) {
            Ok(c) => {
                auth.insert(c.to_string_lossy().into_owned());
                n += 1;
            }
            Err(e) => crate::log::info("[authorize]", &format!("跳过无法规范化的路径 {p}: {e}")),
        }
    }
    Ok(n)
}

/// 解析结果回传契约（对应开发计划阶段 3 `submit_parsed`）
#[derive(Debug, Deserialize)]
pub struct ParsedInputPayload {
    pub title: String,
    pub content_text: String,
    pub fields: Option<String>,
    pub source: Option<String>,
    pub kind: String, // 'file' | 'business'
    pub source_kind: String, // 'txt'|'pdf'|'docx'|'xlsx'|'image'|'business'|...
    pub entity_ids: Vec<i64>,
    pub doc_type: Option<String>,
    pub party: Option<String>,
    pub owner: Option<String>,
    pub date_field: Option<String>,
    pub note: Option<String>,
}

/// 把 Payload 映射为 `aidms_core::ingest::IngestInput`
fn to_ingest_input(p: ParsedInputPayload) -> ingest::IngestInput {
    let sk = if p.source_kind == "business" {
        Kind::Business
    } else {
        parse::kind_from_ext(&p.source_kind).unwrap_or(Kind::Txt)
    };
    ingest::IngestInput {
        kind: p.kind,
        source_kind: sk,
        title: p.title,
        content_text: p.content_text,
        fields: p.fields,
        source: p.source,
        doc_type: p.doc_type,
        party: p.party,
        owner: p.owner,
        date_field: p.date_field,
        note: p.note,
        entity_ids: p.entity_ids,
        created_at: now_iso(),
    }
}

/// 单一受限回传入口：前端（隔离 webview pdfjs / 主 webview mammoth/SheetJS）解析完成后提交。
///
/// 安全契约：① 长度上限（防超大伪造内容）；② `source` 必须命中本会话授权集合
/// （防 XSS 借解析侧入库任意本地文件）；③ 入库（P2-7：经 embed_closure 真实嵌入，
/// 嵌入不可达时按 PRD R10 降级仅建 FTS5，向量缺口由 reindex_missing 补偿）。
#[tauri::command]
pub fn submit_parsed(state: State<DbState>, input: ParsedInputPayload) -> Result<i64, String> {
    // P2-2：长度口径统一按「字符数」（chars().count()），与 parse.rs MAX_CHARS=2,000,000
    // （按字符）一致——修复中文 2M 字符因字节数 6MB 被误拒的问题。
    if input.content_text.chars().count() > parse::MAX_CHARS {
        return Err("content_text 超过上限".into());
    }
    if let Some(f) = &input.fields {
        if f.chars().count() > parse::MAX_CHARS {
            return Err("fields 超过上限".into());
        }
    }
    if let Some(src) = &input.source {
        if !src.is_empty() {
            let canon = std::fs::canonicalize(src).map_err(|e| format!("source 规范化失败: {e}"))?;
            let canon_s = canon.to_string_lossy().into_owned();
            let auth = state.1.lock().unwrap();
            if !auth.contains(&canon_s) {
                return Err("source 未授权（不在本会话导入清单）".into());
            }
        }
    }
    // 嵌入：已配置嵌入模型时传真实闭包（vec_items 写库，P0-2 修复）；否则跳过（仅 FTS5，PRD R10 降级）
    let conn = state.0.lock().unwrap();
    let embed = embed_closure(&conn);
    ingest::ingest(&conn, &to_ingest_input(input), embed.as_deref()).map_err(|e| e.to_string())
}

/// 解析失败：写 status=parse_failed（不静默丢失）
#[tauri::command]
pub fn submit_parse_failed(
    state: State<DbState>,
    input: ParsedInputPayload,
    reason: String,
) -> Result<i64, String> {
    let conn = state.0.lock().unwrap();
    ingest::ingest_failed(&conn, &to_ingest_input(input), &reason).map_err(|e| e.to_string())
}

/// 图片待 OCR：写 status=ocr_pending（OCR 由 src-tauri ocr 模块完成）。
/// P1-3：feature=ocr 时立即尝试识别并 `complete_ocr` 写回；未启用则保持 ocr_pending（前端标示）。
#[tauri::command]
pub fn submit_ocr_pending(state: State<DbState>, app: AppHandle, input: ParsedInputPayload) -> Result<i64, String> {
    let src = input.source.clone();
    let conn = state.0.lock().unwrap();
    let doc_id = ingest::ingest_ocr_pending(&conn, &to_ingest_input(input)).map_err(|e| e.to_string())?;
    let tess = resolve_tessdata_dir(&app);
    let _ = ocr_doc_if_possible(&conn, doc_id, src.as_deref(), &tess);
    Ok(doc_id)
}

/// OCR 完成：写入文本、改 ok、建索引（已配置嵌入时传真实闭包写向量）
#[tauri::command]
pub fn complete_ocr(
    state: State<DbState>,
    doc_id: i64,
    ocr_text: String,
) -> Result<(), String> {
    let conn = state.0.lock().unwrap();
    let embed = embed_closure(&conn);
    ingest::complete_ocr(&conn, doc_id, &ocr_text, embed.as_deref()).map_err(|e| e.to_string())
}

/// 索引缺口补偿返回值（R5-P2-3）：除补建条数外携带嵌入维度探测状态，
/// 前端配置页据此提示「嵌入模型维度与内置 1024 维不符 → 语义检索已降级为关键词」。
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReindexMissingOut {
    /// 本次补建条数（FTS5 缺口 + 向量缺口）
    pub reindexed: i64,
    /// 嵌入模型维度与内置 1024 维不符（语义检索已降级为关键词）
    pub dim_mismatch: bool,
}

/// 索引缺口补偿（启动时 + 定时调用）：补建缺失索引（FTS5 缺口 + 配置嵌入后向量缺口），
/// 返回补建条数与维度探测状态。已配置嵌入模型时历史文档（曾以 None 入库）会补写 vec_items（P0-2）。
#[tauri::command]
pub fn reindex_missing(state: State<DbState>) -> Result<ReindexMissingOut, String> {
    let conn = state.0.lock().unwrap();
    let embed = embed_closure(&conn);
    let n_fts =
        ingest::rebuild_missing_indexes(&conn, embed.as_deref()).map_err(|e| e.to_string())?;
    let n_vec =
        ingest::rebuild_missing_vectors(&conn, embed.as_deref()).map_err(|e| e.to_string())?;
    // P2-3：mismatch 不再静默——返回给前端供配置页提示（维度不符 → 语义降级关键词）
    let dim_mismatch = ingest::embed_dim_probe_status(&conn)
        .map(|s| s.as_deref() == Some("mismatch"))
        .unwrap_or(false);
    Ok(ReindexMissingOut {
        reindexed: (n_fts + n_vec) as i64,
        dim_mismatch,
    })
}

/// 读取嵌入维度探测状态（R5-P2-3）：返回 "ok" | "mismatch" | null（未配置/未探测）。
/// 轻量命令（仅读 app_meta，不触发重建/探测），供配置页保存模型后检测提示。
#[tauri::command]
pub fn get_embed_probe_status(state: State<DbState>) -> Result<Option<String>, String> {
    let conn = state.0.lock().unwrap();
    ingest::embed_dim_probe_status(&conn).map_err(|e| e.to_string())
}

// ---------------- 文件导入主链路（R1 / P0-3） ----------------

/// 单文件导入结果（回传前端 toast 汇总）
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub path: String,
    pub file_name: String,
    /// "ok" | "parse_failed" | "ocr_pending" | "error"
    pub status: String,
    pub doc_id: Option<i64>,
    pub title: Option<String>,
    pub message: Option<String>,
}

fn err_result(status: &str, path: &str, file_name: &str, message: String) -> ImportResult {
    ImportResult {
        path: path.to_string(),
        file_name: file_name.to_string(),
        status: status.to_string(),
        doc_id: None,
        title: None,
        message: Some(message),
    }
}

/// 构造「文件」类型入库入参（未归类主体，导入后可在资料库/抽屉补充归属）
fn file_ingest_input(title: String, source: String, content_text: String, kind: parse::Kind) -> ingest::IngestInput {
    ingest::IngestInput {
        kind: "file".into(),
        source_kind: kind,
        title,
        content_text,
        fields: None,
        source: Some(source),
        doc_type: None,
        party: None,
        owner: None,
        date_field: None,
        note: None,
        entity_ids: Vec::new(),
        created_at: now_iso(),
    }
}

/// 文件导入主链路（R1 / P0-3）：前端只传路径清单（dialog 多选 / 原生拖拽），
/// Rust 侧读取 → 解析（txt/csv/md/pdf/docx/xlsx，Rust 降级）→ 入库（FTS5 + 切块 + 嵌入可选）。
///
/// 安全契约：① 路径 `canonicalize`（防 symlink 穿越）并记入本会话授权集合；
/// ② 大小/类型白名单由 parse 层校验；③ 图片/无文本层扫描件记 `ocr_pending`（README 已知限制），
/// 不阻塞其他文件；④ 不信任前端传入路径，仅以 canonicalize 后的真实路径为准。
#[tauri::command]
pub fn import_files(
    state: State<DbState>,
    app: AppHandle,
    paths: Vec<String>,
) -> Result<Vec<ImportResult>, String> {
    let mut results = Vec::with_capacity(paths.len());
    for p in paths {
        results.push(import_one_file(&state, &app, &p));
    }
    Ok(results)
}

fn import_one_file(state: &State<DbState>, app: &AppHandle, raw: &str) -> ImportResult {
    let file_name = raw
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(raw)
        .to_string();
    // OCR 词库目录解析一次复用（feature=ocr 时传入 ocr_doc_if_possible；not(ocr) 时忽略）
    let tess = resolve_tessdata_dir(app);

    // 1) canonicalize + 授权（防 symlink 穿越；与 authorize_sources 同一会话集合）
    let canon = match std::fs::canonicalize(raw) {
        Ok(c) => c,
        Err(e) => return err_result("error", raw, &file_name, format!("路径无法访问: {e}")),
    };
    let canon_s = canon.to_string_lossy().into_owned();
    {
        let mut auth = state.1.lock().unwrap();
        auth.insert(canon_s.clone());
    }

    // 2) 大小预检（P1-2：读前先用 metadata 预检，超限直接拒绝，**不先全量读入内存**再拒绝
    //    ——防大文件内存 DoS；bytes 全量读取仅在通过预检后进行）
    let meta = match std::fs::metadata(&canon) {
        Ok(m) => m,
        Err(e) => {
            return err_result("error", &canon_s, &file_name, format!("读取元数据失败: {e}"))
        }
    };
    if meta.len() > parse::MAX_FILE_BYTES as u64 {
        return err_result(
            "parse_failed",
            &canon_s,
            &file_name,
            format!("文件超过大小上限 {} 字节", parse::MAX_FILE_BYTES),
        );
    }
    // 读取字节 + 类型识别 + 大小预检（check_size 二次兜底，防 metadata 与实际读入不一致）
    let bytes = match std::fs::read(&canon) {
        Ok(b) => b,
        Err(e) => return err_result("error", &canon_s, &file_name, format!("读取失败: {e}")),
    };
    if let Err(e) = parse::check_size(&bytes) {
        return err_result("parse_failed", &canon_s, &file_name, e.to_string());
    }
    let ext = canon
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_string();
    let kind = match parse::kind_from_ext(&ext).or_else(|| parse::kind_from_magic(&bytes)) {
        Some(k) => k,
        None => {
            return err_result(
                "parse_failed",
                &canon_s,
                &file_name,
                format!("不支持的文件类型: .{ext}"),
            )
        }
    };

    // 3) 解析文本
    let title = file_name.clone();
    match parse::extract_text(kind, &bytes) {
        Ok(Some(text)) => {
            let input = file_ingest_input(title, canon_s.clone(), text, kind);
            let conn = state.0.lock().unwrap();
            let embed = embed_closure(&conn);
            match ingest::ingest(&conn, &input, embed.as_deref()) {
                Ok(doc_id) => ImportResult {
                    path: canon_s,
                    file_name,
                    status: "ok".into(),
                    doc_id: Some(doc_id),
                    title: Some(input.title),
                    message: None,
                },
                Err(e) => err_result("parse_failed", &canon_s, &file_name, format!("入库失败: {e}")),
            }
        }
        Ok(None) => {
            // 无文本层（扫描件 PDF / 空文档）：图片或 PDF 记 ocr_pending，其余记 parse_failed
            if kind == parse::Kind::Image || kind == parse::Kind::Pdf {
                let input = file_ingest_input(title, canon_s.clone(), String::new(), kind);
                let conn = state.0.lock().unwrap();
                match ingest::ingest_ocr_pending(&conn, &input) {
                    Ok(doc_id) => {
                        // P1-3：feature=ocr 时立即识别并写回；未启用则保持 ocr_pending
                        let ocr_ok = ocr_doc_if_possible(&conn, doc_id, input.source.as_deref(), &tess);
                        ImportResult {
                            path: canon_s,
                            file_name,
                            status: if ocr_ok { "ok".into() } else { "ocr_pending".into() },
                            doc_id: Some(doc_id),
                            title: Some(input.title),
                            message: if ocr_ok {
                                None
                            } else {
                                Some("无文本层，待 OCR".into())
                            },
                        }
                    }
                    Err(e) => err_result("error", &canon_s, &file_name, format!("入库失败: {e}")),
                }
            } else {
                err_result("parse_failed", &canon_s, &file_name, "文件无文本内容".into())
            }
        }
        Err(parse::ParseError::Unsupported(reason)) => {
            if kind == parse::Kind::Image {
                let input = file_ingest_input(title, canon_s.clone(), String::new(), kind);
                let conn = state.0.lock().unwrap();
                match ingest::ingest_ocr_pending(&conn, &input) {
                    Ok(doc_id) => {
                        // P1-3：feature=ocr 时立即识别并写回；未启用则保持 ocr_pending
                        let ocr_ok = ocr_doc_if_possible(&conn, doc_id, input.source.as_deref(), &tess);
                        ImportResult {
                            path: canon_s,
                            file_name,
                            status: if ocr_ok { "ok".into() } else { "ocr_pending".into() },
                            doc_id: Some(doc_id),
                            title: Some(input.title),
                            message: if ocr_ok { None } else { Some(reason) },
                        }
                    }
                    Err(e) => err_result("error", &canon_s, &file_name, format!("入库失败: {e}")),
                }
            } else {
                let input = file_ingest_input(title, canon_s.clone(), String::new(), kind);
                let conn = state.0.lock().unwrap();
                match ingest::ingest_failed(&conn, &input, &reason) {
                    Ok(doc_id) => ImportResult {
                        path: canon_s,
                        file_name,
                        status: "parse_failed".into(),
                        doc_id: Some(doc_id),
                        title: Some(input.title),
                        message: Some(reason),
                    },
                    Err(e) => err_result("error", &canon_s, &file_name, format!("入库失败: {e}")),
                }
            }
        }
        Err(e) => {
            let reason = e.to_string();
            let input = file_ingest_input(title, canon_s.clone(), String::new(), kind);
            let conn = state.0.lock().unwrap();
            match ingest::ingest_failed(&conn, &input, &reason) {
                Ok(doc_id) => ImportResult {
                    path: canon_s,
                    file_name,
                    status: "parse_failed".into(),
                    doc_id: Some(doc_id),
                    title: Some(input.title),
                    message: Some(reason),
                },
                Err(e2) => err_result("error", &canon_s, &file_name, format!("入库失败: {e2}")),
            }
        }
    }
}

// ---------------- OCR 接入（P1-3；feature=ocr 门控，系统 tesseract 依赖见 README） ----------------

/// 解析 Tesseract 词库目录（无 cfg，本机 not(ocr) 编译仍可校验类型/签名）。
/// 优先级：环境变量 `TESSDATA_PREFIX` → 随包 resources（`resource_dir()/resources/tessdata`）→ 相对 `tessdata`。
/// 随包资源由 `tauri.conf.json` 的 `bundle.resources` 声明，打包/运行均经 `app.path().resource_dir()` 定位，
/// 不依赖进程 cwd，适配 Windows 安装目录、macOS .app、Linux /usr/lib 等任意路径。
fn resolve_tessdata_dir(app: &AppHandle) -> std::path::PathBuf {
    if let Ok(p) = std::env::var("TESSDATA_PREFIX") {
        if !p.trim().is_empty() {
            return std::path::PathBuf::from(p);
        }
    }
    if let Ok(res) = app.path().resource_dir() {
        // 主路径：bundle.resources 声明的 resources/tessdata 子目录
        let p = res.join("resources").join("tessdata");
        if p.exists() {
            return p;
        }
        // 兼容：直接把 tessdata 放在 resource_dir 根（调试/历史）
        let alt = res.join("tessdata");
        if alt.exists() {
            return alt;
        }
    }
    std::path::PathBuf::from("tessdata")
}

/// 尝试对单条 `ocr_pending` 文档执行 OCR（feature=ocr 时经 ocr.rs tesseract 识别 → `complete_ocr` 写回）。
/// 返回是否已识别完成并写回文本（true 时文档状态转为 ok）。
///
/// feature 未启用：保持 `ocr_pending` 状态，前端明确标示「扫描件待 OCR」，不阻塞（P1-3 务实范围）。
#[cfg(feature = "ocr")]
fn ocr_doc_if_possible(
    conn: &rusqlite::Connection,
    doc_id: i64,
    source: Option<&str>,
    tessdata: &std::path::Path,
) -> bool {
    let Some(src) = source else { return false };
    if src.trim().is_empty() {
        return false;
    }
    let img_path = std::path::Path::new(src);
    match crate::ocr::ocr_image(img_path, tessdata, "chi_sim+eng") {
        Ok(text) if !text.trim().is_empty() => {
            let embed = embed_closure(conn);
            match ingest::complete_ocr(conn, doc_id, &text, embed.as_deref()) {
                Ok(()) => {
                    crate::log::info("[ocr]", &format!("识别完成 doc={doc_id}"));
                    true
                }
                Err(e) => {
                    crate::log::error("[ocr]", &format!("complete_ocr 失败 doc={doc_id}: {e}"));
                    false
                }
            }
        }
        Ok(_) => {
            crate::log::info("[ocr]", &format!("识别结果为空 doc={doc_id}"));
            false
        }
        Err(e) => {
            crate::log::error("[ocr]", &format!("识别失败 doc={doc_id}: {e}"));
            false
        }
    }
}

/// feature 未启用：保持 ocr_pending（前端标示「扫描件待 OCR」），不阻塞。
#[cfg(not(feature = "ocr"))]
fn ocr_doc_if_possible(
    _conn: &rusqlite::Connection,
    _doc_id: i64,
    _source: Option<&str>,
    _tessdata: &std::path::Path,
) -> bool {
    false
}

// ---------------- 文档关联（业务条目 ↔ 文件） ----------------

#[derive(Debug, serde::Serialize)]
pub struct DocLinkOut {
    pub id: i64,
    pub kind: String,
    pub direction: String,
}

/// 列出与某文档关联的所有文档（双向）
#[tauri::command]
pub fn list_links(state: State<DbState>, doc_id: i64) -> Result<Vec<DocLinkOut>, String> {
    let conn = state.0.lock().unwrap();
    let links = entities::list_links(&conn, doc_id).map_err(|e| e.to_string())?;
    Ok(links
        .into_iter()
        .map(|l| DocLinkOut { id: l.id, kind: l.kind, direction: l.direction })
        .collect())
}

#[tauri::command]
pub fn create_link(
    state: State<DbState>,
    from_id: i64,
    to_id: i64,
    kind: String,
) -> Result<(), String> {
    let conn = state.0.lock().unwrap();
    entities::create_link(&conn, from_id, to_id, &kind).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_link(
    state: State<DbState>,
    from_id: i64,
    to_id: i64,
) -> Result<(), String> {
    let conn = state.0.lock().unwrap();
    entities::delete_link(&conn, from_id, to_id).map_err(|e| e.to_string())
}

// ---------------- 文件夹监控 ----------------

/// 启动监控某文件夹（新文件自动尝试入库，需已授权来源路径）。
/// `default_entity_ids`：该目录新入库文件的默认归属主体（可多选，空=未归类）。
#[tauri::command]
pub fn start_folder_watch(
    state: State<DbState>,
    path: String,
default_entity_ids: Option<Vec<i64>>,
) -> Result<(), String> {
    let arc = Arc::clone(&state.0);
    crate::watch::start_watch(arc, &path, default_entity_ids.unwrap_or_default())
        .map_err(|e| e.to_string())
}

/// 停止当前文件夹监控
#[tauri::command]
pub fn stop_folder_watch() -> Result<(), String> {
    crate::watch::stop_watch().map_err(|e| e.to_string())
}

/// 文件夹监控状态（序列化为对象，与前端 FolderWatchStatus 契约一致）
#[derive(Debug, serde::Serialize)]
pub struct FolderWatchStatusOut {
    pub running: bool,
    pub path: Option<String>,
}

/// 获取文件夹监控状态
#[tauri::command]
pub fn get_folder_watch_status() -> Result<FolderWatchStatusOut, String> {
    let (running, path) = crate::watch::is_watching();
    Ok(FolderWatchStatusOut { running, path })
}
