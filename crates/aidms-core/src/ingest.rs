//! 入库编排（阶段 3 核心）
//!
//! 流程：`解析结果/业务字段` -> 写 `document` -> 关联多主体 -> 建索引（FTS5 双表 + 切块 + 向量可选）。
//! - 向量写入在嵌入**不可达/未配置时整体跳过**，仅保留 FTS5 全文索引，不阻塞入库（PRD R10 降级）。
//! - 索引缺口补偿（启动时 + 定时）：扫描 `document` 存在但 FTS 缺行的记录，幂等补建（最终一致闭环）。
//! - 状态机：`ok` / `parse_failed`（解析失败）/ `ocr_pending`（图片待 OCR，OCR 完成转 `ok`）。

use rusqlite::{params, Connection, OptionalExtension, Result as SqlResult};
use thiserror::Error;

use crate::entities::{self, NewDocument};
use crate::index::{chunk_text, write_chunks, write_embedding, index_document_fts};
use crate::parse::Kind;
use crate::schema as S;

/// 嵌入函数：文本 -> f32 向量（未配置时为 None，跳过向量写入）
pub type EmbedFn = dyn Fn(&str) -> Result<Vec<f32>, String>;

#[derive(Debug, Error)]
pub enum IngestError {
    #[error("DB 错误: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("嵌入失败: {0}")]
    Embed(String),
    #[error("非法入库类型: {0}（仅允许 file / business）")]
    InvalidKind(String),
}

/// 已解析结果的入库入参（对应 src-tauri `submit_parsed` 契约）
#[derive(Debug, Clone)]
pub struct IngestInput {
    pub kind: String,       // 'file' | 'business'
    pub source_kind: Kind,  // 原始类型（业务条目=Business）
    pub title: String,
    pub content_text: String,
    pub fields: Option<String>, // JSON（业务条目结构化字段，拼入 content 便于按字段检索）
    pub source: Option<String>,
    pub doc_type: Option<String>,
    pub party: Option<String>,
    pub owner: Option<String>,
    pub date_field: Option<String>,
    pub note: Option<String>,
    pub entity_ids: Vec<i64>, // 归属主体（多对多，可空→未归类）
    pub created_at: String,
}

/// 业务条目把 `fields` JSON 的字段值拼入 content，便于按字段检索
fn build_content_text(input: &IngestInput) -> String {
    let mut c = input.content_text.clone();
    if input.kind == "business" {
        if let Some(fields) = &input.fields {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(fields) {
                if let Some(obj) = v.as_object() {
                    let extra: Vec<String> = obj
                        .values()
                        .filter_map(|x| match x {
                            serde_json::Value::String(s) => Some(s.clone()),
                            serde_json::Value::Number(n) => Some(n.to_string()),
                            serde_json::Value::Bool(b) => Some(b.to_string()),
                            _ => None,
                        })
                        .collect();
                    if !extra.is_empty() {
                        c = format!("{}\n{}", c, extra.join("\n"));
                    }
                }
            }
        }
    }
    c
}

/// 入库主流程：document + 实体关联 + 建索引（P1-5：整体包事务，任一步失败整体回滚）
///
/// - 事务保证：`create_document` / `link_entity` / FTS5+trigram / 切块 / 向量 要么全部落库，
///   要么全部回滚，不产生半成品（例如嵌入失败不残留「无索引的 document 行」）。
/// - vec0 虚拟表在事务内可写（sqlite-vec 0.1.9 静态注册，事务语义与普通表一致，已验证）。
pub fn ingest(
    conn: &Connection,
    input: &IngestInput,
    embed: Option<&EmbedFn>,
) -> Result<i64, IngestError> {
    // 类型白名单校验（防非法 kind 入库；失败在事务内回滚，不落任何行）
    if input.kind != "file" && input.kind != "business" {
        return Err(IngestError::InvalidKind(input.kind.clone()));
    }
    let tx = conn.unchecked_transaction()?;
    let content = build_content_text(input);
    let d = NewDocument {
        kind: input.kind.clone(),
        title: input.title.clone(),
        doc_type: input.doc_type.clone(),
        source: input.source.clone(),
        content_text: Some(content.clone()),
        party: input.party.clone(),
        owner: input.owner.clone(),
        date_field: input.date_field.clone(),
        note: input.note.clone(),
        fields: input.fields.clone(),
        status: Some("ok".into()),
        created_at: input.created_at.clone(),
    };
    let doc_id = entities::create_document(&tx, &d)?;
    for eid in &input.entity_ids {
        entities::link_entity(&tx, doc_id, *eid)?;
    }
    reindex(&tx, doc_id, &input.title, &content, embed)?;
    tx.commit()?;
    Ok(doc_id)
}

/// 解析失败：写 `document`（status=parse_failed），不建索引，列表/搜索明确标示，不静默丢失
pub fn ingest_failed(
    conn: &Connection,
    input: &IngestInput,
    reason: &str,
) -> Result<i64, IngestError> {
    // P2-1：与 ingest 主入口一致的 kind 白名单校验
    if input.kind != "file" && input.kind != "business" {
        return Err(IngestError::InvalidKind(input.kind.clone()));
    }
    let tx = conn.unchecked_transaction()?;
    let d = NewDocument {
        kind: input.kind.clone(),
        title: input.title.clone(),
        doc_type: input.doc_type.clone(),
        source: input.source.clone(),
        content_text: Some(format!("解析失败：{}", reason)),
        party: input.party.clone(),
        owner: input.owner.clone(),
        date_field: input.date_field.clone(),
        note: input.note.clone(),
        fields: input.fields.clone(),
        status: Some("parse_failed".into()),
        created_at: input.created_at.clone(),
    };
    let doc_id = entities::create_document(&tx, &d)?;
    for eid in &input.entity_ids {
        entities::link_entity(&tx, doc_id, *eid)?;
    }
    tx.commit()?;
    Ok(doc_id)
}

/// 图片待 OCR：写 `document`（status=ocr_pending），OCR 完成调用 [`complete_ocr`]
pub fn ingest_ocr_pending(
    conn: &Connection,
    input: &IngestInput,
) -> Result<i64, IngestError> {
    // P2-1：与 ingest 主入口一致的 kind 白名单校验
    if input.kind != "file" && input.kind != "business" {
        return Err(IngestError::InvalidKind(input.kind.clone()));
    }
    let tx = conn.unchecked_transaction()?;
    let d = NewDocument {
        kind: input.kind.clone(),
        title: input.title.clone(),
        doc_type: input.doc_type.clone(),
        source: input.source.clone(),
        content_text: None,
        party: input.party.clone(),
        owner: input.owner.clone(),
        date_field: input.date_field.clone(),
        note: input.note.clone(),
        fields: input.fields.clone(),
        status: Some("ocr_pending".into()),
        created_at: input.created_at.clone(),
    };
    let doc_id = entities::create_document(&tx, &d)?;
    for eid in &input.entity_ids {
        entities::link_entity(&tx, doc_id, *eid)?;
    }
    tx.commit()?;
    Ok(doc_id)
}

/// OCR 完成：写入文本、改 `ok`、建索引（P1-5：整体包事务，写入与建索引原子）
pub fn complete_ocr(
    conn: &Connection,
    doc_id: i64,
    ocr_text: &str,
    embed: Option<&EmbedFn>,
) -> Result<(), IngestError> {
    let tx = conn.unchecked_transaction()?;
    let title = entities::get_document(&tx, doc_id)?
        .map(|d| d.title)
        .unwrap_or_default();
    tx.execute(
        &format!(
            "UPDATE {t} SET content_text=?, status='ok', updated_at=CURRENT_TIMESTAMP WHERE id=?",
            t = S::TABLE_DOCUMENT
        ),
        params![ocr_text, doc_id],
    )?;
    reindex(&tx, doc_id, &title, ocr_text, embed)?;
    tx.commit()?;
    Ok(())
}

/// 幂等重建索引：先清旧 FTS5 双表 + chunk + vec，再重建（不写 document 行）
fn reindex(
    conn: &Connection,
    doc_id: i64,
    title: &str,
    content: &str,
    embed: Option<&EmbedFn>,
) -> Result<(), IngestError> {
    // 清旧（幂等：缺失也不报错）
    conn.execute(
        &format!("DELETE FROM {t} WHERE rowid=?", t = S::TABLE_DOCUMENT_FTS),
        [doc_id],
    )?;
    conn.execute(
        &format!("DELETE FROM {t} WHERE rowid=?", t = S::TABLE_DOCUMENT_FTS_TRIGRAM),
        [doc_id],
    )?;
    let old_chunk_ids: Vec<i64> = {
        let mut stmt = conn.prepare(&format!("SELECT id FROM {t} WHERE document_id=?", t = S::TABLE_CHUNK))?;
        let rows = stmt.query_map([doc_id], |r| r.get(0))?;
        rows.collect::<SqlResult<Vec<_>>>()?
    };
    for cid in &old_chunk_ids {
        conn.execute(
            &format!("DELETE FROM {t} WHERE rowid=?", t = S::TABLE_VEC_ITEMS),
            [*cid],
        )?;
    }
    conn.execute(
        &format!("DELETE FROM {t} WHERE document_id=?", t = S::TABLE_CHUNK),
        [doc_id],
    )?;

    // 建新
    index_document_fts(conn, doc_id, title, content)?;
    let chunks = chunk_text(content, 400, 350);
    let chunk_ids = write_chunks(conn, doc_id, &chunks)?;
    // TODO(P2-10 已知限制)：嵌入为同步网络调用且本函数持 DB 锁执行（调用方
    // `commands::submit_parsed`/`reindex_missing` 均持 `Mutex<Connection>`）。
    // 嵌入慢时阻塞其它写操作；低成本重构需把嵌入挪到锁外（先落库、异步补嵌），
    // 当前以「嵌入失败不阻塞入库 + 缺口补偿」务实缓解，记录于 README「已知限制」。
    if let Some(embed) = embed {
        for (cid, (_, _, text)) in chunk_ids.iter().zip(chunks.iter()) {
            match embed(text) {
                Ok(v) => write_embedding(conn, *cid, &v)?,
                Err(e) => {
                    // 嵌入不可达：按 PRD R10 降级，仅保留 FTS5 全文索引，**不阻塞入库**。
                    // 向量缺口由 `rebuild_missing_vectors` 在模型恢复后补偿（幂等）。
                    // 日志脱敏（P2-10）：避免密钥/长 token 泄入日志
                    eprintln!("[ingest] 嵌入失败，跳过该文档向量（保留 FTS5）: {}", crate::security::redact_log(&e));
                    break;
                }
            }
        }
    }
    Ok(())
}

/// 索引缺口补偿（启动时 + 定时）：扫描 `document`(status=ok) 存在但 FTS 缺行的记录，补建索引。
/// 每条文档的补建包在独立事务内（P2-5：单文档失败不回滚其它文档）。返回补建条数。
/// R5-P2-4：合并消费 `META_REINDEX_PENDING` 标记（字段事务已提交后重建失败等场景），
/// 补建成功即移除标记，失败保留（下轮再试），保证「重建失败 → 下轮补偿能补」闭环。
pub fn rebuild_missing_indexes(
    conn: &Connection,
    embed: Option<&EmbedFn>,
) -> Result<usize, IngestError> {
    let missing: Vec<(i64, String, String)> = {
        let mut stmt = conn.prepare(&format!(
            "SELECT d.id, d.title, COALESCE(d.content_text,'')
             FROM {d} d LEFT JOIN {fts} f ON d.id = f.rowid
             WHERE f.rowid IS NULL AND d.status='ok'",
            d = S::TABLE_DOCUMENT,
            fts = S::TABLE_DOCUMENT_FTS
        ))?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect::<SqlResult<Vec<_>>>()?
    };
    // 合并「标记待补建」的文档（不重复：FTS 缺行已覆盖的不再加）
    let pending: Vec<i64> = read_app_meta(conn, META_REINDEX_PENDING)?
        .as_deref()
        .and_then(|s| serde_json::from_str::<Vec<i64>>(s).ok())
        .unwrap_or_default();
    let mut rows: Vec<(i64, String, String)> = missing;
    for id in &pending {
        if rows.iter().any(|r| r.0 == *id) {
            continue;
        }
        let row: Option<(String, Option<String>)> = conn
            .query_row(
                &format!(
                    "SELECT title, content_text FROM {t} WHERE id=?",
                    t = S::TABLE_DOCUMENT
                ),
                [*id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((title, content)) = row {
            rows.push((*id, title, content.unwrap_or_default()));
        }
    }
    let n = rows.len();
    let mut remaining: Vec<i64> = pending.clone();
    for (id, title, content) in rows {
        let tx = conn.unchecked_transaction()?;
        if let Err(e) = reindex(&tx, id, &title, &content, embed) {
            // 失败：不回滚其它文档，保留 pending 标记（下轮再试）
            eprintln!(
                "[ingest] 索引补建失败 doc={id}: {}（保留待补偿标记）",
                crate::security::redact_log(&e.to_string())
            );
            continue; // tx 在此作用域结束自动回滚
        }
        tx.commit()?;
        remaining.retain(|x| *x != id);
    }
    if remaining.len() != pending.len() {
        // 有成功移除的标记 → 回写剩余列表（空则清空）
        write_app_meta(
            conn,
            META_REINDEX_PENDING,
            &serde_json::to_string(&remaining).unwrap_or_else(|_| "[]".to_string()),
        )?;
    }
    Ok(n)
}

/// 标记文档索引待补建（R5-P2-4）：`set_field_value` 等场景在字段事务已提交后
/// 重建索引失败时调用，确保下轮 `rebuild_missing_indexes` 补偿（不静默丢失语义索引陈旧状态）。
/// 幂等：同一 doc_id 重复标记不重复入列。失败仅记录日志（标记写入失败不阻塞业务）。
pub fn mark_document_reindex_pending(
    conn: &Connection,
    doc_id: i64,
    reason: &str,
) -> Result<(), IngestError> {
    let current = read_app_meta(conn, META_REINDEX_PENDING)?;
    let mut list: Vec<i64> = current
        .as_deref()
        .and_then(|s| serde_json::from_str::<Vec<i64>>(s).ok())
        .unwrap_or_default();
    if !list.contains(&doc_id) {
        list.push(doc_id);
        // 上限防膨胀（超出丢弃最旧）
        if list.len() > META_REINDEX_PENDING_MAX {
            list.drain(0..(list.len() - META_REINDEX_PENDING_MAX));
        }
        write_app_meta(
            conn,
            META_REINDEX_PENDING,
            &serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_string()),
        )?;
    }
    eprintln!(
        "[ingest] 文档索引待补建 doc={doc_id}: {}",
        crate::security::redact_log(reason)
    );
    Ok(())
}

/// 重建单文档完整索引（FTS5 双表 + chunk + 向量），供「自定义字段值变更后需同步语义检索」
/// 场景复用（P2-5）。`embed=None` 时仅重建 FTS5/chunk（向量跳过）；已配置嵌入时由调用方
/// 传入闭包，`reindex` 会先清旧 chunk/vec 再重建（幂等）。
pub fn rebuild_document_index(
    conn: &Connection,
    doc_id: i64,
    embed: Option<&EmbedFn>,
) -> Result<(), IngestError> {
    let (title, content): (String, Option<String>) = conn.query_row(
        &format!(
            "SELECT title, content_text FROM {t} WHERE id=?",
            t = S::TABLE_DOCUMENT
        ),
        [doc_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    reindex(conn, doc_id, &title, content.as_deref().unwrap_or(""), embed)
}

// ---------------- P1-3 嵌入维度探测缓存（app_meta） ----------------
//
// 背景：vec0 固定 1024 维，若用户配置了 512/768/1536 维模型，`write_embedding` 维度守卫
// （P1-1）返回 Ok 不写 vec → 文档永远「缺向量」→ `rebuild_missing_vectors` 每轮 SELECT
// 重新命中 → 全库 DELETE+重建 FTS/chunk + N 次网络嵌入，持锁且永不收敛。
// 方案：模型维度探测前置。每次调用 `rebuild_missing_vectors` 时先嵌入一个短探测文本：
// - 维度 == EMBEDDING_DIM → 缓存 "ok"，正常补建；
// - 维度 != EMBEDDING_DIM → 缓存 "mismatch"，返回 0（不探测、不重嵌），避免每 10 分钟
//   UI 冻结 + 无谓网络开销；模型修复后由 `sync_embed_dim_probe_model`（模型变更时清缓存）恢复。
// - 嵌入不可达 → 本次返回 0（不缓存，下次循环再试，与「嵌入降级」语义一致）。
const META_EMBED_DIM_PROBE: &str = "embed_dim_probe";
const META_EMBED_DIM_PROBE_MODEL: &str = "embed_dim_probe_model";
/// R5-P2-4：待补建索引的文档 id 列表（JSON 数组）。字段事务已提交后重建失败等场景
/// 由 `mark_document_reindex_pending` 写入，`rebuild_missing_indexes` 消费（成功移除）。
const META_REINDEX_PENDING: &str = "reindex_pending_docs";
/// 待补建标记上限（防 app_meta 无限膨胀；超出丢弃最旧）
const META_REINDEX_PENDING_MAX: usize = 200;

fn read_app_meta(conn: &Connection, key: &str) -> Result<Option<String>, IngestError> {
    conn.query_row(
        "SELECT value FROM app_meta WHERE key=?",
        [key],
        |r| r.get(0),
    )
    .optional()
    .map_err(IngestError::Db)
}

fn write_app_meta(conn: &Connection, key: &str, value: &str) -> Result<(), IngestError> {
    conn.execute(
        "INSERT OR REPLACE INTO app_meta(key, value, updated_at) VALUES (?,?,datetime('now'))",
        params![key, value],
    )?;
    Ok(())
}

/// 模型维度探测：嵌入一个短探测文本，返回向量维度；嵌入不可达返回 None。
fn probe_embed_dimension(embed: &EmbedFn) -> Option<usize> {
    embed("维度探测").ok().map(|v| v.len())
}

/// 模型变更时同步维度探测缓存（P1-3）：由 src-tauri 层在读取嵌入配置后调用。
/// 若 app_meta 记录的探测模型与当前 `model` 不一致，清除旧探测状态强制重新探测
/// （否则缓存 mismatch 会在用户换成正确维度模型后永久阻塞向量补建）。
pub fn sync_embed_dim_probe_model(conn: &Connection, model: &str) -> Result<(), IngestError> {
    let cached = read_app_meta(conn, META_EMBED_DIM_PROBE_MODEL)?;
    if cached.as_deref() != Some(model) {
        conn.execute(
            "DELETE FROM app_meta WHERE key IN (?,?)",
            params![META_EMBED_DIM_PROBE, META_EMBED_DIM_PROBE_MODEL],
        )?;
        write_app_meta(conn, META_EMBED_DIM_PROBE_MODEL, model)?;
    }
    Ok(())
}

/// 读取嵌入维度探测状态（R5-P2-3）：返回 `Some("ok")` / `Some("mismatch")` / `None`（未探测/未配置）。
/// 供 src-tauri `reindex_missing` 与配置页提示「嵌入模型维度与内置 1024 维不符，
/// 语义检索已降级为关键词」（mismatch 不再静默）。
pub fn embed_dim_probe_status(conn: &Connection) -> Result<Option<String>, IngestError> {
    read_app_meta(conn, META_EMBED_DIM_PROBE)
}

/// 向量缺口补偿（P0-2）：文档已有 FTS5/chunk 但缺 `vec_items`（历史以 `None` 嵌入入库）时，
/// 在配置了嵌入模型后补写向量。幂等：`reindex` 先清旧 chunk/vec 再重建。
/// 未配置嵌入（embed=None）时直接返回 0（不重建，避免无意义写库）。返回补建条数。
///
/// P1-3：维度探测前置——已缓存 mismatch（模型维度与 vec0 不符）时直接返回 0（不探测不重嵌，
/// 防止每轮 SELECT 重新命中 → 全库重建 + N 次网络嵌入永不收敛）；探测不符当场缓存并返回 0。
/// R5-P2-2：已缓存 ok（维度匹配）时与 mismatch 对称跳过探测，直接进入缺向量 SELECT——
/// 不再每轮重复嵌入探测文本（write-only 缓存修复）；模型变更由 sync_embed_dim_probe_model 清缓存。
/// P2-5：每条文档的补建包在独立事务内（单文档失败不回滚其它文档）。
pub fn rebuild_missing_vectors(
    conn: &Connection,
    embed: Option<&EmbedFn>,
) -> Result<usize, IngestError> {
    let Some(embed) = embed else {
        return Ok(0);
    };
    // P1-3：已知坏维度模型 → 直接返回 0（连探测都跳过）
    let probe = read_app_meta(conn, META_EMBED_DIM_PROBE)?;
    if probe.as_deref() == Some("mismatch") {
        return Ok(0);
    }
    // R5-P2-2：已缓存 ok（维度已确认匹配）→ 与 mismatch 对称**跳过探测**，直接进入
    // 缺向量补建 SELECT，不再每轮重复嵌入探测文本（写 ok 后无人读的 write-only 缓存修复）。
    // 正确性由 `sync_embed_dim_probe_model`（模型名变更时清缓存）保证——模型没变则维度不变。
    if probe.as_deref() != Some("ok") {
        // 维度探测：每次调用最多一次网络嵌入；不符写 mismatch 后终止本轮
        match probe_embed_dimension(embed) {
            Some(dim) if dim == crate::index::EMBEDDING_DIM => {
                write_app_meta(conn, META_EMBED_DIM_PROBE, "ok")?;
            }
            Some(dim) => {
                eprintln!(
                    "[ingest] 嵌入模型维度 {dim} 与 vec0 固定维度 {} 不符，终止本轮向量补建（缓存 mismatch，模型修复后自动恢复）",
                    crate::index::EMBEDDING_DIM
                );
                write_app_meta(conn, META_EMBED_DIM_PROBE, "mismatch")?;
                return Ok(0);
            }
            None => {
                // 嵌入不可达：本次跳过（不缓存），下次循环再试
                return Ok(0);
            }
        }
    }
    let missing: Vec<(i64, String, String)> = {
        let mut stmt = conn.prepare(&format!(
            "SELECT DISTINCT c.document_id, d.title, COALESCE(d.content_text,'')
             FROM {c} c
             JOIN {d} d ON d.id = c.document_id
             LEFT JOIN {v} v ON c.id = v.rowid
             WHERE v.rowid IS NULL AND d.status='ok'",
            c = S::TABLE_CHUNK,
            d = S::TABLE_DOCUMENT,
            v = S::TABLE_VEC_ITEMS
        ))?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect::<SqlResult<Vec<_>>>()?
    };
    let n = missing.len();
    for (id, title, content) in missing {
        let tx = conn.unchecked_transaction()?;
        reindex(&tx, id, &title, &content, Some(embed))?;
        tx.commit()?;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::index::EMBEDDING_DIM;

    fn setup() -> Connection {
        db::open(":memory:").unwrap()
    }

    fn input(kind: &str, title: &str, content: &str) -> IngestInput {
        IngestInput {
            kind: kind.into(),
            source_kind: Kind::Txt,
            title: title.into(),
            content_text: content.into(),
            fields: None,
            source: None,
            doc_type: None,
            party: None,
            owner: None,
            date_field: None,
            note: None,
            entity_ids: vec![],
            created_at: "2024-01-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn ingest_invalid_kind_rejected_no_row() {
        // P1-5：非法 kind 在事务内被拒绝，不落任何行
        let conn = setup();
        let res = ingest(&conn, &input("bad_kind", "标题", "内容"), None);
        assert!(matches!(res, Err(IngestError::InvalidKind(_))));
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM document", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "非法 kind 时 document 不得落库");
    }

    #[test]
    fn ingest_transaction_rolls_back_on_mid_failure() {
        // P1-5：create_document 之后的步骤失败（用触发器模拟中途失败），
        // 整个事务回滚 → document/FTS/chunk 均无残留（不产生半成品）。
        let conn = setup();
        conn.execute_batch(
            "CREATE TRIGGER fail_after_doc AFTER INSERT ON document
             BEGIN SELECT RAISE(ABORT, 'injected mid-flow failure'); END;",
        )
        .unwrap();
        let res = ingest(&conn, &input("file", "半成品测试", "正文内容"), None);
        assert!(res.is_err(), "中途失败应返回 Err");
        let doc_n: i64 = conn
            .query_row("SELECT COUNT(*) FROM document", [], |r| r.get(0))
            .unwrap();
        assert_eq!(doc_n, 0, "事务应回滚 document");
        let fts_n: i64 = conn
            .query_row("SELECT COUNT(*) FROM document_fts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fts_n, 0, "事务应回滚 FTS5");
        let chunk_n: i64 = conn
            .query_row("SELECT COUNT(*) FROM chunk", [], |r| r.get(0))
            .unwrap();
        assert_eq!(chunk_n, 0, "事务应回滚 chunk");
    }

    #[test]
    fn ingest_wrong_dim_embed_keeps_fts_no_vec() {
        // P1-1 + P1-5：嵌入闭包返回错误维度 → 不报错、不写 vec，
        // 文档保持 FTS 可搜（嵌入失败不污染数据）。
        let conn = setup();
        let embed = |_t: &str| Ok(vec![0.1f32; 5]); // 错误维度（非 1024）
        let doc_id = ingest(&conn, &input("file", "维度不符文档", "可被全文检索的正文"), Some(&embed))
            .unwrap();
        let fts_n: i64 = conn
            .query_row("SELECT COUNT(*) FROM document_fts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fts_n, 1, "FTS5 应正常建立");
        let vec_n: i64 = conn
            .query_row("SELECT COUNT(*) FROM vec_items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(vec_n, 0, "维度不符时不得写入 vec_items");
        // 文档状态为 ok（不被维度问题污染）
        let status: String = conn
            .query_row("SELECT status FROM document WHERE id=?", [doc_id], |r| r.get(0))
            .unwrap();
        assert_eq!(status, "ok");
    }

    #[test]
    fn ingest_valid_embed_writes_vec() {
        // P1-1：维度正确（1024）→ 向量正常写库
        let conn = setup();
        let embed = |_t: &str| Ok(vec![0.25f32; EMBEDDING_DIM]);
        let doc_id = ingest(
            &conn,
            &input("file", "维度正确文档", "一段用于切块和向量化的正文内容，长度适中。"),
            Some(&embed),
        )
        .unwrap();
        let vec_n: i64 = conn
            .query_row("SELECT COUNT(*) FROM vec_items", [], |r| r.get(0))
            .unwrap();
        assert!(vec_n > 0, "维度正确时应写入 vec_items（chunk 数 > 0）");
        let _ = doc_id;
    }

    #[test]
    fn rebuild_missing_vectors_dim_mismatch_cached_returns_zero() {
        // P1-3：维度不符时首次探测 → 缓存 mismatch 返回 0；第二次调用不触发嵌入（计数验证）
        use std::sync::atomic::{AtomicUsize, Ordering};
        let conn = setup();
        // 用 None 嵌入入库 → 有 FTS/chunk 但缺向量（模拟历史数据）
        ingest(&conn, &input("file", "缺向量文档", "正文内容"), None).unwrap();

        let bad_embed = |_t: &str| Ok(vec![0.1f32; 512]); // 错误维度
        let n1 = rebuild_missing_vectors(&conn, Some(&bad_embed)).unwrap();
        assert_eq!(n1, 0, "维度不符应返回 0（不重嵌）");

        // 第二次调用：命中缓存 mismatch → 不应再调用嵌入闭包
        let calls = std::sync::Arc::new(AtomicUsize::new(0));
        let calls_in = calls.clone();
        let count_embed = move |_t: &str| {
            calls_in.fetch_add(1, Ordering::SeqCst);
            Ok(vec![0.1f32; 512])
        };
        let n2 = rebuild_missing_vectors(&conn, Some(&count_embed)).unwrap();
        assert_eq!(n2, 0, "缓存 mismatch 后应返回 0");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "缓存 mismatch 后不应再触发嵌入（永不收敛防护）"
        );
    }

    #[test]
    fn rebuild_missing_vectors_model_change_recovers() {
        // P1-3：模型变更（sync_embed_dim_probe_model 清缓存）后，正确维度可恢复补建
        let conn = setup();
        ingest(&conn, &input("file", "缺向量文档", "正文内容"), None).unwrap();
        // 先坏维度 → mismatch 缓存
        let bad = |_t: &str| Ok(vec![0.0f32; 8]);
        assert_eq!(rebuild_missing_vectors(&conn, Some(&bad)).unwrap(), 0);
        // 模型变更 → 清缓存
        crate::ingest::sync_embed_dim_probe_model(&conn, "new-model").unwrap();
        // 正确维度 → 正常补建
        let good = |_t: &str| Ok(vec![0.0f32; EMBEDDING_DIM]);
        let n = rebuild_missing_vectors(&conn, Some(&good)).unwrap();
        assert_eq!(n, 1, "模型修复后应恢复向量补建");
        let vec_n: i64 = conn
            .query_row("SELECT COUNT(*) FROM vec_items", [], |r| r.get(0))
            .unwrap();
        assert!(vec_n > 0);
    }

    #[test]
    fn rebuild_missing_vectors_ok_cached_skips_probe() {
        // R5-P2-2：ok 缓存后不再每轮重复探测嵌入（修复 write-only 缓存）
        use std::sync::atomic::{AtomicUsize, Ordering};
        let conn = setup();
        // 正确维度嵌入入库 → 已有向量（无缺向量，但首次调用仍会探测并缓存 ok）
        let embed = |_t: &str| Ok(vec![0.1f32; EMBEDDING_DIM]);
        ingest(&conn, &input("file", "有向量文档", "正文内容"), Some(&embed)).unwrap();

        // 第一次调用：探测 1 次 → 缓存 ok → 无缺向量 → 0
        let n1 = rebuild_missing_vectors(&conn, Some(&embed)).unwrap();
        assert_eq!(n1, 0);

        // 第二次调用：命中 ok 缓存 → 跳过探测，直接 SELECT → 不应再调用嵌入闭包
        let calls = std::sync::Arc::new(AtomicUsize::new(0));
        let calls_in = calls.clone();
        let count_embed = move |_t: &str| {
            calls_in.fetch_add(1, Ordering::SeqCst);
            Ok(vec![0.1f32; EMBEDDING_DIM])
        };
        let n2 = rebuild_missing_vectors(&conn, Some(&count_embed)).unwrap();
        assert_eq!(n2, 0, "ok 缓存后应返回 0");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "ok 缓存后不应再触发嵌入探测（write-only 缓存修复）"
        );
        // 状态可读（P2-3：前端据此提示）
        assert_eq!(
            crate::ingest::embed_dim_probe_status(&conn).unwrap().as_deref(),
            Some("ok")
        );
    }

    #[test]
    fn embed_dim_probe_status_mismatch_readable() {
        // R5-P2-3：维度不符时状态可读为 mismatch（前端提示「语义检索降级关键词」）
        let conn = setup();
        assert_eq!(
            crate::ingest::embed_dim_probe_status(&conn).unwrap(),
            None,
            "未探测时为 None"
        );
        let bad = |_t: &str| Ok(vec![0.0f32; 8]);
        assert_eq!(rebuild_missing_vectors(&conn, Some(&bad)).unwrap(), 0);
        assert_eq!(
            crate::ingest::embed_dim_probe_status(&conn).unwrap().as_deref(),
            Some("mismatch")
        );
    }

    #[test]
    fn rebuild_missing_indexes_consumes_pending_marker() {
        // R5-P2-4：标记待补建的文档会被 rebuild_missing_indexes 补建并从标记移除
        let conn = setup();
        // 正常入库（FTS 已建）→ 不在 FTS 缺口内
        let doc_id = ingest(&conn, &input("file", "待补偿文档", "正文内容"), None).unwrap();
        // 基线：无缺口且无标记 → 不补建
        assert_eq!(rebuild_missing_indexes(&conn, None).unwrap(), 0, "FTS 无缺口时不补建");
        // 模拟字段事务已提交后重建失败 → 标记待补建
        mark_document_reindex_pending(&conn, doc_id, "模拟失败").unwrap();
        // 有 pending 标记时：即使 FTS 存在也会重建（幂等），成功后移除标记
        let n = rebuild_missing_indexes(&conn, None).unwrap();
        assert_eq!(n, 1, "pending 标记应触发一次补建");
        // 标记已移除 → 再次调用返回 0
        let n2 = rebuild_missing_indexes(&conn, None).unwrap();
        assert_eq!(n2, 0, "补建成功后标记应移除，不再重复");
    }

    #[test]
    fn ingest_failed_invalid_kind_rejected() {
        // P2-1：ingest_failed 与 ingest 主入口一致的 kind 白名单
        let conn = setup();
        let res = ingest_failed(&conn, &input("bad_kind", "标题", "内容"), "解析失败");
        assert!(matches!(res, Err(IngestError::InvalidKind(_))));
        let res2 = ingest_ocr_pending(&conn, &input("bad_kind", "标题", "内容"));
        assert!(matches!(res2, Err(IngestError::InvalidKind(_))));
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM document", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "非法 kind 时不得落库");
    }
}
