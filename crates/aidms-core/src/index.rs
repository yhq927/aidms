//! 索引写入：FTS5（jieba 主表 + trigram 兜底）+ 切块 + vec0 向量（嵌入不可达时跳过）
//!
//! 两路 FTS 均以 `document.id` 为 rowid；vec0 以 `chunk.id` 为 rowid（与技术设计 §3 一致）。
use rusqlite::{params, Connection, Result};

use crate::schema as S;
use crate::tokenize;

/// vec0 固定维度（与 0001_init.sql `embedding float[1024]` 保持一致，BGE-M3 稠密 1024 维）
pub const EMBEDDING_DIM: usize = 1024;

/// f32 向量 -> sqlite-vec blob（小端）
pub fn to_blob(v: &[f32]) -> Vec<u8> {
    let mut b = Vec::with_capacity(v.len() * 4);
    for &x in v {
        b.extend_from_slice(&x.to_le_bytes());
    }
    b
}

/// 把文本切块（相邻块重叠，记原文偏移，供 RAG 回贴）
///
/// - `window`：块字符窗口（约 200–400）
/// - `step`：步长（window - 重叠，重叠 10–15%）
pub fn chunk_text(text: &str, window: usize, step: usize) -> Vec<(usize, usize, String)> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut start = 0;
    loop {
        let end = (start + window).min(chars.len());
        let slice: String = chars[start..end].iter().collect();
        out.push((start, end, slice));
        if end >= chars.len() {
            break;
        }
        start += step;
    }
    out
}

/// 读取某文档的全部自定义字段值，拼成空格分隔串（供 FTS5 索引，R12 可检索）
fn read_field_values(conn: &Connection, doc_id: i64) -> Result<String> {
    let mut stmt = conn.prepare(&format!(
        "SELECT value FROM {t} WHERE document_id=?",
        t = S::TABLE_FIELD_VALUE
    ))?;
    let vals: Vec<String> = stmt
        .query_map([doc_id], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>>>()?;
    Ok(vals.join(" "))
}

/// 写两路 FTS5（rowid = document.id）
/// - `document_fts`：jieba 预切词后空格拼接（精确词/短语命中）
/// - `document_fts_trigram`：原生原文（不经 jieba，3 字滑窗子串/包含兜底）
/// 自定义字段值（field_value）自动并入 content，使其可被全文检索（R12）。
pub fn index_document_fts(
    conn: &Connection,
    doc_id: i64,
    title: &str,
    content: &str,
) -> Result<()> {
    let fields = read_field_values(conn, doc_id)?;
    let combined = if fields.is_empty() {
        content.to_string()
    } else {
        format!("{content} {fields}")
    };
    let t_title = tokenize::cut_search(title);
    let t_content = tokenize::cut_search(&combined);
    conn.execute(
        &format!(
            "INSERT INTO {t}(rowid, title, content) VALUES (?,?,?)",
            t = S::TABLE_DOCUMENT_FTS
        ),
        params![doc_id, t_title, t_content],
    )?;
    conn.execute(
        &format!(
            "INSERT INTO {t}(rowid, title, content) VALUES (?,?,?)",
            t = S::TABLE_DOCUMENT_FTS_TRIGRAM
        ),
        params![doc_id, title, combined],
    )?;
    Ok(())
}

/// 重建单文档的 FTS5 索引（R12：自定义字段增改后调用）。
/// 从 `document` 读回 title/content_text + field_value，删除旧两路 FTS5 行后重写。
/// 向量重建需嵌入模型，由调用方在「已配置嵌入」时另行触发，此处仅保证 FTS5 必重建。
pub fn rebuild_document_fts(conn: &Connection, doc_id: i64) -> Result<()> {
    let (title, content): (String, Option<String>) = conn.query_row(
        &format!(
            "SELECT title, content_text FROM {t} WHERE id=?",
            t = S::TABLE_DOCUMENT
        ),
        [doc_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    conn.execute(
        &format!("DELETE FROM {t} WHERE rowid=?", t = S::TABLE_DOCUMENT_FTS),
        [doc_id],
    )?;
    conn.execute(
        &format!("DELETE FROM {t} WHERE rowid=?", t = S::TABLE_DOCUMENT_FTS_TRIGRAM),
        [doc_id],
    )?;
    index_document_fts(conn, doc_id, &title, content.as_deref().unwrap_or(""))
}

/// 写入切块（返回各 chunk 的 rowid，供 vec0 写入）
pub fn write_chunks(
    conn: &Connection,
    doc_id: i64,
    chunks: &[(usize, usize, String)],
) -> Result<Vec<i64>> {
    let mut ids = Vec::with_capacity(chunks.len());
    for (i, (s, e, text)) in chunks.iter().enumerate() {
        conn.execute(
            "INSERT INTO chunk(document_id, seq, start_offset, end_offset, text) VALUES (?,?,?,?,?)",
            params![doc_id, i as i64, *s as i64, *e as i64, text],
        )?;
        ids.push(conn.last_insert_rowid());
    }
    Ok(ids)
}

/// 写单块向量（rowid = chunk.id）。嵌入不可达时调用方不调用本函数即可（仅保留 FTS5）。
///
/// P1-1 维度守卫：写入前校验 `vec.len() == EMBEDDING_DIM`（vec0 固定 `float[1024]`）。
/// 维度不符视为「嵌入不可用」（模型配置错误/版本不匹配）——返回 Ok 但不写 vec，
/// **不抛错、不产生半成品**：文档保持 FTS5 可搜，向量缺口由 `rebuild_missing_vectors`
/// 在模型恢复后幂等补偿（其内部 `reindex` 会先清旧 chunk/vec 再重建）。
pub fn write_embedding(conn: &Connection, chunk_id: i64, vec: &[f32]) -> Result<()> {
    if vec.len() != EMBEDDING_DIM {
        // 日志脱敏：仅记录维度，不输出向量/文本内容
        eprintln!(
            "[index] 嵌入维度 {} 与 vec0 固定维度 {} 不符，跳过该块向量写入（文档保持 FTS 可搜）",
            vec.len(),
            EMBEDDING_DIM
        );
        return Ok(());
    }
    conn.execute(
        &format!(
            "INSERT INTO {t}(rowid, embedding) VALUES (?,?)",
            t = S::TABLE_VEC_ITEMS
        ),
        params![chunk_id, to_blob(vec)],
    )?;
    Ok(())
}

/// vec0 cosine KNN（可选 per-entity 子集约束）
///
/// - `query`：查询向量（f32）
/// - `k`：取 Top-K
/// - `subset_chunk_ids`：仅在该 chunk 子集内检索（per-entity KNN，见开发计划阶段 4）
pub fn knn(
    conn: &Connection,
    query: &[f32],
    k: usize,
    subset_chunk_ids: Option<&[i64]>,
) -> Result<Vec<(i64, f64)>> {
    let mut sql = format!(
        "SELECT rowid, distance FROM {t} WHERE embedding MATCH ? AND k = ?",
        t = S::TABLE_VEC_ITEMS
    );
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(to_blob(query)), Box::new(k as i64)];
    if let Some(ids) = subset_chunk_ids {
        if !ids.is_empty() {
            let placeholders: Vec<String> = ids.iter().map(|_| "?".to_string()).collect();
            sql.push_str(&format!(" AND rowid IN ({})", placeholders.join(",")));
            for id in ids {
                args.push(Box::new(*id));
            }
        }
    }
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(args), |r| {
        Ok((r.get(0)?, r.get(1)?))
    })?;
    rows.collect::<Result<Vec<_>>>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn setup() -> Connection {
        db::open(":memory:").unwrap()
    }

    #[test]
    fn write_embedding_dim_mismatch_skipped_no_error() {
        // P1-1：维度不符 → 返回 Ok 且不写库（不抛错、不产生半成品）
        let conn = setup();
        conn.execute("INSERT INTO chunk(document_id, seq, text) VALUES (1,0,'测试')", [])
            .unwrap();
        let cid = conn.last_insert_rowid();
        // 错误维度（如 8 维 / 512 维）
        assert!(write_embedding(&conn, cid, &[0.1f32; 8]).is_ok());
        assert!(write_embedding(&conn, cid, &[0.1f32; 512]).is_ok());
        // vec_items 不应有任何行
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM vec_items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "维度不符时不得写入 vec_items");
    }

    #[test]
    fn write_embedding_correct_dim_written() {
        // P1-1：维度正确（1024）→ 正常写库
        let conn = setup();
        conn.execute("INSERT INTO chunk(document_id, seq, text) VALUES (1,0,'测试')", [])
            .unwrap();
        let cid = conn.last_insert_rowid();
        let v = vec![0.25f32; EMBEDDING_DIM];
        assert!(write_embedding(&conn, cid, &v).is_ok());
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM vec_items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "维度正确时应写入一条 vec_items");
    }
}
