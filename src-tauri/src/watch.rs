//! 文件夹监控（notify）：监控指定目录，新文件出现时自动尝试入库。
//!
//! 设计：
//! - 监控句柄存全局 `WATCHER`，避免被 drop 导致监听停止；
//! - DB 连接以 `Arc<Mutex<Connection>>` 形式全局持有，供后台线程回调使用；
//! - 仅处理 `Create` 事件中的常规文件，且 source 经 canonicalize 规范化（防 symlink 穿越）；
//! - 入库走 `ingest::ingest`（与拖拽导入同一流水线）：先读字节 → `parse::extract_text`
//!   （txt/csv/md/pdf/docx/xlsx，Rust 降级）→ 填 content_text 再入库；图片/无文本层扫描件
//!   按状态机记 `ocr_pending`，解析失败记 `parse_failed`，均不阻塞（P1-2）。
//! - 已配置嵌入模型时传真实嵌入闭包（与 import_files 一致，P0-2）。
//!
//! 注：本模块无法在当前沙箱（缺 webkit2gtk）下 `cargo build` 验证，需在装齐系统库的
//! 机器上 `cargo tauri dev` 运行时校验。代码已按 notify 6 API 编写。

use std::path::Path;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::thread;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use rusqlite::Connection;

use aidms_core::ingest;
use aidms_core::parse;

use crate::commands::embed_closure;

static WATCHER: Mutex<Option<RecommendedWatcher>> = Mutex::new(None);
static CONN: Mutex<Option<Arc<Mutex<Connection>>>> = Mutex::new(None);
/// 该监控目录新入库文件的默认归属主体（可多选，空=未归类）
static DEFAULT_ENTITIES: Mutex<Vec<i64>> = Mutex::new(Vec::new());
/// 当前监控目录（用于状态查询展示）
static WATCH_PATH: Mutex<Option<String>> = Mutex::new(None);

/// 启动文件夹监控。
/// `default_entity_ids`：该目录新入库文件的默认归属主体（可多选，空=未归类）。
pub fn start_watch(
    conn: Arc<Mutex<Connection>>,
    path: &str,
    default_entity_ids: Vec<i64>,
) -> std::result::Result<(), String> {
    *CONN.lock().unwrap() = Some(conn);
    *DEFAULT_ENTITIES.lock().unwrap() = default_entity_ids;
    *WATCH_PATH.lock().unwrap() = Some(path.to_string());

    let (tx, rx) = channel();
    let mut watcher =
        notify::recommended_watcher(tx).map_err(|e| format!("创建 watcher 失败: {e}"))?;
    watcher
        .watch(Path::new(path), RecursiveMode::NonRecursive)
        .map_err(|e| format!("监控目录失败 {path}: {e}"))?;

    thread::spawn(move || {
        for res in rx {
            if let Ok(Event {
                kind: EventKind::Create(_),
                paths,
                ..
            }) = res
            {
                for p in paths {
                    if p.is_file() {
                        handle_new_file(&p);
                    }
                }
            }
        }
    });

    *WATCHER.lock().unwrap() = Some(watcher);
    Ok(())
}

fn handle_new_file(path: &Path) {
    let title = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    // 规范化来源路径（防 symlink 穿越；失败则跳过）
    let canon = match std::fs::canonicalize(path) {
        Ok(c) => c,
        Err(_) => return,
    };
    let src = canon.to_string_lossy().into_owned();

    let conn = {
        let guard = CONN.lock().unwrap();
        match guard.as_ref() {
            Some(c) => Arc::clone(c),
            None => return,
        }
    };

    // 默认归属主体：start_watch 时由用户指定，监控期间保持不变
    let entity_ids = DEFAULT_ENTITIES.lock().unwrap().clone();

    // P1-2：读前先用 metadata 预检大小，超限直接记 parse_failed 并跳过
    // （**不先全量读入内存再拒绝**——防大文件内存 DoS）
    let meta = match std::fs::metadata(&canon) {
        Ok(m) => m,
        Err(e) => {
            crate::log::error("[watch]", &format!("读取元数据失败 {src}: {e}"));
            return;
        }
    };
    if meta.len() > parse::MAX_FILE_BYTES as u64 {
        let input = ingest::IngestInput {
            kind: "file".into(),
            source_kind: parse::Kind::Txt, // 占位；仅记 parse_failed，不解析内容
            title,
            content_text: String::new(),
            fields: None,
            source: Some(src.clone()),
            doc_type: None,
            party: None,
            owner: None,
            date_field: None,
            note: None,
            entity_ids,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        let conn = conn.lock().unwrap();
        if let Err(e) = ingest::ingest_failed(
            &conn,
            &input,
            &format!("文件超过大小上限 {} 字节", parse::MAX_FILE_BYTES),
        ) {
            crate::log::error("[watch]", &format!("记 parse_failed 失败 {src}: {e}"));
        }
        return;
    }

    // 读取字节（parse 层有 50MB 上限二次兜底，超限按 parse_failed 记录不阻塞）
    let bytes = match std::fs::read(&canon) {
        Ok(b) => b,
        Err(e) => {
            crate::log::error("[watch]", &format!("读取失败 {src}: {e}"));
            return;
        }
    };

    // P2-2：扩展名 → magic 兜底识别；无法识别时记 parse_failed，**不**按 Txt 索引二进制
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let kind = match parse::kind_from_ext(ext).or_else(|| parse::kind_from_magic(&bytes)) {
        Some(k) => k,
        None => {
            let input = ingest::IngestInput {
                kind: "file".into(),
                source_kind: parse::Kind::Txt,
                title,
                content_text: String::new(),
                fields: None,
                source: Some(src.clone()),
                doc_type: None,
                party: None,
                owner: None,
                date_field: None,
                note: None,
                entity_ids,
                created_at: chrono::Utc::now().to_rfc3339(),
            };
            let conn = conn.lock().unwrap();
            if let Err(e) =
                ingest::ingest_failed(&conn, &input, &format!("不支持的文件类型: .{ext}"))
            {
                crate::log::error("[watch]", &format!("记 parse_failed 失败 {src}: {e}"));
            }
            return;
        }
    };

    let input = ingest::IngestInput {
        kind: "file".into(),
        source_kind: kind,
        title,
        content_text: String::new(),
        fields: None,
        source: Some(src.clone()),
        doc_type: None,
        party: None,
        owner: None,
        date_field: None,
        note: None,
        entity_ids,
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    let conn = conn.lock().unwrap();
    let embed = embed_closure(&conn);

    // 解析文本 → 入库；图片/无文本层/解析失败按状态机记录，不阻塞（P1-2）
    match parse::extract_text(kind, &bytes) {
        Ok(Some(text)) => {
            let mut input = input;
            input.content_text = text;
            if let Err(e) = ingest::ingest(&conn, &input, embed.as_deref()) {
                crate::log::error("[watch]", &format!("自动入库失败 {src}: {e}"));
            }
        }
        Ok(None) => {
            if kind == parse::Kind::Image || kind == parse::Kind::Pdf {
                if let Err(e) = ingest::ingest_ocr_pending(&conn, &input) {
                    crate::log::error("[watch]", &format!("记 ocr_pending 失败 {src}: {e}"));
                }
            } else if let Err(e) = ingest::ingest_failed(&conn, &input, "文件无文本内容") {
                crate::log::error("[watch]", &format!("记 parse_failed 失败 {src}: {e}"));
            }
        }
        Err(parse::ParseError::Unsupported(reason)) => {
            if kind == parse::Kind::Image {
                if let Err(e) = ingest::ingest_ocr_pending(&conn, &input) {
                    crate::log::error("[watch]", &format!("记 ocr_pending 失败 {src}: {e}"));
                }
            } else if let Err(e) = ingest::ingest_failed(&conn, &input, &reason) {
                crate::log::error("[watch]", &format!("记 parse_failed 失败 {src}: {e}"));
            }
        }
        Err(e) => {
            if let Err(e2) = ingest::ingest_failed(&conn, &input, &e.to_string()) {
                crate::log::error("[watch]", &format!("记 parse_failed 失败 {src}: {e2}"));
            }
        }
    }
}

/// 停止监控并释放句柄。
pub fn stop_watch() -> std::result::Result<(), String> {
    if let Some(mut w) = WATCHER.lock().unwrap().take() {
        // notify 6 的 Watcher::unwatch 必须传 path（之前监控的目录）
        if let Some(p) = WATCH_PATH.lock().unwrap().clone() {
            w.unwatch(Path::new(&p)).map_err(|e| format!("停止监控失败: {e}"))?;
        }
    }
    *WATCH_PATH.lock().unwrap() = None;
    Ok(())
}

/// 当前监控状态：(是否监控中, 监控目录)。
pub fn is_watching() -> (bool, Option<String>) {
    let on = WATCHER.lock().unwrap().is_some();
    let path = WATCH_PATH.lock().unwrap().clone();
    (on, path)
}
