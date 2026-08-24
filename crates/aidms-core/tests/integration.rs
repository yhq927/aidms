//! 阶段 2 集成测试：建表全字段 / 切词+FTS5 / trigram 兜底 / vec0 KNN 子集 / CRUD 防注入 / 关联 / 三维筛选
use aidms_core::db;
use aidms_core::entities::{self, DocumentFilter, NewDocument};
use aidms_core::index;
use aidms_core::schema;
use rusqlite::Connection;

fn setup() -> Connection {
    db::open(":memory:").expect("open in-memory db + migrations")
}

fn new_doc(title: &str, content: &str, doc_type: Option<&str>) -> NewDocument {
    NewDocument {
        kind: "file".to_string(),
        title: title.to_string(),
        doc_type: doc_type.map(|s| s.to_string()),
        source: None,
        content_text: Some(content.to_string()),
        party: None,
        owner: None,
        date_field: None,
        note: None,
        fields: None,
        status: None,
        created_at: "2026-08-23T00:00:00Z".to_string(),
    }
}

#[test]
fn schema_all_tables_and_columns() {
    let conn = setup();
    for (table, cols) in schema::COLUMNS {
        let existing: Vec<String> = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap()
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .map(|x| x.unwrap())
            .collect();
        assert_eq!(existing.len(), cols.len(), "表 {table} 列数不符");
        for c in *cols {
            assert!(existing.iter().any(|e| e == c), "表 {table} 缺少列 {c}");
        }
    }
    // 虚拟表存在性
    for vt in schema::VIRTUAL_TABLES {
        match *vt {
            schema::TABLE_VEC_ITEMS => {
                let v: String = conn.query_row("select vec_version()", [], |r| r.get(0)).unwrap();
                assert!(v.starts_with('v'), "vec0 未注册: {v}");
            }
            other => {
                let n: i64 = conn
                    .query_row(&format!("SELECT count(*) FROM {other}"), [], |r| r.get(0))
                    .unwrap();
                assert_eq!(n, 0, "{other} 可查询");
            }
        }
    }
}

#[test]
fn fts5_jieba_match_and_or() {
    let conn = setup();
    let id1 = entities::create_document(
        &conn,
        &new_doc("合作框架协议", "双方就智能文档管理系统达成合作意向", Some("合同")),
    )
    .unwrap();
    let id2 = entities::create_document(
        &conn,
        &new_doc("2024年度财务审计报告", "本报告涵盖资产负债与现金流分析", Some("发票")),
    )
    .unwrap();
    index::index_document_fts(&conn, id1, "合作框架协议", "双方就智能文档管理系统达成合作意向").unwrap();
    index::index_document_fts(&conn, id2, "2024年度财务审计报告", "本报告涵盖资产负债与现金流分析").unwrap();

    // 默认 AND
    let and: Vec<i64> = conn
        .prepare("SELECT rowid FROM document_fts WHERE document_fts MATCH ?")
        .unwrap()
        .query_map(["合作 框架"], |r| r.get(0))
        .unwrap()
        .map(|x| x.unwrap())
        .collect();
    assert!(and.contains(&id1), "AND 应命中 doc1: {and:?}");

    // OR 重组扩召回
    let or: Vec<i64> = conn
        .prepare("SELECT rowid FROM document_fts WHERE document_fts MATCH ?")
        .unwrap()
        .query_map(["合作 OR 财务"], |r| r.get(0))
        .unwrap()
        .map(|x| x.unwrap())
        .collect();
    assert!(or.contains(&id1) && or.contains(&id2), "OR 应命中 doc1+doc2: {or:?}");
}

#[test]
fn trigram_substring_fallback() {
    let conn = setup();
    let id = entities::create_document(
        &conn,
        &new_doc("产品说明书", "本智能文档管理系统支持多主体资料归集", Some("资质")),
    )
    .unwrap();
    index::index_document_fts(&conn, id, "产品说明书", "本智能文档管理系统支持多主体资料归集").unwrap();
    // trigram 存原生原文，子串“文档管理”应被 3 字滑窗命中
    let hits: Vec<i64> = conn
        .prepare("SELECT rowid FROM document_fts_trigram WHERE document_fts_trigram MATCH ?")
        .unwrap()
        .query_map(["文档管理"], |r| r.get(0))
        .unwrap()
        .map(|x| x.unwrap())
        .collect();
    assert!(hits.contains(&id), "trigram 子串兜底应命中: {hits:?}");
}

#[test]
fn vec0_knn_and_per_entity_subset() {
    let conn = setup();
    let doc1 = entities::create_document(&conn, &new_doc("doc1", "a", Some("合同"))).unwrap();
    let doc2 = entities::create_document(&conn, &new_doc("doc2", "b", Some("发票"))).unwrap();
    let c1 = index::write_chunks(&conn, doc1, &[(0, 5, "chunk one".to_string())]).unwrap()[0];
    let c2 = index::write_chunks(&conn, doc1, &[(0, 5, "chunk two".to_string())]).unwrap()[0];
    let c3 = index::write_chunks(&conn, doc2, &[(0, 5, "chunk three".to_string())]).unwrap()[0];

    let v1: Vec<f32> = vec![0.10; 1024];
    let v2: Vec<f32> = vec![0.12; 1024];
    let v3: Vec<f32> = vec![0.90; 1024];
    let vq: Vec<f32> = vec![0.11; 1024];
    index::write_embedding(&conn, c1, &v1).unwrap();
    index::write_embedding(&conn, c2, &v2).unwrap();
    index::write_embedding(&conn, c3, &v3).unwrap();

    let full = index::knn(&conn, &vq, 3, None).unwrap();
    assert!(!full.is_empty(), "KNN 应返回结果");

    // per-entity 子集：仅 doc1 的 chunk (c1,c2)
    let subset = index::knn(&conn, &vq, 3, Some(&[c1, c2])).unwrap();
    assert!(!subset.is_empty(), "子集 KNN 应返回结果");
    assert!(
        subset.iter().all(|(id, _)| *id == c1 || *id == c2),
        "子集约束应排除 c3: {subset:?}"
    );
}

#[test]
fn crud_parameterized_no_injection() {
    let conn = setup();
    let evil = "'; DROP TABLE document; --";
    let id = entities::create_document(&conn, &new_doc(evil, "content", Some("合同"))).unwrap();
    let d = entities::get_document(&conn, id).unwrap().expect("应能取回刚插入的文档");
    assert_eq!(d.title, evil, "参数化应原样存储，未被截断/执行");

    // 表未被注入破坏
    let n: i64 = conn.query_row("SELECT count(*) FROM document", [], |r| r.get(0)).unwrap();
    assert!(n >= 1, "document 表应仍存在且有数据");

    // 删除级联清理
    entities::delete_document(&conn, id).unwrap();
    let after: i64 = conn.query_row("SELECT count(*) FROM document", [], |r| r.get(0)).unwrap();
    assert_eq!(after, 0, "删除后 document 应为空");
}

#[test]
fn document_link_and_entity_tag() {
    let conn = setup();
    let biz = entities::create_document(
        &conn,
        &NewDocument {
            kind: "business".to_string(),
            title: "采购合同".to_string(),
            doc_type: Some("合同".to_string()),
            source: None,
            content_text: None,
            party: None,
            owner: None,
            date_field: None,
            note: None,
            fields: None,
            status: None,
            created_at: "2026-08-23T00:00:00Z".to_string(),
        },
    )
    .unwrap();
    let file = entities::create_document(&conn, &new_doc("附件扫描件", "x", Some("资质"))).unwrap();
    entities::create_link(&conn, biz, file, "attachment").unwrap();

    let n: i64 = conn
        .query_row(
            "SELECT count(*) FROM document_link WHERE from_id=? AND to_id=?",
            [biz, file],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1, "业务条目↔文件关联应建立");

    let eid = entities::create_entity(&conn, "甲公司", None, None, "2026-08-23T00:00:00Z").unwrap();
    entities::link_entity(&conn, file, eid).unwrap();
    let tid = entities::create_tag(&conn, "重要").unwrap();
    entities::add_document_tag(&conn, file, tid).unwrap();

    // 三维筛选：entity + type + tag 同时命中 file
    let docs = entities::list_documents(
        &conn,
        &DocumentFilter {
            entity_id: Some(eid),
            doc_type: Some("资质".to_string()),
            tag_id: Some(tid),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(docs.len(), 1, "三维筛选应精确命中 1 条");
    assert_eq!(docs[0].id, file);
}

// ===================== 阶段 6：实体 CRUD + 删除校验 =====================

#[test]
fn entity_crud_and_delete_guard() {
    let conn = setup();
    let now = "2026-08-23T00:00:00Z";

    // 增
    let eid = entities::create_entity(&conn, "甲公司", Some("91500000X"), Some("母公司"), now).unwrap();
    let mut list = entities::list_entities(&conn).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "甲公司");
    assert_eq!(list[0].credit_code.as_deref(), Some("91500000X"));

    // 改
    entities::update_entity(&conn, eid, "甲公司（更名）", Some("91500000Y"), Some("全资子公司")).unwrap();
    list = entities::list_entities(&conn).unwrap();
    assert_eq!(list[0].name, "甲公司（更名）");
    assert_eq!(list[0].credit_code.as_deref(), Some("91500000Y"));
    assert_eq!(list[0].note.as_deref(), Some("全资子公司"));

    // 关联一份资料后，删除应被拦截
    let doc = entities::create_document(&conn, &new_doc("某甲公司的合同", "正文", Some("合同"))).unwrap();
    entities::link_entity(&conn, doc, eid).unwrap();
    assert_eq!(entities::count_entity_documents(&conn, eid).unwrap(), 1, "应统计到 1 份归属资料");

    let guard = entities::delete_entity_guard(&conn, eid);
    assert!(guard.is_err(), "有归属资料时删除必须被拦截");
    assert!(guard.unwrap_err().contains("仍有"), "拦截信息应说明仍有资料");
    // 拦截后主体仍在
    assert_eq!(entities::list_entities(&conn).unwrap().len(), 1, "拦截后主体不应被删除");

    // 移出归属后再删：成功
    entities::unlink_entity(&conn, doc, eid).unwrap();
    assert_eq!(entities::count_entity_documents(&conn, eid).unwrap(), 0, "移出后应为 0");
    entities::delete_entity_guard(&conn, eid).unwrap();
    assert_eq!(entities::list_entities(&conn).unwrap().len(), 0, "无归属后可删除");
}

#[test]
fn entity_soft_limit_is_not_hard_enforced() {
    // 阶段 6：不设强制硬上限，典型 ≤5 为 UI 软目标；后端允许创建多于 5 个
    let conn = setup();
    let now = "2026-08-23T00:00:00Z";
    for i in 0..7 {
        entities::create_entity(&conn, &format!("公司{i}"), None, None, now).unwrap();
    }
    assert_eq!(entities::list_entities(&conn).unwrap().len(), 7, "后端允许超过 5 个主体（UI 软目标）");
}

#[test]
fn list_documents_with_entities_flags_unclassified() {
    let conn = setup();
    let now = "2026-08-23T00:00:00Z";
    let doc = entities::create_document(&conn, &new_doc("未归类文件", "正文", Some("其他"))).unwrap();
    let eid = entities::create_entity(&conn, "乙公司", None, None, now).unwrap();
    let linked = entities::create_document(&conn, &new_doc("已归类文件", "正文", Some("合同"))).unwrap();
    entities::link_entity(&conn, linked, eid).unwrap();

    // 未带筛选：两份都应返回；doc 无归属，linked 有归属
    let all = entities::list_documents_with_entities(&conn, &DocumentFilter::default()).unwrap();
    assert_eq!(all.len(), 2);
    let by_id = |id: i64| all.iter().find(|x| x.doc.id == id).unwrap();
    assert!(by_id(doc).entity_ids.is_empty(), "未归类资料 entity_ids 应为空");
    assert_eq!(by_id(linked).entity_ids, vec![eid], "已归类资料应带回主体 id");

    // 按主体筛选仅命中 linked
    let only_e = entities::list_documents_with_entities(
        &conn,
        &DocumentFilter { entity_id: Some(eid), ..Default::default() },
    )
    .unwrap();
    assert_eq!(only_e.len(), 1);
    assert_eq!(only_e[0].doc.id, linked);
}

#[test]
fn export_csv_json_with_entity_and_tag() {
  use aidms_core::export::{self, ExportFormat};

  let conn = setup();

  // 主体 + 标签
  let e1 = entities::create_entity(&conn, "重庆智习室科技", Some("91500000MA5X"), None, "2026-01-01T00:00:00Z").unwrap();
  let e2 = entities::create_entity(&conn, "上海数智信息", None, None, "2026-02-01T00:00:00Z").unwrap();
  let t1 = entities::create_tag(&conn, "重要").unwrap();
  let t2 = entities::create_tag(&conn, "年度").unwrap();

  // 文档 1：归属 e1，标签 t1/t2
  let d1 = entities::create_document(&conn, &new_doc("合作框架协议", "双方就合作达成一致", Some("合同"))).unwrap();
  entities::link_entity(&conn, d1, e1).unwrap();
  entities::add_document_tag(&conn, d1, t1).unwrap();
  entities::add_document_tag(&conn, d1, t2).unwrap();

  // 文档 2：归属 e2，无标签
  let d2 = entities::create_document(&conn, &new_doc("2024财务报表", "营收同比增长 12.4%", Some("资质"))).unwrap();
  entities::link_entity(&conn, d2, e2).unwrap();

  // CSV 全量导出
  let csv = export::export_documents(&conn, &DocumentFilter::default(), ExportFormat::Csv).unwrap();
  assert!(csv.starts_with("id,kind,title,type"), "CSV 表头正确");
  assert!(csv.contains("重庆智习室科技"), "CSV 含主体标注 e1");
  assert!(csv.contains("上海数智信息"), "CSV 含主体标注 e2");
  assert!(csv.contains("重要;年度") || csv.contains("年度;重要"), "含两标签（顺序稳健）");
  assert_eq!(csv.lines().count(), 3, "2 条数据 + 1 表头");

  // JSON 全量导出（serde_json 默认对非 ASCII 转义，故解析回 Value 校验）
  let json = export::export_documents(&conn, &DocumentFilter::default(), ExportFormat::Json).unwrap();
  let arr: Vec<serde_json::Value> = serde_json::from_str(&json).expect("JSON 可解析");
  assert_eq!(arr.len(), 2, "JSON 含 2 条");
  let row1 = arr.iter().find(|r| r["title"] == "合作框架协议").expect("找到文档1");
  assert_eq!(row1["entities"], "重庆智习室科技", "JSON 主体标注正确");
  assert!(row1["tags"].as_str().unwrap().contains("重要"), "JSON 含标签");

  // 三维筛选：仅 e1
  let csv_e1 = export::export_documents(
    &conn,
    &DocumentFilter { entity_id: Some(e1), ..Default::default() },
    ExportFormat::Csv,
  ).unwrap();
  assert!(csv_e1.contains("重庆智习室科技"), "筛选 e1 含其文档");
  assert!(!csv_e1.contains("上海数智信息"), "筛选 e1 不含 e2 文档");
  assert_eq!(csv_e1.lines().count(), 2, "仅 1 条 + 表头");

  // 格式解析
  assert!(ExportFormat::parse("CSV").is_ok());
  assert!(ExportFormat::parse("json").is_ok());
  assert!(ExportFormat::parse("xlsx").is_err());
}

#[test]
fn custom_fields_searchable_and_rebuild_on_change() {
  use aidms_core::fields;
  use aidms_core::index;
  use aidms_core::search::{SearchMode, SearchRequest};

  let conn = setup();

  // 预置字段已自动播种
  let defs = fields::get_field_defs(&conn, "合同").unwrap();
  assert!(!defs.is_empty(), "合同类型应有预置字段");

  // 业务条目（合同）：基础内容 + 自定义金额字段
  let d = entities::create_document(
    &conn,
    &new_doc("XX 采购合同", "双方就采购事宜达成一致", Some("合同")),
  )
  .unwrap();
  index::index_document_fts(&conn, d, "XX 采购合同", "双方就采购事宜达成一致").unwrap();

  // 写入自定义字段金额 = 500000（触发 FTS5 重建）
  fields::set_field_value(&conn, d, "amount", "500000").unwrap();

  // R12：自定义字段值可被全文检索
  let req = SearchRequest {
    query: "500000".to_string(),
    query_vec: None,
    mode: SearchMode::Keyword,
    entity_ids: None,
    doc_types: None,
    tag_ids: None,
    limit: 10,
  };
  let hits = aidms_core::search::search(&conn, &req).unwrap();
  assert!(hits.iter().any(|h| h.doc_id == d), "自定义金额应可被检索");

  // R12：改后重建——旧值不可搜、新值可搜
  fields::set_field_value(&conn, d, "amount", "999999").unwrap();
  let hits_old = aidms_core::search::search(
    &conn,
    &SearchRequest { query: "500000".to_string(), query_vec: None, mode: SearchMode::Keyword, entity_ids: None, doc_types: None, tag_ids: None, limit: 10 },
  )
  .unwrap();
  assert!(!hits_old.iter().any(|h| h.doc_id == d), "改后旧值不应命中");
  let hits_new = aidms_core::search::search(
    &conn,
    &SearchRequest { query: "999999".to_string(), query_vec: None, mode: SearchMode::Keyword, entity_ids: None, doc_types: None, tag_ids: None, limit: 10 },
  )
  .unwrap();
  assert!(hits_new.iter().any(|h| h.doc_id == d), "改后新值应命中");

  // field_value 读写
  let vals = fields::get_field_values(&conn, d).unwrap();
  assert_eq!(vals.get("amount").map(|s| s.as_str()), Some("999999"));
}

#[test]
fn document_link_list_delete_bidirectional() {
  use aidms_core::entities::{self, DocLink};

  let conn = setup();
  let d1 = entities::create_document(&conn, &new_doc("合同A", "内容A", Some("合同"))).unwrap();
  let d2 = entities::create_document(&conn, &new_doc("附件B", "内容B", Some("资质"))).unwrap();
  let d3 = entities::create_document(&conn, &new_doc("报告C", "内容C", Some("报告"))).unwrap();

  entities::create_link(&conn, d1, d2, "attachment").unwrap();
  entities::create_link(&conn, d3, d1, "supports").unwrap();

  // d1 双向：out → d2, in → d3
  let links: Vec<DocLink> = entities::list_links(&conn, d1).unwrap();
  assert_eq!(links.len(), 2, "d1 应有 2 条双向关联");
  let out = links.iter().find(|l| l.direction == "out").unwrap();
  assert_eq!(out.id, d2);
  assert_eq!(out.kind, "attachment");
  let inl = links.iter().find(|l| l.direction == "in").unwrap();
  assert_eq!(inl.id, d3);

  // 删除 out 关联后仅剩 in
  entities::delete_link(&conn, d1, d2).unwrap();
  let links2 = entities::list_links(&conn, d1).unwrap();
  assert_eq!(links2.len(), 1);
  assert_eq!(links2[0].id, d3);
}

// ===================== 阶段 7 收尾：R15 高级筛选 =====================

#[test]
fn advanced_filter_r15_owner_date_source() {
  let conn = setup();

  // 三份资料：不同负责人 / 日期 / 来源
  let mut a = new_doc("合同甲", "正文A", Some("合同"));
  a.owner = Some("张三".to_string());
  a.date_field = Some("2024-05-01".to_string());
  a.source = Some("/data/import/合同甲.pdf".to_string());
  let da = entities::create_document(&conn, &a).unwrap();

  let mut b = new_doc("合同乙", "正文B", Some("合同"));
  b.owner = Some("李四".to_string());
  b.date_field = Some("2024-11-15".to_string());
  b.source = Some("/data/import/合同乙.docx".to_string());
  let db_ = entities::create_document(&conn, &b).unwrap();

  let mut c = new_doc("报告丙", "正文C", Some("报告"));
  c.owner = Some("张三".to_string());
  c.date_field = Some("2025-02-20".to_string());
  c.source = Some("/data/scan/报告丙.pdf".to_string());
  let dc = entities::create_document(&conn, &c).unwrap();

  // 负责人精确匹配
  let by_owner = entities::list_documents(
    &conn,
    &DocumentFilter { owner: Some("张三".to_string()), ..Default::default() },
  )
  .unwrap();
  assert_eq!(by_owner.len(), 2, "张三应命中 2 份");
  assert!(by_owner.iter().any(|d| d.id == da));
  assert!(by_owner.iter().any(|d| d.id == dc));

  // 日期范围（含端点）
  let by_date = entities::list_documents(
    &conn,
    &DocumentFilter {
      date_from: Some("2024-06-01".to_string()),
      date_to: Some("2025-01-01".to_string()),
      ..Default::default()
    },
  )
  .unwrap();
  assert_eq!(by_date.len(), 1, "仅合同乙落在 2024-06 ~ 2025-01 区间");
  assert_eq!(by_date[0].id, db_);

  // 来源 LIKE 模糊匹配（含斜杠路径前缀）
  let by_src = entities::list_documents(
    &conn,
    &DocumentFilter { source: Some("import".to_string()), ..Default::default() },
  )
  .unwrap();
  assert_eq!(by_src.len(), 2, "来源含 import 的应命中 2 份");
  assert!(by_src.iter().any(|d| d.id == da));
  assert!(by_src.iter().any(|d| d.id == db_));

  // LIKE 通配符转义：`%` 不应当作任意匹配（防注入）
  let by_wild = entities::list_documents(
    &conn,
    &DocumentFilter { source: Some("%".to_string()), ..Default::default() },
  )
  .unwrap();
  assert!(by_wild.is_empty(), "字面 % 不应命中任何行（通配符已转义）");

  // 组合：负责人 + 日期 + 来源 三条件同时命中
  let combined = entities::list_documents(
    &conn,
    &DocumentFilter {
      owner: Some("张三".to_string()),
      date_from: Some("2024-01-01".to_string()),
      source: Some("scan".to_string()),
      ..Default::default()
    },
  )
  .unwrap();
  assert_eq!(combined.len(), 1, "组合筛选应精确命中报告丙");
  assert_eq!(combined[0].id, dc);

  // 多主体 entity_ids 与高级筛选并存
  let e1 = entities::create_entity(&conn, "主体甲", None, None, "2026-08-23T00:00:00Z").unwrap();
  let e2 = entities::create_entity(&conn, "主体乙", None, None, "2026-08-23T00:00:00Z").unwrap();
  entities::link_entity(&conn, da, e1).unwrap();
  entities::link_entity(&conn, db_, e2).unwrap();
  let multi = entities::list_documents(
    &conn,
    &DocumentFilter {
      entity_ids: vec![e1, e2],
      owner: Some("张三".to_string()),
      ..Default::default()
    },
  )
  .unwrap();
  assert_eq!(multi.len(), 1, "多主体 + 负责人交集：仅合同甲");
  assert_eq!(multi[0].id, da);
}
