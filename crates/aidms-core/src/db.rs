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
        // sqlite-vec 官方用法：sqlite3_vec_init 签名为 `fn()`，而 sqlite3_auto_extension
        // 期望三参入口；用 transmute 桥接 ABI（sqlite-vec 自带测试同款写法）。
        let rc = sqlite3_auto_extension(Some(std::mem::transmute(
            sqlite_vec::sqlite3_vec_init as *const (),
        )));
        if rc != 0 {
            eprintln!("[aidms] 警告: sqlite3_auto_extension 注册 vec0 返回非零码 {rc}");
        }
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

/// 编译期嵌入迁移脚本（按文件名升序）。避免使用 `CARGO_MANIFEST_DIR` 在运行时定位
/// `migrations/`——该宏在打包后指向 CI 编译机路径，在用户机器上不存在会导致 `db::open`
/// 失败并触发启动 panic（正是此前「安装后 <1 秒闪退」的根因）。嵌入后跨平台零运行时路径依赖。
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_init.sql"),
    include_str!("../migrations/0002_upgrade_template.sql"),
];

/// 执行迁移：依次 `execute_batch` 嵌入的 SQL（须幂等，见 0002 模板注释）。
pub fn run_migrations(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    for sql in MIGRATIONS {
        conn.execute_batch(sql)?;
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
