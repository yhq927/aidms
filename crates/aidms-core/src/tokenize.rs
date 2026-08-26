//! jieba Search 模式预切词
//!
//! 入库端与查询端**必须同法调用** [`cut_search`]，保证 FTS5(unicode61) 写入与查询的空格切分一致，
//! 否则会漏召回（开发计划阶段 2 / 技术设计 §4）。
//!
//! 字典文件随 crate 编译期嵌入二进制（`include_bytes!`），运行期写出到 `%TEMP%/aidms_jieba_dict`
//! 再加载——**不再使用 `CARGO_MANIFEST_DIR` 定位 `dict/`**：该宏在打包后指向 CI 编译机路径，
//! 在用户机器上不存在，会导致 `JiebaDict::new` 打开失败并触发 C++ FATAL 崩溃（搜索/OCR 时）。
//! 写出目标为 `%TEMP%` 下的 ASCII 子目录名，规避 Windows 中文路径窄字符编码问题（cppjieba 内部
//! 用 C++ `std::ifstream` 按系统 ANSI 代码页解释窄字符路径，含中文时必然失败）。
use std::path::PathBuf;
use std::thread_local;

use jieba::{Jieba, JiebaDict};

/// 编译期嵌入词典字节（文件名与顺序须与下方 `dict_dir` 解包一致）。
const DICT_BYTES: [(&str, &[u8]); 5] = [
    ("jieba.dict.utf8", include_bytes!("../dict/jieba.dict.utf8")),
    ("hmm_model.utf8", include_bytes!("../dict/hmm_model.utf8")),
    ("user.dict.utf8", include_bytes!("../dict/user.dict.utf8")),
    ("idf.utf8", include_bytes!("../dict/idf.utf8")),
    ("stop_words.utf8", include_bytes!("../dict/stop_words.utf8")),
];

thread_local! {
    static JIEBA: Jieba = build_jieba();
}

/// 字典解包目录：从嵌入字节写出到临时目录后加载，规避打包后 `CARGO_MANIFEST_DIR` 失效，
/// 同时利用 `%TEMP%` 下的 ASCII 子目录名规避 Windows 中文路径窄字符编码问题。
fn dict_dir() -> PathBuf {
    let cache = std::env::temp_dir().join("aidms_jieba_dict");
    let _ = std::fs::create_dir_all(&cache);
    for (name, bytes) in DICT_BYTES {
        let dst = cache.join(name);
        if !dst.exists() {
            let _ = std::fs::write(&dst, bytes);
        }
    }
    cache
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
