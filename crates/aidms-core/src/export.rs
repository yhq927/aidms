//! 资料导出（R17）：按三维筛选导出 CSV / JSON，含主体标注与标签。
//!
//! 纯数据层、可独立 `cargo test`；不依赖 Tauri，导出文本由前端负责落盘/下载。
use rusqlite::{Connection, Result};
use serde::Serialize;

use crate::entities::{list_documents, DocumentFilter};
use crate::schema as S;

/// 导出格式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Csv,
    Json,
}

impl ExportFormat {
    /// 由前端字符串解析（"csv" / "json"，大小写不敏感），非法值回 Err
    pub fn parse(s: &str) -> std::result::Result<ExportFormat, String> {
        match s.to_ascii_lowercase().as_str() {
            "csv" => Ok(ExportFormat::Csv),
            "json" => Ok(ExportFormat::Json),
            other => Err(format!("不支持的导出格式: {other}（仅 csv / json）")),
        }
    }
}

/// 单条导出记录（字段扁平、含主体标注 entity_names 与 tags）
#[derive(Debug, Clone, Serialize)]
struct ExportRow {
    id: i64,
    kind: String,
    title: String,
    #[serde(rename = "type")]
    doc_type: String,
    source: String,
    party: String,
    owner: String,
    date_field: String,
    note: String,
    status: String,
    created_at: String,
    /// 归属主体名称，多个以 `;` 连接；空串表示未归类
    entities: String,
    /// 标签名称，多个以 `;` 连接
    tags: String,
}

/// 取某文档的归属主体名称（按 id 升序，便于稳定输出）
fn entity_names(conn: &Connection, doc_id: i64) -> Result<String> {
    let mut stmt = conn.prepare(&format!(
        "SELECT e.name FROM {de} de JOIN {e} e ON de.entity_id=e.id WHERE de.document_id=? ORDER BY e.id",
        de = S::TABLE_DOCUMENT_ENTITY,
        e = S::TABLE_ENTITY
    ))?;
    let names: Vec<String> = stmt
        .query_map([doc_id], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>>>()?;
    Ok(names.join(";"))
}

/// 取某文档的标签名称（按 id 升序）
fn tag_names(conn: &Connection, doc_id: i64) -> Result<String> {
    let mut stmt = conn.prepare(&format!(
        "SELECT t.name FROM {dt} dt JOIN {t} t ON dt.tag_id=t.id WHERE dt.document_id=? ORDER BY t.id",
        dt = S::TABLE_DOCUMENT_TAG,
        t = S::TABLE_TAG
    ))?;
    let names: Vec<String> = stmt
        .query_map([doc_id], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>>>()?;
    Ok(names.join(";"))
}

/// 按三维筛选生成导出记录（不截断内容，便于用户离线检索）
fn build_rows(conn: &Connection, f: &DocumentFilter) -> Result<Vec<ExportRow>> {
    let docs = list_documents(conn, f)?;
    let mut rows = Vec::with_capacity(docs.len());
    for d in docs {
        let entities = entity_names(conn, d.id)?;
        let tags = tag_names(conn, d.id)?;
        rows.push(ExportRow {
            id: d.id,
            kind: d.kind,
            title: d.title,
            doc_type: d.doc_type.unwrap_or_default(),
            source: d.source.unwrap_or_default(),
            party: d.party.unwrap_or_default(),
            owner: d.owner.unwrap_or_default(),
            date_field: d.date_field.unwrap_or_default(),
            note: d.note.unwrap_or_default(),
            status: d.status.unwrap_or_default(),
            created_at: d.created_at,
            entities,
            tags,
        });
    }
    Ok(rows)
}

/// CSV 字段转义：含逗号/引号/换行时用双引号包裹，内部双引号翻倍
fn csv_field(v: &str) -> String {
    if v.contains(',') || v.contains('"') || v.contains('\n') || v.contains('\r') {
        let mut s = String::with_capacity(v.len() + 2);
        s.push('"');
        for c in v.chars() {
            if c == '"' {
                s.push_str("\"\"");
            } else {
                s.push(c);
            }
        }
        s.push('"');
        s
    } else {
        v.to_string()
    }
}

/// 导出为 CSV（含表头）
pub fn export_csv(conn: &Connection, f: &DocumentFilter) -> Result<String> {
    let rows = build_rows(conn, f)?;
    let header = [
        "id", "kind", "title", "type", "source", "party", "owner",
        "date_field", "note", "status", "created_at", "entities", "tags",
    ];
    let mut out = String::new();
    out.push_str(&header.iter().map(|h| csv_field(h)).collect::<Vec<_>>().join(","));
    out.push('\n');
    for r in &rows {
        let cols = [
            r.id.to_string(),
            r.kind.clone(),
            r.title.clone(),
            r.doc_type.clone(),
            r.source.clone(),
            r.party.clone(),
            r.owner.clone(),
            r.date_field.clone(),
            r.note.clone(),
            r.status.clone(),
            r.created_at.clone(),
            r.entities.clone(),
            r.tags.clone(),
        ];
        out.push_str(&cols.iter().map(|c| csv_field(c)).collect::<Vec<_>>().join(","));
        out.push('\n');
    }
    Ok(out)
}

/// 导出为 JSON 数组（字段名与 CSV 表头一致）
pub fn export_json(conn: &Connection, f: &DocumentFilter) -> Result<String> {
    let rows = build_rows(conn, f)?;
    Ok(serde_json::to_string_pretty(&rows).unwrap_or_else(|_| "[]".to_string()))
}

/// 统一入口：按格式导出（驱动前端的「导出」按钮）
pub fn export_documents(
    conn: &Connection,
    f: &DocumentFilter,
    format: ExportFormat,
) -> std::result::Result<String, String> {
    let s = match format {
        ExportFormat::Csv => export_csv(conn, f),
        ExportFormat::Json => export_json(conn, f),
    };
    s.map_err(|e| e.to_string())
}
