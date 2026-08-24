//! jieba Search 模式预切词
//!
//! 入库端与查询端**必须同法调用** [`cut_search`]，保证 FTS5(unicode61) 写入与查询的空格切分一致，
//! 否则会漏召回（开发计划阶段 2 / 技术设计 §4）。
//!
//! 字典文件随 crate 分发于 `dict/`，通过 `CARGO_MANIFEST_DIR` 定位（Tauri 打包后在阶段 1 改为资源路径）。
//! 用 `thread_local` 持有 `Jieba`：避免全局静态对 `Sync` 的要求（Jieba 内部含裸指针），每个线程懒加载一次。
//!
//! ⚠️ Windows 中文路径兼容（2026-08-24 实测修复）：cppjieba 内部用 C++ `std::ifstream` 打开字典，
//! 在 Windows 上按系统 ANSI 代码页（如 GBK）解释窄字符路径——若工程/安装路径含中文（UTF-8），
//! 打开必然失败并触发 C++ FATAL 崩溃（0xc0000409）。解决：路径含非 ASCII 时先把 5 个字典文件
//! 复制到 ASCII-only 的临时目录（`%TEMP%/aidms_jieba_dict`）再加载，规避编码错配。
use std::path::PathBuf;
use std::thread_local;

use jieba::{Jieba, JiebaDict};

const DICT_FILES: [&str; 5] = [
    "jieba.dict.utf8",
    "hmm_model.utf8",
    "user.dict.utf8",
    "idf.utf8",
    "stop_words.utf8",
];

thread_local! {
    static JIEBA: Jieba = build_jieba();
}

/// 定位字典目录：优先原 `dict/`；Windows 上若路径含非 ASCII 则复制到 ASCII 临时目录后返回。
fn dict_dir() -> PathBuf {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("dict");
    #[cfg(windows)]
    {
        if base.to_string_lossy().is_ascii() {
            return base;
        }
        let cache = std::env::temp_dir().join("aidms_jieba_dict");
        let _ = std::fs::create_dir_all(&cache);
        for name in DICT_FILES {
            let dst = cache.join(name);
            if !dst.exists() {
                let _ = std::fs::copy(base.join(name), &dst);
            }
        }
        cache
    }
    #[cfg(not(windows))]
    {
        base
    }
}

fn build_jieba() -> Jieba {
    let base = dict_dir();
    let dict = JiebaDict::new(
        &base.join("jieba.dict.utf8").to_string_lossy(),
        &base.join("hmm_model.utf8").to_string_lossy(),
        &base.join("user.dict.utf8").to_string_lossy(),
        &base.join("idf.utf8").to_string_lossy(),
        &base.join("stop_words.utf8").to_string_lossy(),
    );
    Jieba::with_dict(dict)
}

/// Search 模式切词并以空格拼接（写 FTS5 unicode61 表 / 查询端复用同一函数）
pub fn cut_search(text: &str) -> String {
    JIEBA.with(|j| j.cut_for_search(text, false).join(" "))
}
