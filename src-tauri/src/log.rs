//! 统一日志出口（P2-10）：src-tauri 全部 `eprintln!` 经此模块打印，
//! 内容统一过 `aidms_core::security::redact_log` 脱敏（密钥/超长 token 掩码），
//! 避免 api_key / content_text 全文泄入日志。仅脱敏展示，不改变控制流。

use aidms_core::security::redact_log;

/// 打印带前缀的日志行（自动脱敏）。
/// 示例：`log::info("[watch]", &format!("读取失败 {src}: {e}"))`
pub fn info(prefix: &str, msg: &str) {
    eprintln!("{} {}", prefix, redact_log(msg));
}

/// 打印错误日志行（自动脱敏）。
pub fn error(prefix: &str, msg: &str) {
    eprintln!("{} {}", prefix, redact_log(msg));
}
