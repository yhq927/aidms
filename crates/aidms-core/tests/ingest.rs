//! 阶段 3 集成测试：解析 + 入库编排 + 索引缺口补偿 + 状态机
//!
//! 覆盖开发计划阶段 3 验收点：
//! - 多类型（txt/md/csv/pdf文本/业务）入库后 FTS5 MATCH 命中
//! - 未配置嵌入时 vec_items 空但可纯 FTS5 检索（降级路径正确）
//! - 已配置嵌入时 vec_items 行数 = chunk 行数
//! - 索引缺口补偿：删除 FTS 行后 rebuild 补回，可重新检索
//! - 解析失败样本 status=parse_failed 可见
//! - 业务条目 fields 拼入 content 可搜
//! - 多主体 link（三维筛选 entity 维度）

use aidms_core::db;
use aidms_core::entities;
use aidms_core::ingest::{self, EmbedFn, IngestInput};
use aidms_core::parse::{self, Kind};
use aidms_core::schema as S;
use aidms_core::tokenize;
use rusqlite::Connection;

fn open_db() -> Connection {
    db::open(":memory:").expect("open db")
}

fn fts_hits(conn: &Connection, q: &str) -> Vec<i64> {
    let q = tokenize::cut_search(q);
    let sql = format!(
        "SELECT rowid FROM {t} WHERE {t} MATCH ?",
        t = S::TABLE_DOCUMENT_FTS
    );
    let mut stmt = conn.prepare(&sql).unwrap();
    stmt.query_map([q], |r| r.get::<_, i64>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn trigram_hits(conn: &Connection, q: &str) -> Vec<i64> {
    let sql = format!(
        "SELECT rowid FROM {t} WHERE {t} MATCH ?",
        t = S::TABLE_DOCUMENT_FTS_TRIGRAM
    );
    let mut stmt = conn.prepare(&sql).unwrap();
    stmt.query_map([q], |r| r.get::<_, i64>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn vec_count(conn: &Connection) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {}", S::TABLE_VEC_ITEMS), [], |r| r.get(0))
        .unwrap()
}

fn chunk_count(conn: &Connection, doc_id: i64) -> i64 {
    conn.query_row(
        &format!("SELECT COUNT(*) FROM {} WHERE document_id=?", S::TABLE_CHUNK),
        [doc_id],
        |r| r.get(0),
    )
    .unwrap()
}

fn mk_input(kind: &str, sk: Kind, title: &str, content: &str) -> IngestInput {
    IngestInput {
        kind: kind.to_string(),
        source_kind: sk,
        title: title.to_string(),
        content_text: content.to_string(),
        fields: None,
        source: Some(format!("/mock/{}.dat", title)),
        doc_type: Some("合同".to_string()),
        party: None,
        owner: None,
        date_field: None,
        note: None,
        entity_ids: vec![],
        created_at: "2024-01-01T00:00:00Z".to_string(),
    }
}

fn dummy_embed() -> &'static EmbedFn {
    &| _text: &str| -> Result<Vec<f32>, String> { Ok(vec![0.1f32; 1024]) }
}

// ---- parse 单测（资源上限 / 类型识别 / CSV）----

#[test]
fn parse_kind_recognition() {
    assert_eq!(parse::kind_from_ext("pdf"), Some(Kind::Pdf));
    assert_eq!(parse::kind_from_ext("docx"), Some(Kind::Docx));
    assert_eq!(parse::kind_from_ext("CSV"), Some(Kind::Csv));
    assert_eq!(parse::kind_from_ext("xyz"), None);
    assert_eq!(parse::kind_from_magic(b"%PDF-1.4..."), Some(Kind::Pdf));
    assert_eq!(parse::kind_from_magic(b"\x89PNG\r\n\x1a\n"), Some(Kind::Image));
    assert_eq!(parse::kind_from_magic(b"hello"), None);
}

#[test]
fn parse_size_limit() {
    let big = vec![b'a'; parse::MAX_FILE_BYTES + 1];
    assert!(parse::check_size(&big).is_err());
    assert!(parse::check_size(b"small").is_ok());
}

#[test]
fn parse_csv_to_text() {
    let csv = b"name,age\nAlice,30\nBob,25";
    let r = parse::extract_text(Kind::Csv, csv).unwrap().unwrap();
    assert!(r.contains("Alice\t30"));
    assert!(r.contains("Bob\t25"));
}

// ---- ingest 集成 ----

#[test]
fn multi_type_ingest_fts_hit() {
    let conn = open_db();
    let samples = [
        ("file", Kind::Txt, "合同A", "重庆智习室科技有限公司与相对方签订合作协议"),
        ("file", Kind::Markdown, "报表B", "2024年度财务报表显示营业收入同比增长"),
        ("file", Kind::Csv, "清单C", "供应商名单包含多家制造企业"),
        ("file", Kind::Pdf, "扫描合同D", "本营业执照由市场监督管理局颁发"),
        ("business", Kind::Business, "客户E", "客户名称：远方科技，联系人：王经理"),
    ];
    for (k, sk, title, content) in samples {
        let id = ingest::ingest(&conn, &mk_input(k, sk, title, content), None).unwrap();
        assert!(id > 0);
    }
    // 各类关键词 FTS5 命中
    assert!(!fts_hits(&conn, "智习室").is_empty());
    assert!(!fts_hits(&conn, "财务").is_empty());
    assert!(!fts_hits(&conn, "供应商").is_empty());
    assert!(!fts_hits(&conn, "营业执照").is_empty());
    assert!(!fts_hits(&conn, "远方科技").is_empty());
    // trigram 子串兜底（原生原文不经 jieba）
    assert!(!trigram_hits(&conn, "市场监督管理局").is_empty());
}

#[test]
fn degrade_no_embed_vec_empty_but_searchable() {
    let conn = open_db();
    let id = ingest::ingest(
        &conn,
        &mk_input("file", Kind::Txt, "文档X", "合作协议与相对方就项目交付达成一致"),
        None,
    )
    .unwrap();
    // 未配置嵌入：vec_items 为空，但仍可纯 FTS5 检索
    assert_eq!(vec_count(&conn), 0);
    assert!(!fts_hits(&conn, "相对方").is_empty());
    assert!(chunk_count(&conn, id) > 0);
}

#[test]
fn with_embed_vec_equals_chunk() {
    let conn = open_db();
    let id = ingest::ingest(
        &conn,
        &mk_input("file", Kind::Txt, "文档Y", "这是一段用于向量化的测试文本，包含合同与财务相关内容。"),
        Some(dummy_embed()),
    )
    .unwrap();
    // 已配置嵌入：vec_items 行数 = chunk 行数
    assert_eq!(vec_count(&conn), chunk_count(&conn, id));
    assert!(vec_count(&conn) > 0);
}

#[test]
fn embed_failure_degrades_to_fts_only() {
    let conn = open_db();
    let failing_embed: &'static EmbedFn = &|_t: &str| -> Result<Vec<f32>, String> {
        Err("模型不可达".into())
    };
    let id = ingest::ingest(
        &conn,
        &mk_input("file", Kind::Txt, "文档W", "嵌入失败降级：仅保留全文索引，不阻塞入库"),
        Some(failing_embed),
    )
    .unwrap();
    // 嵌入失败：向量跳过、FTS 保留、入库不阻塞（PRD R10 降级）
    assert_eq!(vec_count(&conn), 0);
    assert!(!fts_hits(&conn, "降级").is_empty());
    let _ = id;
}

#[test]
fn index_gap_compensation() {
    let conn = open_db();
    let id = ingest::ingest(
        &conn,
        &mk_input("file", Kind::Txt, "文档Z", "索引缺口补偿测试：合同违约条款约定"),
        None,
    )
    .unwrap();
    assert!(!fts_hits(&conn, "违约").is_empty());
    // 模拟部分失败：删除 FTS 行（索引缺口）
    conn.execute(&format!("DELETE FROM {t} WHERE rowid=?", t = S::TABLE_DOCUMENT_FTS), [id])
        .unwrap();
    assert!(fts_hits(&conn, "违约").is_empty());
    // 补偿后补齐
    let n = ingest::rebuild_missing_indexes(&conn, None).unwrap();
    assert_eq!(n, 1);
    assert!(!fts_hits(&conn, "违约").is_empty());
}

#[test]
fn vector_gap_compensation_backfills_vec() {
    let conn = open_db();
    let id = ingest::ingest(
        &conn,
        &mk_input("file", Kind::Txt, "文档V", "向量缺口补偿：历史文档以 None 入库后补写向量"),
        None,
    )
    .unwrap();
    assert_eq!(vec_count(&conn), 0); // 未配置嵌入：无向量
    // 配置嵌入后补偿：应补写 vec_items（P0-2）
    let n = ingest::rebuild_missing_vectors(&conn, Some(dummy_embed())).unwrap();
    assert_eq!(n, 1);
    assert!(vec_count(&conn) > 0);
    assert_eq!(vec_count(&conn), chunk_count(&conn, id));
    // 再次调用幂等：无缺口
    let n2 = ingest::rebuild_missing_vectors(&conn, Some(dummy_embed())).unwrap();
    assert_eq!(n2, 0);
    // 未配置嵌入（None）时不重建
    let n3 = ingest::rebuild_missing_vectors(&conn, None).unwrap();
    assert_eq!(n3, 0);
}

#[test]
fn parse_failed_status_visible() {
    let conn = open_db();
    let mut input = mk_input("file", Kind::Pdf, "损坏文件", "原始内容");
    input.source = Some("/mock/broken.pdf".into());
    let id = ingest::ingest_failed(&conn, &input, "PDF 损坏/加密").unwrap();
    let doc = entities::get_document(&conn, id).unwrap().unwrap();
    assert_eq!(doc.status.as_deref(), Some("parse_failed"));
    // parse_failed 不建索引，不可搜
    assert!(fts_hits(&conn, "原始内容").is_empty());
}

#[test]
fn business_fields_searchable() {
    let conn = open_db();
    let mut input = mk_input("business", Kind::Business, "客户F", "客户基础信息");
    input.fields = Some(r#"{"客户名称":"星河集团","行业":"新能源","规模":"大型"}"#.to_string());
    let id = ingest::ingest(&conn, &input, None).unwrap();
    // 业务条目结构化字段值拼入 content，按字段值检索命中
    assert!(!fts_hits(&conn, "星河集团").is_empty());
    assert!(!fts_hits(&conn, "新能源").is_empty());
    // 主内容也可搜
    assert!(!fts_hits(&conn, "客户基础信息").is_empty());
    let _ = id;
}

#[test]
fn multi_entity_link_filter() {
    let conn = open_db();
    let e1 = entities::create_entity(&conn, "主体甲", None, None, "2024-01-01T00:00:00Z").unwrap();
    let e2 = entities::create_entity(&conn, "主体乙", None, None, "2024-01-01T00:00:00Z").unwrap();
    let mut input = mk_input("file", Kind::Txt, "共享资料", "甲乙共有的一份合作协议");
    input.entity_ids = vec![e1, e2];
    let id = ingest::ingest(&conn, &input, None).unwrap();

    let f1 = entities::DocumentFilter {
        entity_id: Some(e1),
        ..Default::default()
    };
    let f2 = entities::DocumentFilter {
        entity_id: Some(e2),
        ..Default::default()
    };
    let r1 = entities::list_documents(&conn, &f1).unwrap();
    let r2 = entities::list_documents(&conn, &f2).unwrap();
    assert!(r1.iter().any(|d| d.id == id));
    assert!(r2.iter().any(|d| d.id == id));
}
