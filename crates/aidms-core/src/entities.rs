//! 实体 / 文档 / 标签 增删改查（全部参数化 ? 占位，防 SQL 注入）
//!
//! 所有标识符（表名/列名）走 [`crate::schema`] 常量或固定字符串；用户值一律经 `?` 绑定。
use std::collections::HashMap;

use rusqlite::{params, params_from_iter, Connection, Result, Row};

use crate::schema as S;

/// 公司主体行
#[derive(Debug, Clone, serde::Serialize)]
pub struct EntityRow {
    pub id: i64,
    pub name: String,
    pub credit_code: Option<String>,
    pub note: Option<String>,
    pub created_at: String,
}

/// 资料（文件或业务条目）行
#[derive(Debug, Clone, serde::Serialize)]
pub struct DocumentRow {
    pub id: i64,
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
    pub sync_status: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 标签行
#[derive(Debug, Clone, serde::Serialize)]
pub struct TagRow {
    pub id: i64,
    pub name: String,
}

/// 新建文档入参（created_at 由调用方提供 ISO 时间）
#[derive(Debug, Clone)]
pub struct NewDocument {
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

fn row_to_entity(r: &Row<'_>) -> Result<EntityRow> {
    Ok(EntityRow {
        id: r.get(0)?,
        name: r.get(1)?,
        credit_code: r.get(2)?,
        note: r.get(3)?,
        created_at: r.get(4)?,
    })
}

fn row_to_document(r: &Row<'_>) -> Result<DocumentRow> {
    Ok(DocumentRow {
        id: r.get(0)?,
        kind: r.get(1)?,
        title: r.get(2)?,
        doc_type: r.get(3)?,
        source: r.get(4)?,
        content_text: r.get(5)?,
        party: r.get(6)?,
        owner: r.get(7)?,
        date_field: r.get(8)?,
        note: r.get(9)?,
        fields: r.get(10)?,
        status: r.get(11)?,
        sync_status: r.get(12)?,
        created_at: r.get(13)?,
        updated_at: r.get(14)?,
    })
}

fn row_to_tag(r: &Row<'_>) -> Result<TagRow> {
    Ok(TagRow {
        id: r.get(0)?,
        name: r.get(1)?,
    })
}

// ---------------- entity ----------------

pub fn create_entity(
    conn: &Connection,
    name: &str,
    credit_code: Option<&str>,
    note: Option<&str>,
    created_at: &str,
) -> Result<i64> {
    conn.execute(
        &format!(
            "INSERT INTO {tbl}(name, credit_code, note, created_at) VALUES (?,?,?,?)",
            tbl = S::TABLE_ENTITY
        ),
        params![name, credit_code, note, created_at],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_entities(conn: &Connection) -> Result<Vec<EntityRow>> {
  let mut stmt = conn.prepare(&format!(
    "SELECT id, name, credit_code, note, created_at FROM {tbl} ORDER BY id",
    tbl = S::TABLE_ENTITY
  ))?;
  let rows = stmt.query_map([], row_to_entity)?;
  let out = rows.collect::<Result<Vec<_>>>()?;
  Ok(out)
}

/// 更新主体（仅改安全字段：名称 / 信用代码 / 备注）
pub fn update_entity(
  conn: &Connection,
  id: i64,
  name: &str,
  credit_code: Option<&str>,
  note: Option<&str>,
) -> Result<()> {
  conn.execute(
    &format!(
      "UPDATE {tbl} SET name=?, credit_code=?, note=? WHERE id=?",
      tbl = S::TABLE_ENTITY
    ),
    params![name, credit_code, note, id],
  )?;
  Ok(())
}

/// 统计归属某主体的资料数（删除前校验，避免悬挂引用）
pub fn count_entity_documents(conn: &Connection, id: i64) -> Result<i64> {
  let mut stmt = conn.prepare(&format!(
    "SELECT COUNT(*) FROM {tbl} WHERE entity_id=?",
    tbl = S::TABLE_DOCUMENT_ENTITY
  ))?;
  let n: i64 = stmt.query_row([id], |r| r.get(0))?;
  Ok(n)
}

/// 直接删除主体（不校验）。建议经 [`delete_entity_guard`] 调用以拦截有归属的主体。
pub fn delete_entity(conn: &Connection, id: i64) -> Result<()> {
  conn.execute(
    &format!("DELETE FROM {tbl} WHERE id=?", tbl = S::TABLE_ENTITY),
    [id],
  )?;
  Ok(())
}

/// 删除主体；若仍有资料归属（count>0）则拒绝，返回中文错误提示。
pub fn delete_entity_guard(conn: &Connection, id: i64) -> std::result::Result<(), String> {
  match count_entity_documents(conn, id) {
    Ok(n) if n > 0 => Err(format!(
      "该主体下仍有 {n} 份资料，请先将其移出该主体（或删除资料）后再删除主体"
    )),
    Ok(_) => delete_entity(conn, id).map_err(|e| e.to_string()),
    Err(e) => Err(e.to_string()),
  }
}

// ---------------- document ----------------

pub fn create_document(conn: &Connection, d: &NewDocument) -> Result<i64> {
    conn.execute(
        &format!(
            "INSERT INTO {tbl}(kind, title, type, source, content_text, party, owner, date_field, note, fields, status, created_at)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?)",
            tbl = S::TABLE_DOCUMENT
        ),
        params![
            d.kind,
            d.title,
            d.doc_type,
            d.source,
            d.content_text,
            d.party,
            d.owner,
            d.date_field,
            d.note,
            d.fields,
            d.status.clone().unwrap_or_else(|| "ok".to_string()),
            d.created_at
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_document(conn: &Connection, id: i64) -> Result<Option<DocumentRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT id, kind, title, type, source, content_text, party, owner, date_field, note, fields, status, sync_status, created_at, updated_at
         FROM {tbl} WHERE id=?",
        tbl = S::TABLE_DOCUMENT
    ))?;
    let mut rows = stmt.query_map([id], row_to_document)?;
    match rows.next() {
        Some(r) => Ok(Some(r?)),
        None => Ok(None),
    }
}

pub fn set_document_status(conn: &Connection, id: i64, status: &str) -> Result<()> {
    conn.execute(
        &format!(
            "UPDATE {tbl} SET status=?, updated_at=CURRENT_TIMESTAMP WHERE id=?",
            tbl = S::TABLE_DOCUMENT
        ),
        params![status, id],
    )?;
    Ok(())
}

/// 删除文档并级联清理索引（FTS5 两表按 rowid=doc.id；vec_items 按该 doc 的 chunk.id）。
/// P2-5：整个级联清理包在单个事务内——任一步失败整体回滚，不留半删状态。
pub fn delete_document(conn: &Connection, id: i64) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    let chunk_ids: Vec<i64> = {
        let mut stmt = tx.prepare("SELECT id FROM chunk WHERE document_id=?")?;
        let rows = stmt.query_map([id], |r| r.get(0))?;
        rows.collect::<Result<Vec<_>>>()?
    };
    for cid in chunk_ids {
        tx.execute(&format!("DELETE FROM {t} WHERE rowid=?", t = S::TABLE_VEC_ITEMS), [cid])?;
    }
    tx.execute(&format!("DELETE FROM {t} WHERE rowid=?", t = S::TABLE_DOCUMENT_FTS), [id])?;
    tx.execute(
        &format!("DELETE FROM {t} WHERE rowid=?", t = S::TABLE_DOCUMENT_FTS_TRIGRAM),
        [id],
    )?;
    tx.execute("DELETE FROM chunk WHERE document_id=?", [id])?;
    tx.execute(
        &format!("DELETE FROM {t} WHERE document_id=?", t = S::TABLE_DOCUMENT_ENTITY),
        [id],
    )?;
    tx.execute(
        &format!("DELETE FROM {t} WHERE document_id=?", t = S::TABLE_DOCUMENT_TAG),
        [id],
    )?;
    tx.execute(
        &format!("DELETE FROM {t} WHERE from_id=? OR to_id=?", t = S::TABLE_DOCUMENT_LINK),
        params![id, id],
    )?;
    // P2-1：级联清理自定义字段值（field_value 无外键约束，须显式删除，防孤儿行）
    tx.execute(
        &format!("DELETE FROM {t} WHERE document_id=?", t = S::TABLE_FIELD_VALUE),
        [id],
    )?;
    tx.execute(&format!("DELETE FROM {t} WHERE id=?", t = S::TABLE_DOCUMENT), [id])?;
    tx.commit()?;
    Ok(())
}

// ---------------- tag ----------------

pub fn create_tag(conn: &Connection, name: &str) -> Result<i64> {
    conn.execute(
        &format!("INSERT INTO {tbl}(name) VALUES (?)", tbl = S::TABLE_TAG),
        params![name],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_tags(conn: &Connection) -> Result<Vec<TagRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT id, name FROM {tbl} ORDER BY id",
        tbl = S::TABLE_TAG
    ))?;
    let rows = stmt.query_map([], row_to_tag)?;
    let out = rows.collect::<Result<Vec<_>>>()?;
    Ok(out)
}

pub fn add_document_tag(conn: &Connection, document_id: i64, tag_id: i64) -> Result<()> {
    conn.execute(
        &format!(
            "INSERT OR IGNORE INTO {tbl}(document_id, tag_id) VALUES (?,?)",
            tbl = S::TABLE_DOCUMENT_TAG
        ),
        params![document_id, tag_id],
    )?;
    Ok(())
}

pub fn remove_document_tag(conn: &Connection, document_id: i64, tag_id: i64) -> Result<()> {
    conn.execute(
        &format!(
            "DELETE FROM {tbl} WHERE document_id=? AND tag_id=?",
            tbl = S::TABLE_DOCUMENT_TAG
        ),
        params![document_id, tag_id],
    )?;
    Ok(())
}

/// 列出某文档的标签 id 列表（供前端打标 UI 展示当前已打标签，P2-3）
pub fn list_document_tags(conn: &Connection, document_id: i64) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT tag_id FROM {tbl} WHERE document_id=? ORDER BY tag_id",
        tbl = S::TABLE_DOCUMENT_TAG
    ))?;
    let rows = stmt.query_map([document_id], |r| r.get(0))?;
    rows.collect::<Result<Vec<_>>>()
}

// ---------------- document_entity (多对多) ----------------

pub fn link_entity(conn: &Connection, document_id: i64, entity_id: i64) -> Result<()> {
    conn.execute(
        &format!(
            "INSERT OR IGNORE INTO {tbl}(document_id, entity_id) VALUES (?,?)",
            tbl = S::TABLE_DOCUMENT_ENTITY
        ),
        params![document_id, entity_id],
    )?;
    Ok(())
}

pub fn unlink_entity(conn: &Connection, document_id: i64, entity_id: i64) -> Result<()> {
    conn.execute(
        &format!(
            "DELETE FROM {tbl} WHERE document_id=? AND entity_id=?",
            tbl = S::TABLE_DOCUMENT_ENTITY
        ),
        params![document_id, entity_id],
    )?;
    Ok(())
}

// ---------------- document_link (业务条目 ↔ 文件) ----------------

pub fn create_link(conn: &Connection, from_id: i64, to_id: i64, kind: &str) -> Result<()> {
    conn.execute(
        &format!(
            "INSERT OR IGNORE INTO {tbl}(from_id, to_id, kind) VALUES (?,?,?)",
            tbl = S::TABLE_DOCUMENT_LINK
        ),
        params![from_id, to_id, kind],
    )?;
    Ok(())
}

/// 单条关联（from→to）
#[derive(Debug, Clone, serde::Serialize)]
pub struct DocLink {
    pub id: i64,
    pub kind: String,
    /// "out" = 本文档为 from；"in" = 本文档为 to
    pub direction: String,
}

/// 列出与某文档关联的所有文档（双向：from↔to）
pub fn list_links(conn: &Connection, doc_id: i64) -> Result<Vec<DocLink>> {
    let mut out: Vec<DocLink> = Vec::new();
    {
        let mut stmt = conn.prepare(&format!(
            "SELECT to_id, kind FROM {tbl} WHERE from_id=?",
            tbl = S::TABLE_DOCUMENT_LINK
        ))?;
        let rows = stmt.query_map([doc_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        for r in rows {
            let (to_id, kind) = r?;
            out.push(DocLink { id: to_id, kind, direction: "out".into() });
        }
    }
    {
        let mut stmt = conn.prepare(&format!(
            "SELECT from_id, kind FROM {tbl} WHERE to_id=?",
            tbl = S::TABLE_DOCUMENT_LINK
        ))?;
        let rows = stmt.query_map([doc_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        for r in rows {
            let (from_id, kind) = r?;
            out.push(DocLink { id: from_id, kind, direction: "in".into() });
        }
    }
    Ok(out)
}

/// 删除一条关联（from→to，方向固定）
pub fn delete_link(conn: &Connection, from_id: i64, to_id: i64) -> Result<()> {
    conn.execute(
        &format!(
            "DELETE FROM {tbl} WHERE from_id=? AND to_id=?",
            tbl = S::TABLE_DOCUMENT_LINK
        ),
        params![from_id, to_id],
    )?;
    Ok(())
}

// ---------------- 三维筛选 ----------------

/// 三维正交筛选：主体(entity) × 类型(type) × 标签(tag) + 高级筛选（R15）
#[derive(Debug, Default, Clone)]
pub struct DocumentFilter {
    pub entity_id: Option<i64>,
    pub doc_type: Option<String>,
    pub tag_id: Option<i64>,
    /// 高级筛选：多主体（与 `entity_id` 取并集，用于 R15）
    pub entity_ids: Vec<i64>,
    /// 高级筛选（R15）：负责人（精确匹配，空白视为不筛选）
    pub owner: Option<String>,
    /// 高级筛选（R15）：业务日期起始（YYYY-MM-DD，含当天）
    pub date_from: Option<String>,
    /// 高级筛选（R15）：业务日期截止（YYYY-MM-DD，含当天）
    pub date_to: Option<String>,
    /// 高级筛选（R15）：来源（LIKE 模糊匹配，`%`/`_`/`\` 已转义防通配符注入）
    pub source: Option<String>,
}

/// 把用户输入转义为 LIKE 模糊匹配模式（`\` `%` `_` 转义，防通配符注入）。
/// SQL 侧使用 `LIKE ? ESCAPE '\'`，此处生成 `%escaped%`。
fn like_pattern(v: &str) -> String {
    let mut s = String::with_capacity(v.len() + 2);
    s.push('%');
    for c in v.chars() {
        if c == '\\' || c == '%' || c == '_' {
            s.push('\\');
        }
        s.push(c);
    }
    s.push('%');
    s
}

pub fn list_documents(conn: &Connection, f: &DocumentFilter) -> Result<Vec<DocumentRow>> {
    let mut sql = format!(
        "SELECT d.id, d.kind, d.title, d.type, d.source, d.content_text, d.party, d.owner,
                d.date_field, d.note, d.fields, d.status, d.sync_status, d.created_at, d.updated_at
         FROM {tbl} d",
        tbl = S::TABLE_DOCUMENT
    );
    let mut conds: Vec<String> = Vec::new();
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    // 多主体（顶栏单选 entity_id + 高级多选 entity_ids，取并集）
    let mut ent: Vec<i64> = Vec::new();
    if let Some(e) = f.entity_id {
        ent.push(e);
    }
    ent.extend(f.entity_ids.iter().cloned());
    if !ent.is_empty() {
        let ph = std::iter::repeat("?").take(ent.len()).collect::<Vec<_>>().join(",");
        conds.push(format!(
            "d.id IN (SELECT document_id FROM {de} WHERE entity_id IN ({ph}))",
            de = S::TABLE_DOCUMENT_ENTITY
        ));
        for id in &ent {
            args.push(Box::new(*id));
        }
    }
    if let Some(t) = &f.doc_type {
        conds.push("d.type = ?".to_string());
        args.push(Box::new(t.clone()));
    }
    if f.tag_id.is_some() {
        conds.push(format!(
            "d.id IN (SELECT document_id FROM {dt} WHERE tag_id = ?)",
            dt = S::TABLE_DOCUMENT_TAG
        ));
        args.push(Box::new(f.tag_id.unwrap()));
    }

    // ---- R15 高级筛选：负责人 / 日期范围 / 来源 ----
    if let Some(o) = &f.owner {
        let o = o.trim();
        if !o.is_empty() {
            conds.push("d.owner = ?".to_string());
            args.push(Box::new(o.to_string()));
        }
    }
    if let Some(df) = &f.date_from {
        let df = df.trim();
        if !df.is_empty() {
            // date() 归一化两侧（兼容完整时间戳 / 纯日期），字符串比较即日历序
            conds.push("date(d.date_field) >= date(?)".to_string());
            args.push(Box::new(df.to_string()));
        }
    }
    if let Some(dt) = &f.date_to {
        let dt = dt.trim();
        if !dt.is_empty() {
            conds.push("date(d.date_field) <= date(?)".to_string());
            args.push(Box::new(dt.to_string()));
        }
    }
    if let Some(s) = &f.source {
        let s = s.trim();
        if !s.is_empty() {
            conds.push("d.source LIKE ? ESCAPE '\\'".to_string());
            args.push(Box::new(like_pattern(s)));
        }
    }

    if !conds.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&conds.join(" AND "));
    }
    sql.push_str(" ORDER BY d.id");

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(args), row_to_document)?;
    let out = rows.collect::<Result<Vec<_>>>()?;
    Ok(out)
}

/// 资料 + 其归属主体 id 列表（用于「未归类主体」标示：entity_ids 为空即未归类）。
/// `#[serde(flatten)]` 使序列化扁平为 `DocumentRow 字段 + entity_ids`，与前端
/// `DocumentWithEntities extends DocumentRow` 契约一致（Library 直接读 d.title/d.entity_ids）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct DocumentWithEntities {
    #[serde(flatten)]
    pub doc: DocumentRow,
    pub entity_ids: Vec<i64>,
}

pub fn list_documents_with_entities(
    conn: &Connection,
    f: &DocumentFilter,
) -> Result<Vec<DocumentWithEntities>> {
    let docs = list_documents(conn, f)?;
    if docs.is_empty() {
        return Ok(Vec::new());
    }
    // P2-10（N+1 优化）：一次 IN 查询取全部归属主体映射，替代「每文档一条 SELECT」
    let ids: Vec<i64> = docs.iter().map(|d| d.id).collect();
    let ph = std::iter::repeat("?")
        .take(ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let mut doc_entities: HashMap<i64, Vec<i64>> = HashMap::new();
    {
        let sql = format!(
            "SELECT document_id, entity_id FROM {tbl} WHERE document_id IN ({ph})",
            tbl = S::TABLE_DOCUMENT_ENTITY
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(ids.iter()), |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
        })?;
        for r in rows {
            let (did, eid) = r?;
            doc_entities.entry(did).or_default().push(eid);
        }
    }
    Ok(docs
        .into_iter()
        .map(|d| DocumentWithEntities {
            entity_ids: doc_entities.remove(&d.id).unwrap_or_default(),
            doc: d,
        })
        .collect())
}
