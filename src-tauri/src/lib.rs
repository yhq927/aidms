mod commands;
mod net;
mod config;
mod rag;
mod watch;
mod log;
#[cfg(feature = "ocr")]
mod ocr;

use aidms_core::db;
use aidms_core::ingest;
use rusqlite::Connection;
use std::collections::HashSet;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tauri::Manager;

/// 应用级状态：自建 rusqlite 连接（向量索引走它，非 Tauri 内置 sqlite 插件）
/// + 本会话已授权来源集合（拖拽/选择导入的文件清单，submit_parsed 据此做越权防护）
/// + 问答取消标志（cancel_rag 翻转，ask_rag 轮询以中断流式生成）
pub struct DbState(
    pub Arc<Mutex<Connection>>,
    pub Mutex<HashSet<String>>, // authorized_sources：本会话已 canonicalize 的授权来源路径
    pub Arc<AtomicBool>,        // rag_cancel：问答停止生成标志
);

/// 索引缺口补偿后台任务（P1-B，开发计划阶段 3 步骤 9）：
///
/// - 启动后延迟数秒执行一次（防启动阻塞：等 Tauri 主循环就绪、DB 锁空闲后再补）；
/// - 之后每 10 分钟循环执行一次（`std::thread::sleep`，与 Tauri 主循环共存，不 panic）。
/// - 复用 `commands::embed_closure`：已配置嵌入模型时顺带补写历史向量缺口（P0-2）。
///
/// 已知限制（P2-10）：补建时嵌入为同步网络调用且持 DB 锁执行；当前以「失败不阻塞 +
/// 下轮重试」务实缓解，详见 README「已知限制」。
fn spawn_index_compensation(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        // 首次延迟 8s（防启动阻塞/与 setup 内建库竞争锁）
        std::thread::sleep(std::time::Duration::from_secs(8));
        loop {
            let n = {
                let state = app.state::<DbState>();
                let conn = state.0.lock().unwrap();
                let embed = commands::embed_closure(&conn);
                match ingest::rebuild_missing_indexes(&conn, embed.as_deref())
                    .and_then(|n_fts| {
                        let n_vec = ingest::rebuild_missing_vectors(&conn, embed.as_deref())?;
                        Ok((n_fts + n_vec) as i64)
                    }) {
                    Ok(n) => n,
                    Err(e) => {
                        crate::log::error(
                            "[reindex]",
                            &format!(
                                "索引缺口补偿失败: {}",
                                aidms_core::security::redact_log(&e.to_string())
                            ),
                        );
                        0
                    }
                }
            };
            if n > 0 {
                crate::log::info("[reindex]", &format!("索引缺口补偿补建 {n} 条"));
            }
            std::thread::sleep(std::time::Duration::from_secs(600)); // 10 分钟
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = app
                .path()
                .app_data_dir()
                .expect("无法获取应用数据目录");
            std::fs::create_dir_all(&dir).ok();
            let db_path = dir.join("aidms.db");
            let conn = db::open(&db_path.to_string_lossy())
                .map_err(|e| format!("数据库初始化失败: {e}"))?;
            app.manage(DbState(
                Arc::new(Mutex::new(conn)),
                Mutex::new(HashSet::new()),
                Arc::new(AtomicBool::new(false)),
            ));
            // P1-B：启动索引缺口补偿后台任务（延迟一次 + 每 10 分钟循环）
            spawn_index_compensation(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::greet,
            commands::list_documents,
            commands::create_document,
            commands::delete_document,
            commands::link_entity,
            commands::unlink_entity,
            // 阶段 6 多主体 + 标签
            commands::list_entities,
            commands::create_entity,
            commands::update_entity,
            commands::delete_entity,
            commands::list_tags,
            commands::create_tag,
            commands::add_document_tag,
            commands::remove_document_tag,
            commands::list_document_tags,
            commands::list_documents_with_entities,
            commands::export_documents,
            commands::get_field_defs,
            commands::set_field_value,
            commands::add_field_def,
            commands::remove_field_def,
            commands::search_documents,
            // 阶段 3 入库流水线
            commands::authorize_sources,
            commands::import_files,
            commands::submit_parsed,
            commands::submit_parse_failed,
            commands::submit_ocr_pending,
            commands::complete_ocr,
            commands::reindex_missing,
            commands::get_embed_probe_status,
            // 阶段 5 LLM 配置 + RAG 问答
            config::save_llm_config,
            config::get_llm_config,
            config::set_llm_enabled,
            rag::ask_rag,
            rag::cancel_rag,
            // 阶段 7 文档关联 + 文件夹监控
            commands::create_link,
            commands::list_links,
            commands::delete_link,
            commands::start_folder_watch,
            commands::stop_folder_watch,
            commands::get_folder_watch_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
