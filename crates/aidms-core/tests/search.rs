//! 阶段 4 融合检索集成测试
//! 覆盖：RRF 融合召回、多主体 per-entity 约束、trigram 子串兜底、
//!       未配向量语义降级、结果内类型/标签过滤、片段 <mark> 高亮。
use aidms_core::db;
use aidms_core::entities;
use aidms_core::ingest::{self, IngestInput};
use aidms_core::parse::Kind;
use aidms_core::search::{SearchMode, SearchRequest};

use rusqlite::Connection;

/// 确定性「bag-of-chars」嵌入（仅测试用，非真实模型）：
/// 相同/相近字符构成的文本向量余弦更高，足以验证语义召回路径。
fn dummy_embed(text: &str) -> Vec<f32> {
    let mut v = vec![0f32; 1024];
    for ch in text.chars() {
        let idx = (ch as usize) % 1024;
        v[idx] += 1.0;
    }
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in &mut v {
            *x /= norm;
        }
    }
    v
}

struct Fixture {
    conn: Connection,
    title_to_id: std::collections::HashMap<String, i64>,
}

fn ingest_doc(
    conn: &Connection,
    entity_ids: Vec<i64>,
    title: &str,
    content: &str,
    doc_type: &str,
) -> i64 {
    let input = IngestInput {
        kind: "file".into(),
        source_kind: Kind::Txt,
        title: title.into(),
        content_text: content.into(),
        fields: None,
        source: None,
        doc_type: Some(doc_type.into()),
        party: None,
        owner: None,
        date_field: None,
        note: None,
        entity_ids,
        created_at: "2024-01-01T00:00:00Z".into(),
    };
    ingest::ingest(conn, &input, Some(&|t: &str| Ok(dummy_embed(t)))).unwrap()
}

fn setup() -> Fixture {
    let conn = db::open(":memory:").unwrap();
    let mut title_to_id = std::collections::HashMap::new();

    // 三个主体
    let e_a = entities::create_entity(&conn, "重庆智习室科技有限公司", None, None, "2024-01-01T00:00:00Z").unwrap();
    let e_b = entities::create_entity(&conn, "北京未来科技有限公司", None, None, "2024-01-01T00:00:00Z").unwrap();
    let e_c = entities::create_entity(&conn, "上海云图设计有限公司", None, None, "2024-01-01T00:00:00Z").unwrap();

    let docs = [
        (e_a, "财务报表A", "2024年度财务报表 资产负债表 利润表 现金流 债务 统一社会信用代码", "财务报表"),
        (e_a, "劳动合同A", "劳动合同 员工 薪资 社保 试用期", "人事合同"),
        (e_a, "采购合同A", "采购合同 供应商 相对方 付款 交付", "商务合同"),
        (e_b, "财务报表B", "2024年度财务报表 审计 营收 纳税", "财务报表"),
        (e_b, "保密协议B", "保密协议 竞业限制 知识产权", "人事合同"),
        (e_b, "融资协议B", "融资协议 投资 股权 估值", "商务合同"),
        (e_c, "设计委托C", "设计委托合同 著作权 交付物", "商务合同"),
        (e_c, "审计报告C", "年度审计报告 财务 合规", "财务报表"),
        (e_c, "员工手册C", "员工手册 培训 考勤", "人事合同"),
    ];
    for (e, title, content, dt) in docs {
        let id = ingest_doc(&conn, vec![e], title, content, dt);
        title_to_id.insert(title.to_string(), id);
    }
    Fixture { conn, title_to_id }
}

fn run(fx: &Fixture, query: &str, mode: SearchMode, entity_ids: Option<Vec<i64>>) -> Vec<i64> {
    let req = SearchRequest {
        query: query.into(),
        query_vec: Some(dummy_embed(query)),
        mode,
        entity_ids,
        doc_types: None,
        tag_ids: None,
        limit: 10,
    };
    aidms_core::search::search(&fx.conn, &req)
        .unwrap()
        .into_iter()
        .map(|h| h.doc_id)
        .collect()
}

#[test]
fn recall_at_10_across_queries() {
    let fx = setup();
    let cases = [
        ("财务报表", "财务报表A"),
        ("劳动合同", "劳动合同A"),
        ("采购合同", "采购合同A"),
        ("保密协议", "保密协议B"),
        ("融资协议", "融资协议B"),
        ("设计委托", "设计委托C"),
        ("审计报告", "审计报告C"),
        ("员工手册", "员工手册C"),
        ("现金流", "财务报表A"),
        ("股权", "融资协议B"),
        ("社保", "劳动合同A"),
        ("著作权", "设计委托C"),
    ];
    let mut hits = 0;
    for (q, expected) in cases {
        let ids = run(&fx, q, SearchMode::Hybrid, None);
        let ok = ids.contains(&fx.title_to_id[expected]);
        if ok {
            hits += 1;
        } else {
            eprintln!("RECALL MISS: query={q} expected={expected} got={ids:?}");
        }
    }
    let recall = hits as f64 / cases.len() as f64;
    assert!(recall >= 0.9, "recall@10 = {recall} < 0.9");
}

#[test]
fn per_entity_constraint_excludes_others() {
    let fx = setup();
    let e_a = entities::list_entities(&fx.conn).unwrap()[0].id;
    let ids = run(&fx, "财务报表", SearchMode::Hybrid, Some(vec![e_a]));
    // 仅应包含 A 主体的文档，不应出现 B/C 的财务报表
    assert!(ids.contains(&fx.title_to_id["财务报表A"]));
    assert!(!ids.contains(&fx.title_to_id["财务报表B"]));
    assert!(!ids.contains(&fx.title_to_id["审计报告C"]));
}

#[test]
fn trigram_substring_fallback() {
    let fx = setup();
    // "信用代码" 为 4 字子串，可能非 jieba 短语 token，但 trigram 兜底命中
    let ids = run(&fx, "信用代码", SearchMode::Keyword, None);
    assert!(ids.contains(&fx.title_to_id["财务报表A"]));
}

#[test]
fn keyword_mode_works_without_embedding() {
    let fx = setup();
    // 不传 query_vec、且模式为关键字：仍能 FTS 召回（语义不可用时不阻断）
    let req = SearchRequest {
        query: "劳动合同".into(),
        query_vec: None,
        mode: SearchMode::Keyword,
        entity_ids: None,
        doc_types: None,
        tag_ids: None,
        limit: 10,
    };
    let hits = aidms_core::search::search(&fx.conn, &req).unwrap();
    assert!(!hits.is_empty());
    assert!(hits.iter().any(|h| h.doc_id == fx.title_to_id["劳动合同A"]));
}

#[test]
fn semantic_mode_degrades_to_keyword_when_no_vec() {
    let fx = setup();
    // 语义模式但无向量：应降级为全文而非报错，且仍能召回
    let req = SearchRequest {
        query: "采购合同".into(),
        query_vec: None,
        mode: SearchMode::Semantic,
        entity_ids: None,
        doc_types: None,
        tag_ids: None,
        limit: 10,
    };
    let hits = aidms_core::search::search(&fx.conn, &req).unwrap();
    assert!(!hits.is_empty());
    assert!(hits.iter().any(|h| h.doc_id == fx.title_to_id["采购合同A"]));
}

#[test]
fn type_filter_restricts_results() {
    let fx = setup();
    let req = SearchRequest {
        query: "合同".into(),
        query_vec: Some(dummy_embed("合同")),
        mode: SearchMode::Hybrid,
        entity_ids: None,
        doc_types: Some(vec!["财务报表".to_string()]),
        tag_ids: None,
        limit: 20,
    };
    let hits = aidms_core::search::search(&fx.conn, &req).unwrap();
    // 类型过滤为「财务报表」时，结果不应含人事/商务合同
    let titles: Vec<String> = hits
        .iter()
        .map(|h| h.title.clone())
        .collect();
    for t in &titles {
        assert!(
            t.contains('A') || t.contains('B') || t.contains('C'),
            "type 过滤未生效，出现非财务报表文档: {t}"
        );
        assert!(
            t.starts_with("财务报表") || t.starts_with("审计报告"),
            "出现非目标类型文档: {t}"
        );
    }
    assert!(!titles.is_empty());
}

#[test]
fn snippet_contains_highlight_and_is_safe() {
    let fx = setup();
    let req = SearchRequest {
        query: "财务报表".into(),
        query_vec: Some(dummy_embed("财务报表")),
        mode: SearchMode::Hybrid,
        entity_ids: None,
        doc_types: None,
        tag_ids: None,
        limit: 10,
    };
    let hits = aidms_core::search::search(&fx.conn, &req).unwrap();
    let hit = hits
        .iter()
        .find(|h| h.doc_id == fx.title_to_id["财务报表A"])
        .expect("命中财务报表A");
    assert!(hit.snippet.contains("<mark>"), "片段应含 <mark> 高亮: {}", hit.snippet);
    assert!(!hit.snippet.contains("<script>"), "片段不应含未净化脚本");
    // 高亮词应出现
    assert!(hit.snippet.contains("财务") || hit.snippet.contains("报表"));
}
