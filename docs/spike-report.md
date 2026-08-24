# AIDMS 阶段 0 Spike 报告

> 撰写：软件开发团队（工程师）｜日期：2026-08-24
> 依据：**实际代码证据**（非虚构）。代码基线：aidms-core 47 项测试全绿（lib 15 / ingest 12 / integration 13 / search 7）、src-tauri `cargo check` 通过。
> 说明：标注「✅ 实测验证」= 有代码 + 有测试/编译证据；「⚠️ 未实装」= 设计有但当前未落地，如实记录为已知偏离。

---

## 0. 结论（TL;DR）

核心技术选型全部验证可行，唯「隔离 webview（pdfjs-dist）」一项**未实装**（以纯 Rust `pdf-extract` 文本层抽取降级替代，功能等价但不含隔离进程）；其余 6 项 Spike 均有代码 + 测试证据，可直接进入阶段 1–4 建设。

---

## 1. jieba Search 模式双端一致 —— ✅ 实测验证

- **代码**：`crates/aidms-core/src/tokenize.rs:67` `cut_search(text) -> String` 以 `Jieba::cut_for_search(text, false)` 预切词并用空格拼接；字典 5 文件（jieba.dict.utf8 / hmm_model.utf8 / user.dict.utf8 / idf.utf8 / stop_words.utf8）随 crate 分发于 `dict/`，经 `CARGO_MANIFEST_DIR` 定位。
- **双端一致**：入库端 `index.rs:69` `index_document_fts` 写 FTS5(unicode61) 前调用 `cut_search`；查询端 `search.rs:367` 同样调用 `cut_search` 后拆分 token。两端同法切词，保证空格分词索引与查询一致，不漏召回（技术设计 §4）。
- **Windows 中文路径兼容（实测修复）**：cppjieba 内部用 C++ `std::ifstream` 按系统 ANSI 代码页解释窄字符路径；工程/安装路径含中文（UTF-8）时字典加载必然失败并 C++ FATAL（0xc0000409）。修复：`tokenize.rs:31` `dict_dir()` 在 Windows 且路径非 ASCII 时，把 5 个字典复制到 ASCII-only 临时目录 `%TEMP%/aidms_jieba_dict` 再加载。
- **测试证据**：`tests/search.rs` `fts5_jieba_match_and_or`、`recall_at_10_across_queries`；`tests/integration.rs` `fts5_jieba_match_and_or`。

## 2. sqlite-vec 静态链接 + auto_extension —— ✅ 实测验证

- **代码**：`crates/aidms-core/src/db.rs:12-18` `register_vec0()` 用 `rusqlite::ffi::sqlite3_auto_extension(Some(sqlite_vec::sqlite3_vec_init))` 在进程级注册 `vec0` 虚拟表；`db.rs:24 open()` 在首个连接打开前调用。
- **结论**：sqlite-vec 0.1.9 静态链接进二进制，无需 `load_extension`、无需随包分发扩展 `.dll/.so`；Tauri 内置 sqlite 插件是另一套连接不会注册 vec0，故向量索引走自建 rusqlite 连接（`lib.rs:16-23` DbState）。
- **测试证据**：全量 47 项测试在 `db::open(":memory:")` 上运行，vec0 建表/写入/KNN 均通过。

## 3. vec0 cosine 建表 + KNN —— ✅ 实测验证

- **建表**：`migrations/0001_init.sql` 定义 `vec_items(rowid INTEGER PRIMARY KEY, embedding BLOB) USING v0`（cosine 度量，distance 越小越相关）。
- **写入**：`index.rs:10` `to_blob`（f32 小端拼接）→ `index.rs:129` `write_embedding(conn, chunk_id, vec)`，rowid = `chunk.id`（技术设计 §3 rowid 约定）。
- **KNN**：`index.rs:145` `knn(query, k, subset)` 执行 `SELECT rowid, distance FROM vec_items WHERE embedding MATCH ? AND k = ?`；支持可选 chunk 子集 `rowid IN (...)`（per-entity KNN 底座）。
- **测试证据**：`tests/integration.rs` `vec0_knn_and_per_entity_subset`。

## 4. rusqlite × sqlite-vec ABI —— ✅ 实测验证（编译通过即为证）

- rusqlite 0.40.2（`bundled` feature，静态编译 SQLite）与 sqlite-vec 0.1.9 的扩展初始化函数经 `std::mem::transmute` 转为 `unsafe extern "C" fn` 注册（`db.rs:14-16`）。
- **验证**：`cargo test` 编译 + 47 项测试全绿，即 ABI 签名匹配、自动扩展在 bundled SQLite 内正常注册并被虚拟表查询使用。若 ABI 不匹配会在注册/查询期崩溃或报 `no such module: vec0`，实际均未发生。

## 5. FTS5 多 token 连接符（AND→OR→trigram 兜底） —— ✅ 实测验证

- **代码**：`search.rs:367-383`：
  1. jieba 主表 **AND**：`tokenized`（空格连接全部 token）`MATCH`，bm25 升序取 Top-200（`search.rs:165-178`）；
  2. jieba 主表 **OR**：`tokens.join(" OR ")` 扩大召回，缀在 AND 之后（`search.rs:180+`）；
  3. **trigram 兜底**：`document_fts_trigram`（原生原文，3 字滑窗）以双引号包裹原始查询串做包含匹配（`search.rs:379-383`），覆盖 jieba 未收录词/英文大小写/长串。
- **测试证据**：`tests/search.rs` `fts5_jieba_match_and_or`、`trigram_substring_fallback`；`tests/integration.rs` `trigram_substring_fallback`。

## 6. per-entity KNN 可行性 —— ✅ 实测验证

- **代码**：`search.rs:77-97` `subset_chunk_ids`：按 `document_entity` 关联算出指定主体的 chunk id 集合（SQL 参数化 `IN (...)`）；`index.rs:151` KNN 追加 `AND rowid IN (...)` 把向量检索限定在该主体子集内；`search.rs:410-414` 另有候选级 entity/type/tag 过滤兜底。
- **结论**：per-entity 向量检索 =「chunk 子集 + 子集内 cosine KNN」，主体切换器（R8）可与向量路直接联动，实测通过。
- **测试证据**：`tests/search.rs` `per_entity_constraint_excludes_others`；`tests/integration.rs` `vec0_knn_and_per_entity_subset`。

## 7. 隔离 webview（pdfjs-dist）+ CSP —— ⚠️ 未实装（已知偏离）

- **设计**：PDF 渲染 + OCR 放 Rust 侧独立进程，docx/xlsx 在主 webview 仅纯文本提取、不渲染 HTML（技术设计 §10 解析隔离）。
- **实际**：**未实装隔离 webview / pdfjs-dist**。PDF 走纯 Rust `pdf-extract` 文本层抽取（`parse.rs:116 extract_pdf`），无文本层（扫描件）返回 `Ok(None)` 转 OCR；docx/xlsx 走 `zip`+手写 XML 纯文本提取（`parse.rs`，P0-3 落地，仅文本不渲染 HTML）；图片走 `ocr.rs`（feature=ocr 门控，P1-3 接入状态机）。
- **CSP**：`tauri.conf.json` dev/prod 分离；prod 已按技术设计 §10 精确放行 `style-src-attr 'unsafe-inline'` 与 `style-src-elem 'unsafe-inline'`（P1-1），未放宽 `script-src`/`connect-src`。
- **风险与后续**：PDF 解析在 Rust 侧与主进程同进程（无独立沙箱进程），解析隔离强度低于设计；若引入 pdfjs-dist 需重新评估 webview CSP 与资源加载策略。已记录为已知偏离，不阻塞当前交付。

---

## 附：验证命令

```bash
cd crates/aidms-core && cargo test        # 47 项全绿
cd src-tauri && cargo check               # 通过（默认 feature，无 OCR 系统库依赖）
cd frontend && npm run build              # 通过（vite build）
```
