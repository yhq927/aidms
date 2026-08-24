//! 业务预置字段（R2）/ 自定义字段（R12）：field_def 预置 + field_value 读写。
//!
//! 基础字段（名称/相对方/负责人/日期/备注/归属主体）落在 `document` 主表列；
//! 仅「扩展字段」存 `field_value`，并同步进 FTS5 可检索（R12：改后必重建 FTS5）。
use rusqlite::{params, Connection, Result, Row};
use std::collections::HashMap;

use crate::schema as S;

/// 字段定义（预置或用户自定义）
#[derive(Debug, Clone, serde::Serialize)]
pub struct FieldDef {
    pub id: i64,
    pub biz_type: String,
    pub field_key: String,
    pub field_label: String,
    pub field_type: String,
    pub options: Option<String>,
    pub is_preset: bool,
}

fn row_to_field_def(r: &Row<'_>) -> Result<FieldDef> {
    Ok(FieldDef {
        id: r.get(0)?,
        biz_type: r.get(1)?,
        field_key: r.get(2)?,
        field_label: r.get(3)?,
        field_type: r.get(4)?,
        options: r.get(5)?,
        is_preset: r.get::<_, i64>(6)? != 0,
    })
}

/// 企业通用预置字段（按业务类型），仅扩展字段，不含主表基础列
const PRESETS: &[(&str, &str, &str, &str, Option<&str>)] = &[
    // (biz_type, field_key, field_label, field_type, options_json)
    ("客户", "industry", "行业", "text", None),
    ("客户", "contact", "联系人", "text", None),
    ("客户", "phone", "联系电话", "text", None),
    ("客户", "level", "客户等级", "select", Some("[\"A\",\"B\",\"C\"]")),
    ("合同", "amount", "合同金额", "number", None),
    ("合同", "effective_date", "生效日期", "date", None),
    ("合同", "expire_date", "到期日期", "date", None),
    ("项目", "period", "项目周期", "text", None),
    ("项目", "budget", "预算", "number", None),
    ("供应商", "category", "供应品类", "text", None),
    ("供应商", "settlement", "结算方式", "select", Some("[\"月结\",\"现结\",\"预收\"]")),
    ("资质", "valid_until", "有效期至", "date", None),
    ("资质", "issuer", "发证机构", "text", None),
];

/// 预置字段定义（幂等：仅当尚无预置定义时写入）
pub fn seed_preset_field_defs(conn: &Connection) -> Result<()> {
    let existing: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM {t} WHERE is_preset=1", t = S::TABLE_FIELD_DEF),
        [],
        |r| r.get(0),
    )?;
    if existing > 0 {
        return Ok(());
    }
    for (biz, key, label, ftype, opts) in PRESETS {
        conn.execute(
            &format!(
                "INSERT INTO {t}(biz_type, field_key, field_label, field_type, options, is_preset)
                 VALUES (?,?,?,?,?,1)",
                t = S::TABLE_FIELD_DEF
            ),
            params![biz, key, label, ftype, opts],
        )?;
    }
    Ok(())
}

/// 取某业务类型的字段定义（预置 + 用户自定义），按 id 升序
pub fn get_field_defs(conn: &Connection, biz_type: &str) -> Result<Vec<FieldDef>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT id, biz_type, field_key, field_label, field_type, options, is_preset
         FROM {t} WHERE biz_type=? ORDER BY id",
        t = S::TABLE_FIELD_DEF
    ))?;
    let rows = stmt.query_map([biz_type], row_to_field_def)?;
    rows.collect::<Result<Vec<_>>>()
}

/// 用户新增自定义字段定义（is_preset=0）
pub fn add_field_def(
    conn: &Connection,
    biz_type: &str,
    field_key: &str,
    field_label: &str,
    field_type: &str,
) -> Result<i64> {
    conn.execute(
        &format!(
            "INSERT INTO {t}(biz_type, field_key, field_label, field_type, is_preset)
             VALUES (?,?,?,?,0)",
            t = S::TABLE_FIELD_DEF
        ),
        params![biz_type, field_key, field_label, field_type],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 删除用户自定义字段定义（R12 补删，P2-8）：仅允许删 is_preset=0 的自定义字段。
/// 级联删除该 field_key 的全部 field_value（防孤儿值），并对受影响文档重建 FTS5
/// （字段值曾拼入索引，删除后必须重建，避免旧值残留可检索）。
/// 预置字段（is_preset=1）拒绝删除，返回 0（不报错，保持幂等）。
pub fn remove_field_def(conn: &Connection, id: i64) -> std::result::Result<usize, String> {
    use rusqlite::OptionalExtension;
    let key: Option<String> = conn
        .query_row(
            &format!(
                "SELECT field_key FROM {t} WHERE id=? AND is_preset=0",
                t = S::TABLE_FIELD_DEF
            ),
            [id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some(field_key) = key else {
        return Ok(0); // 不存在或为预置字段：不删除
    };
    // 受影响文档：删除 field_value 前先收集 doc_id（重建 FTS5 用）
    let doc_ids: Vec<i64> = {
        let mut stmt = conn
            .prepare(&format!(
                "SELECT DISTINCT document_id FROM {t} WHERE field_key=?",
                t = S::TABLE_FIELD_VALUE
            ))
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([&field_key], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };
    conn.execute(
        &format!("DELETE FROM {t} WHERE field_key=?", t = S::TABLE_FIELD_VALUE),
        [&field_key],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        &format!("DELETE FROM {t} WHERE id=?", t = S::TABLE_FIELD_DEF),
        [id],
    )
    .map_err(|e| e.to_string())?;
    // 删除后重建受影响文档的 FTS5（旧字段值从索引移除）
    for doc_id in &doc_ids {
        crate::index::rebuild_document_fts(conn, *doc_id).map_err(|e| e.to_string())?;
    }
    Ok(doc_ids.len())
}

/// 写入某文档的字段值（upsert）。写入后触发该文档 FTS5 重建（R12：自定义字段改后 FTS5 必重建）。
/// P2-2：同步更新 `document.fields` JSON（结构化快照），避免 field_value 与 fields 双写漂移。
/// 三个步骤（field_value upsert + fields JSON 更新 + FTS5 重建）包在单个事务内。
pub fn set_field_value(
    conn: &Connection,
    doc_id: i64,
    field_key: &str,
    value: &str,
) -> std::result::Result<(), String> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    tx.execute(
        &format!(
            "INSERT INTO {t}(document_id, field_key, value) VALUES (?,?,?)
             ON CONFLICT(document_id, field_key) DO UPDATE SET value=excluded.value",
            t = S::TABLE_FIELD_VALUE
        ),
        params![doc_id, field_key, value],
    )
    .map_err(|e| e.to_string())?;
    sync_document_fields_json(&tx, doc_id, field_key, value)?;
    crate::index::rebuild_document_fts(&tx, doc_id).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

/// 把单字段值并入 `document.fields` JSON（幂等：字段不存在则新增，存在则覆盖）。
fn sync_document_fields_json(
    conn: &Connection,
    doc_id: i64,
    field_key: &str,
    value: &str,
) -> std::result::Result<(), String> {
    // fields 列为 NULL 时 r.get::<_, Option<String>>(0) 返回 Ok(None)；
    // 文档不存在时 query_row 返回 Err（由调用方在 field_value upsert 之前保证文档存在）。
    let fields: Option<String> = conn
        .query_row(
            &format!("SELECT fields FROM {t} WHERE id=?", t = S::TABLE_DOCUMENT),
            [doc_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .map_err(|e| e.to_string())?;
    let mut obj: serde_json::Value = match fields {
        Some(f) if !f.trim().is_empty() => serde_json::from_str(&f)
            .map_err(|e| format!("fields JSON 解析失败: {e}"))?,
        _ => serde_json::Value::Object(serde_json::Map::new()),
    };
    if let serde_json::Value::Object(map) = &mut obj {
        map.insert(field_key.to_string(), serde_json::Value::String(value.to_string()));
    }
    let json = serde_json::to_string(&obj).map_err(|e| e.to_string())?;
    conn.execute(
        &format!(
            "UPDATE {t} SET fields=?, updated_at=CURRENT_TIMESTAMP WHERE id=?",
            t = S::TABLE_DOCUMENT
        ),
        params![json, doc_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// 取某文档的全部字段值（field_key → value）
pub fn get_field_values(conn: &Connection, doc_id: i64) -> Result<HashMap<String, String>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT field_key, value FROM {t} WHERE document_id=?",
        t = S::TABLE_FIELD_VALUE
    ))?;
    let rows = stmt.query_map([doc_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut map = HashMap::new();
    for r in rows {
        let (k, v) = r?;
        map.insert(k, v);
    }
    Ok(map)
}
