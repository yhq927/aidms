//! 表 / 列名常量（避免 SQL 字符串散落 + 防注入：所有标识符走常量）
//!
//! 各表列名数组用于迁移后字段数断言（阶段 2 验收：全字段创建）。

pub const TABLE_ENTITY: &str = "entity";
pub const TABLE_DOCUMENT: &str = "document";
pub const TABLE_CHUNK: &str = "chunk";
pub const TABLE_DOCUMENT_ENTITY: &str = "document_entity";
pub const TABLE_TAG: &str = "tag";
pub const TABLE_DOCUMENT_TAG: &str = "document_tag";
pub const TABLE_FIELD_DEF: &str = "field_def";
pub const TABLE_FIELD_VALUE: &str = "field_value";
pub const TABLE_LLM_CONFIG: &str = "llm_config";
pub const TABLE_DOCUMENT_FTS: &str = "document_fts";
pub const TABLE_DOCUMENT_FTS_TRIGRAM: &str = "document_fts_trigram";
pub const TABLE_VEC_ITEMS: &str = "vec_items";
pub const TABLE_DOCUMENT_LINK: &str = "document_link";

/// 普通表的列名（用于 `PRAGMA table_info` 断言列数 / 列名）
pub const COLUMNS: &[(&str, &[&str])] = &[
    (
        TABLE_ENTITY,
        &["id", "name", "credit_code", "note", "created_at"],
    ),
    (
        TABLE_DOCUMENT,
        &[
            "id", "kind", "title", "type", "source", "content_text", "party", "owner",
            "date_field", "note", "fields", "status", "sync_status", "created_at", "updated_at",
        ],
    ),
    (
        TABLE_CHUNK,
        &["id", "document_id", "seq", "start_offset", "end_offset", "page", "text"],
    ),
    (
        TABLE_DOCUMENT_ENTITY,
        &["document_id", "entity_id"],
    ),
    (TABLE_TAG, &["id", "name"]),
    (TABLE_DOCUMENT_TAG, &["document_id", "tag_id"]),
    (
        TABLE_FIELD_DEF,
        &["id", "biz_type", "field_key", "field_label", "field_type", "options", "is_preset"],
    ),
    (
        TABLE_FIELD_VALUE,
        &["document_id", "field_key", "value"],
    ),
    (
        TABLE_LLM_CONFIG,
        &["id", "provider", "base_url", "api_key_ref", "embed_model", "gen_model", "enabled"],
    ),
    (
        TABLE_DOCUMENT_LINK,
        &["from_id", "to_id", "kind"],
    ),
];

/// 虚拟表（FTS5 / vec0）不在 PRAGMA table_info 的常规列里，单独列出用于存在性断言
pub const VIRTUAL_TABLES: &[&str] = &[
    TABLE_DOCUMENT_FTS,
    TABLE_DOCUMENT_FTS_TRIGRAM,
    TABLE_VEC_ITEMS,
];
