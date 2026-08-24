//! 融合检索（阶段 4）：FTS5 关键词 + sqlite-vec 向量 → RRF 融合
//!
//! 契约（开发计划阶段 4 / 技术设计 §4）：
//! - FTS5 路：jieba 主表（Search 模式切词）AND/OR 主干 + trigram 子串兜底，合并为一条 FTS 路 rank。
//! - vec0 路：查询向量 KNN；per-entity KNN（仅在指定主体 chunk 子集内检索）；
//!   同一文档取「最相关 chunk 的 min 距离」作为该文档距离（非 max/sum，避免排序失真），
//!   按距离升序排 rank。
//! - 两路各自按得分排 rank 后算 RRF `score = Σ 1/(k+rank)`，相加融合（量纲统一为 rank 序）。
//! - 片段高亮 `<mark>`：直接对 `document.content_text` 生成（绕开 FTS 文本层 token 化文本不便展示的问题），
//!   前端经 `DOMPurify(ALLOWED_TAGS:['mark'])` 净化后再注入（技术设计 §10）。
use std::collections::{HashMap, HashSet};

use rusqlite::{params, params_from_iter, Connection, Result};
use serde::Serialize;

use crate::schema as S;
use crate::tokenize;

/// RRF 常数（经验值 60）
const RRF_K: f64 = 60.0;
/// 向量检索取 Top-K（每主体语料小，放大 K 防过滤后过少，开发计划阶段 4）
const VEC_K: usize = 50;
/// 片段最大字符数
const SNIPPET_MAX: usize = 160;

/// 检索模式（语义/问答需向量；未配 LLM 则语义不可用）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SearchMode {
    /// 仅全文（FTS5，始终可用）
    Keyword,
    /// 仅语义（vec0，需 query_vec）
    Semantic,
    /// 融合（FTS5 + vec0）
    Hybrid,
}

impl Default for SearchMode {
    fn default() -> Self {
        SearchMode::Hybrid
    }
}

/// 检索请求
#[derive(Debug, Clone, Default)]
pub struct SearchRequest {
    pub query: String,
    /// 查询向量（稠密 f32）。语义/融合模式必需；缺省则降级为全文。
    pub query_vec: Option<Vec<f32>>,
    pub mode: SearchMode,
    /// 主体约束（多主体）：None=全部；Some=仅这些主体。
    /// 影响 vec0 子集与 FTS 候选过滤。
    pub entity_ids: Option<Vec<i64>>,
    /// 类型约束（轻量结果内筛选，非全局切换器）
    pub doc_types: Option<Vec<String>>,
    /// 标签约束
    pub tag_ids: Option<Vec<i64>>,
    /// 返回条数
    pub limit: usize,
}

/// 检索结果命中
#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub doc_id: i64,
    pub title: String,
    /// 片段 HTML（含 `<mark>` 高亮），前端须净化后注入
    pub snippet: String,
    /// 融合 RRF 得分（越大越相关）
    pub score: f64,
    /// 主要由向量语义召回（用于高亮着色区分）
    pub semantic: bool,
    /// 归属主体 id 列表（为空即「未归类主体」，前端据此标示）
    pub entity_ids: Vec<i64>,
}

/// 计算指定主体相关的 chunk 子集（per-entity KNN 约束）
fn subset_chunk_ids(conn: &Connection, entity_ids: &Option<Vec<i64>>) -> Result<Vec<i64>> {
    let Some(ids) = entity_ids else {
        return Ok(Vec::new());
    };
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders: Vec<String> = ids.iter().map(|_| "?".to_string()).collect();
    let sql = format!(
        "SELECT c.id FROM chunk c
         JOIN document d ON d.id = c.document_id
         JOIN document_entity de ON de.document_id = d.id
         WHERE de.entity_id IN ({})",
        placeholders.join(",")
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params_from_iter(ids.iter().map(|v| *v as i64)), |r| r.get(0))?
        .collect::<Result<Vec<i64>>>()?;
    Ok(rows)
}

/// 过滤后的允许文档集合；无任何约束返回 None（表示不限制）
fn allowed_doc_ids(
    conn: &Connection,
    entity_ids: &Option<Vec<i64>>,
    doc_types: &Option<Vec<String>>,
    tag_ids: &Option<Vec<i64>>,
) -> Result<Option<HashSet<i64>>> {
    let has_entity = entity_ids.as_ref().map(|v| !v.is_empty()).unwrap_or(false);
    let has_type = doc_types.as_ref().map(|v| !v.is_empty()).unwrap_or(false);
    let has_tag = tag_ids.as_ref().map(|v| !v.is_empty()).unwrap_or(false);
    if !has_entity && !has_type && !has_tag {
        return Ok(None);
    }

    let mut sql = String::from("SELECT DISTINCT d.id FROM document d");
    let mut conds: Vec<String> = Vec::new();
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if has_tag {
        sql.push_str(" JOIN document_tag dt ON dt.document_id = d.id");
        let tags = tag_ids.as_ref().unwrap();
        let ph: Vec<String> = tags.iter().map(|_| "?".to_string()).collect();
        conds.push(format!("dt.tag_id IN ({})", ph.join(",")));
        for t in tags {
            args.push(Box::new(*t));
        }
    }
    if has_entity {
        sql.push_str(" JOIN document_entity de ON de.document_id = d.id");
        let ents = entity_ids.as_ref().unwrap();
        let ph: Vec<String> = ents.iter().map(|_| "?".to_string()).collect();
        conds.push(format!("de.entity_id IN ({})", ph.join(",")));
        for e in ents {
            args.push(Box::new(*e));
        }
    }
    if has_type {
        let types = doc_types.as_ref().unwrap();
        let ph: Vec<String> = types.iter().map(|_| "?".to_string()).collect();
        conds.push(format!("d.type IN ({})", ph.join(",")));
        for t in types {
            args.push(Box::new(t.clone()));
        }
    }
    if !conds.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&conds.join(" AND "));
    }

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params_from_iter(args), |r| r.get(0))?
        .collect::<Result<Vec<i64>>>()?;
    Ok(Some(rows.into_iter().collect()))
}

/// FTS5 路：返回有序 (doc_id) 列表（rank 序，首元素最相关），按 jieba AND → OR → trigram 兜底 优先级排列
fn fts5_path(
    conn: &Connection,
    tokenized: &str,
    or_query: &str,
    trigram_query: &str,
) -> Result<Vec<i64>> {
    let mut ranked: Vec<i64> = Vec::new();
    let mut seen: HashSet<i64> = HashSet::new();

    // 1) jieba 主表 AND（精确短语，bm25 升序，越负越相关）
    if !tokenized.trim().is_empty() {
        let sql = format!(
            "SELECT rowid FROM {t} WHERE {t} MATCH ? ORDER BY bm25({t}) ASC LIMIT 200",
            t = S::TABLE_DOCUMENT_FTS
        );
        let mut stmt = conn.prepare(&sql)?;
        for r in stmt.query_map([tokenized], |row| row.get::<_, i64>(0))? {
            let id = r?;
            if seen.insert(id) {
                ranked.push(id);
            }
        }
    }

    // 2) jieba 主表 OR（扩大召回，缀在 AND 之后）
    if !or_query.trim().is_empty() {
        let sql = format!(
            "SELECT rowid FROM {t} WHERE {t} MATCH ? ORDER BY bm25({t}) ASC LIMIT 200",
            t = S::TABLE_DOCUMENT_FTS
        );
        let mut stmt = conn.prepare(&sql)?;
        for r in stmt.query_map([or_query], |row| row.get::<_, i64>(0))? {
            let id = r?;
            if seen.insert(id) {
                ranked.push(id);
            }
        }
    }

    // 3) trigram 子串兜底（≥3 字包含匹配，无短语排序，缀在最后）
    if !trigram_query.trim().is_empty() {
        let sql = format!(
            "SELECT rowid FROM {t} WHERE {t} MATCH ? LIMIT 200",
            t = S::TABLE_DOCUMENT_FTS_TRIGRAM
        );
        if let Ok(mut stmt) = conn.prepare(&sql) {
            if let Ok(rows) = stmt.query_map([trigram_query], |row| row.get::<_, i64>(0)) {
                for r in rows {
                    if let Ok(id) = r {
                        if seen.insert(id) {
                            ranked.push(id);
                        }
                    }
                }
            }
        }
    }

    Ok(ranked)
}

/// vec0 路：返回有序 (doc_id) 列表（rank 序，首元素最相关：min 距离最小）
fn vec_path(
    conn: &Connection,
    query_vec: &[f32],
    subset: &[i64],
) -> Result<Vec<i64>> {
    let knn = crate::index::knn(conn, query_vec, VEC_K, Some(subset))?;
    if knn.is_empty() {
        return Ok(Vec::new());
    }

    // chunk_id -> doc_id
    let chunk_ids: Vec<i64> = knn.iter().map(|(cid, _)| *cid).collect();
    let ph: Vec<String> = chunk_ids.iter().map(|_| "?".to_string()).collect();
    let sql = format!(
        "SELECT id, document_id FROM chunk WHERE id IN ({})",
        ph.join(",")
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params_from_iter(chunk_ids.iter().copied()), |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
        })?
        .collect::<Result<Vec<(i64, i64)>>>()?;

    let chunk_to_doc: HashMap<i64, i64> = rows.into_iter().collect();

    // 每文档取最相关 chunk 的 min 距离
    let mut doc_dist: HashMap<i64, f64> = HashMap::new();
    for (cid, dist) in &knn {
        if let Some(doc) = chunk_to_doc.get(cid) {
            let cur = doc_dist.entry(*doc).or_insert(f64::INFINITY);
            if *dist < *cur {
                *cur = *dist;
            }
        }
    }

    let mut docs: Vec<(i64, f64)> = doc_dist.into_iter().collect();
    docs.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    Ok(docs.into_iter().map(|(doc, _)| doc).collect())
}

/// 对有序 doc 列表算 RRF 得分表
fn rrf_scores(ranked: &[i64]) -> HashMap<i64, f64> {
    let mut map = HashMap::new();
    for (i, id) in ranked.iter().enumerate() {
        let rank = (i + 1) as f64;
        *map.entry(*id).or_insert(0.0) += 1.0 / (RRF_K + rank);
    }
    map
}

/// 生成片段：在 content 中定位首个命中词，取窗口并包裹 `<mark>`
fn make_snippet(content: &str, terms: &[String]) -> String {
    let chars: Vec<char> = content.chars().collect();
    if chars.is_empty() {
        return String::new();
    }
    let lower: Vec<char> = content.to_lowercase().chars().collect();

    // 计算命中位置（取最早出现的词）
    let mut best: Option<usize> = None;
    let mut term_lcs: Vec<(String, Vec<char>)> = Vec::new();
    for t in terms {
        let lc: Vec<char> = t.to_lowercase().chars().collect();
        if lc.is_empty() {
            continue;
        }
        term_lcs.push((t.clone(), lc));
        if let Some(pos) = find_subslice(&lower, &term_lcs.last().unwrap().1) {
            best = Some(match best {
                Some(b) => b.min(pos),
                None => pos,
            });
        }
    }

    let start = best.unwrap_or(0);
    let s = start.saturating_sub(SNIPPET_MAX / 4);
    let e = (start + SNIPPET_MAX).min(chars.len());
    let slice: String = chars[s..e].iter().collect();
    let esc = escape_html(&slice);

    let mut out = esc;
    for (t, _) in &term_lcs {
        let esc_term = escape_html(t);
        out = highlight_in_escaped(&out, &esc_term);
    }

    let prefix = if s > 0 { "…" } else { "" };
    let suffix = if e < chars.len() { "…" } else { "" };
    format!("{}{}{}", prefix, out, suffix)
}

fn find_subslice(hay: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// 在已转义文本中大小写不敏感地包裹 `<mark>`（term 也为已转义形式）
/// 采用字符索引，避免多字节中文切到字符中间。
fn highlight_in_escaped(s: &str, term: &str) -> String {
    if term.is_empty() {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let tchars: Vec<char> = term.chars().collect();
    if tchars.is_empty() {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + tchars.len() * 7);
    let mut i = 0;
    while i < chars.len() {
        if i + tchars.len() <= chars.len() && chars[i..i + tchars.len()] == tchars[..] {
            out.push_str("<mark>");
            out.extend(chars[i..i + tchars.len()].iter());
            out.push_str("</mark>");
            i += tchars.len();
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// 融合检索主入口
pub fn search(conn: &Connection, req: &SearchRequest) -> Result<Vec<SearchHit>> {
    let limit = if req.limit == 0 { 20 } else { req.limit };
    let raw = req.query.trim().to_string();

    // 切词与查询串构造
    let tokens: Vec<String> = tokenize::cut_search(&raw)
        .split_whitespace()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let tokenized = tokens.join(" ");
    let or_query = if tokens.len() > 1 {
        tokens.join(" OR ")
    } else {
        String::new()
    };
    // trigram：原生查询串（≥3 字），双引号包裹；去除内部引号避免语法破坏
    let trigram_query = if raw.chars().count() >= 3 {
        format!("\"{}\"", raw.replace('"', " "))
    } else {
        String::new()
    };

    // 是否走向量路
    let mut use_vec = matches!(req.mode, SearchMode::Semantic | SearchMode::Hybrid);
    let query_vec: Option<Vec<f32>> = req.query_vec.clone();
    if use_vec && query_vec.is_none() {
        // 未配向量：语义/融合降级为全文（不报错，保证传统检索可用）
        use_vec = false;
    }
    // P1-2：查询侧维度守卫——与写侧（index::write_embedding）对称。
    // 用户换 512/768/1536 维模型后 query_vec 与 vec0 固定 1024 维不符，
    // 若直通 knn 会报 "Dimension mismatch" 使整个搜索报错；此处降级全文并提示（不报错）。
    if use_vec {
        if let Some(v) = &query_vec {
            if v.len() != crate::index::EMBEDDING_DIM {
                eprintln!(
                    "[search] 查询向量维度 {} 与 vec0 固定维度 {} 不符，降级为全文模式",
                    v.len(),
                    crate::index::EMBEDDING_DIM
                );
                use_vec = false;
            }
        }
    }

    // FTS 路 rank
    let fts_ranked = fts5_path(conn, &tokenized, &or_query, &trigram_query)?;
    let fts_scores = rrf_scores(&fts_ranked);

    // vec 路 rank
    let mut vec_scores: HashMap<i64, f64> = HashMap::new();
    if use_vec {
        let subset = subset_chunk_ids(conn, &req.entity_ids)?;
        let vec_ranked = vec_path(conn, query_vec.as_ref().unwrap(), &subset)?;
        vec_scores = rrf_scores(&vec_ranked);
    }

    // 候选并集
    let mut candidates: HashSet<i64> = HashSet::new();
    candidates.extend(fts_scores.keys().copied());
    candidates.extend(vec_scores.keys().copied());

    // 过滤（entity/type/tag）
    let allowed = allowed_doc_ids(conn, &req.entity_ids, &req.doc_types, &req.tag_ids)?;
    if let Some(allowed) = allowed {
        candidates.retain(|id| allowed.contains(id));
    }

    // 融合得分
    let mut fused: Vec<(i64, f64)> = candidates
        .into_iter()
        .map(|id| {
            let s = fts_scores.get(&id).copied().unwrap_or(0.0)
                + vec_scores.get(&id).copied().unwrap_or(0.0);
            (id, s)
        })
        .collect();
    fused.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    fused.truncate(limit);

    // 组装命中（取标题 + 原文生成片段）
    let mut hits = Vec::with_capacity(fused.len());
    for (doc_id, score) in fused {
        let row = conn
            .query_row(
                &format!(
                    "SELECT title, content_text FROM {t} WHERE id = ?",
                    t = S::TABLE_DOCUMENT
                ),
                params![doc_id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)),
            )
            .ok();
        let (title, content) = match row {
            Some((t, c)) => (t, c.unwrap_or_default()),
            None => continue,
        };
        // 高亮词：jieba 切词结果（去除与标题重叠的噪声），并补入原生查询
        let mut hl_terms = tokens.clone();
        if raw.chars().count() >= 2 && !hl_terms.iter().any(|t| t == &raw) {
            hl_terms.push(raw.clone());
        }
        let snippet = make_snippet(&content, &hl_terms);
        let semantic = use_vec && vec_scores.get(&doc_id).copied().unwrap_or(0.0) > 0.0
            && fts_scores.get(&doc_id).copied().unwrap_or(0.0) == 0.0;
        // 归属主体 id（用于「未归类主体」标示）
        let mut stmt = conn.prepare(&format!(
            "SELECT entity_id FROM {t} WHERE document_id=?",
            t = S::TABLE_DOCUMENT_ENTITY
        ))?;
        let entity_ids: Vec<i64> = stmt
            .query_map([doc_id], |r| r.get(0))?
            .collect::<Result<Vec<_>>>()?;
        hits.push(SearchHit {
            doc_id,
            title,
            snippet,
            score,
            semantic,
            entity_ids,
        });
    }

    Ok(hits)
}
