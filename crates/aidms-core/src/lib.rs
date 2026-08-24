//! AIDMS 核心数据层（阶段 2 + 阶段 3）
//!
//! 不依赖 Tauri，可独立 `cargo test` 验证：建表 / 迁移 / jieba 预切词 / FTS5 / vec0 / CRUD /
//! 解析 / 入库编排 / 索引缺口补偿。
//! 模块：
//! - `db`：连接 + vec0 自动注册 + 迁移
//! - `schema`：表/列名常量
//! - `tokenize`：jieba Search 模式预切词
//! - `entities`：实体/文档/标签 CRUD
//! - `index`：FTS5 / trigram / vec0 索引写入
//! - `parse`：格式识别 + 资源上限 + 纯文本/CSV/md/PDF 文本层解析
//! - `ingest`：入库编排（解析结果→document+实体关联+索引）+ 索引缺口补偿

pub mod db;
pub mod schema;
pub mod tokenize;
pub mod entities;
pub mod index;
pub mod security;
pub mod parse;
pub mod ingest;
pub mod search;
pub mod config;
pub mod rag;
pub mod export;
pub mod fields;
pub mod crypto;
