//! 数据库连接 + vec0 自动注册 + 迁移
//!
//! sqlite-vec 0.1.9 为静态链接，通过 `sqlite3_auto_extension` 在进程内自动注册 `vec0`
//! 虚拟表，无需 `load_extension`、无需随包分发扩展二进制。
//!
//! 迁移（P2-12）：`migrations/*.sql` 按文件名升序全量执行（每次启动重跑，脚本须幂等，
//! 参考 `0002_upgrade_template.sql` 的说明与写法）。
use rusqlite::ffi::sqlite3_auto_extension;

/// 进程级：注册 sqlite-vec vec0 虚拟表。必须在首个 rusqlite 连接打开前调用一次。
///
/// 重复调用安全（SQLite 内部对相同扩展去重）。
fn register_vec0() {
    unsafe {
        sqlite3_auto_extension(Some(std::mem::transmute(
            sqlite_vec::sqlite3_vec_init as *const (),
        )));
    }
}

/// 打开数据库并跑基线迁移。
///
/// - `path`：`":memory:"` 用于测试；正式路径为应用数据目录下的 `aidms.db`。
/// - 向量索引须走自建 rusqlite 连接（Tauri 内置 sqlite 插件是另一套连接，不会注册 vec0）。
pub fn open(path: &str) -> rusqlite::Result<rusqlite::Connection> {
    register_vec0();
    let conn = rusqlite::Connection::open(path)?;
    run_migrations(&conn)?;
    Ok(conn)
}

fn io_to_sqlite(e: std::io::Error) -> rusqlite::Error {
    rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
        Some(format!("迁移文件读取失败: {e}")),
    )
}

/// 执行迁移：枚举 `migrations/*.sql`（文件名升序）逐文件 `execute_batch`。
/// 0001_init.sql 为基线；后续 V2+ 升级追加 `000N_*.sql`（须幂等，见 0002 模板注释）。
pub fn run_migrations(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .map_err(io_to_sqlite)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("sql"))
        .collect();
    // 按文件名（0001_*.sql < 0002_*.sql）字典序执行
    files.sort();
    for f in files {
        let sql = std::fs::read_to_string(&f).map_err(io_to_sqlite)?;
        conn.execute_batch(&sql)?;
    }
    // 业务预置字段（R2）：首次建库写入企业通用字段定义，幂等
    crate::fields::seed_preset_field_defs(conn).map_err(|e| {
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
            Some(format!("预置字段写入失败: {e}")),
        )
    })?;
    Ok(())
}
