-- AIDMS 基线迁移（版本 0001）
-- 按技术设计 §3 全字段落地；FTS5 两表以 document.id 为 rowid，vec_items 以 chunk.id 为 rowid。
-- 后续升级以 0002_xxx.sql / 0003_xxx.sql 递增，db.rs 按文件名字典序顺序执行。

-- 公司主体（典型 ≤ 5 个）
CREATE TABLE IF NOT EXISTS entity (
  id          INTEGER PRIMARY KEY,
  name        TEXT NOT NULL,
  credit_code TEXT,
  note        TEXT,
  created_at  TEXT NOT NULL
);

-- 资料主记录（文件或业务条目统一）
CREATE TABLE IF NOT EXISTS document (
  id          INTEGER PRIMARY KEY,
  kind        TEXT NOT NULL,                 -- 'file' | 'business'
  title       TEXT NOT NULL,
  type        TEXT,                          -- 合同/资质/发票/客户/项目/供应商/...
  source      TEXT,                          -- 原始路径 / 来源说明
  content_text TEXT,                         -- 解析后的纯文本
  party       TEXT,                          -- 相对方
  owner       TEXT,                          -- 负责人
  date_field  TEXT,                          -- 业务日期
  note        TEXT,
  fields      TEXT,                          -- JSON：业务条目全部结构化字段
  status      TEXT DEFAULT 'ok',             -- ok / parse_failed / ocr_pending
  sync_status TEXT DEFAULT 'local',          -- 多端同步占位，本期恒为 local
  created_at  TEXT NOT NULL,
  updated_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- 切块（RAG 引用回贴：chunk → document + 原文偏移）
CREATE TABLE IF NOT EXISTS chunk (
  id           INTEGER PRIMARY KEY,
  document_id  INTEGER,
  seq          INTEGER,                      -- 块序号
  start_offset INTEGER,                      -- 在 content_text 中的字符起点
  end_offset   INTEGER,                      -- 字符终点
  page         INTEGER,                      -- 来源页（PDF/图片，可选）
  text         TEXT                          -- 该块文本
);

-- 资料 ↔ 公司主体（多对多）
CREATE TABLE IF NOT EXISTS document_entity (
  document_id INTEGER,
  entity_id   INTEGER,
  PRIMARY KEY (document_id, entity_id)
);

-- 标签
CREATE TABLE IF NOT EXISTS tag (
  id   INTEGER PRIMARY KEY,
  name TEXT UNIQUE
);

CREATE TABLE IF NOT EXISTS document_tag (
  document_id INTEGER,
  tag_id      INTEGER,
  PRIMARY KEY (document_id, tag_id)
);

-- 业务条目自定义字段 schema（按 type 预置，R12 可增删）
CREATE TABLE IF NOT EXISTS field_def (
  id          INTEGER PRIMARY KEY,
  biz_type    TEXT NOT NULL,
  field_key   TEXT NOT NULL,
  field_label TEXT NOT NULL,
  field_type  TEXT,                          -- text/number/date/select
  options     TEXT,                          -- select 选项（JSON）
  is_preset   INTEGER DEFAULT 1              -- 1=企业通用预置 0=用户自定义
);

-- 业务条目字段值（按字段检索；写入时同步进 FTS5）
CREATE TABLE IF NOT EXISTS field_value (
  document_id INTEGER,
  field_key   TEXT NOT NULL,
  value       TEXT,
  PRIMARY KEY (document_id, field_key)
);

-- LLM / 嵌入配置（用户自配，默认空；敏感密钥存 OS keychain）
CREATE TABLE IF NOT EXISTS llm_config (
  id          INTEGER PRIMARY KEY DEFAULT 1,
  provider    TEXT,                          -- 'ollama' | 'openai_compat'
  base_url    TEXT,
  api_key_ref TEXT,                          -- 仅存引用/标记；真实密钥存 OS keychain
  embed_model TEXT,
  gen_model   TEXT,
  enabled     INTEGER DEFAULT 0
);

-- 全文索引：jieba 预切词后以空格拼接写入，unicode61 按空格分词
CREATE VIRTUAL TABLE IF NOT EXISTS document_fts USING fts5(
  title, content, tokenize='unicode61'
);

-- 子串/包含兜底索引：原生原文（不经 jieba）写入，trigram 按 3 字滑窗切分
CREATE VIRTUAL TABLE IF NOT EXISTS document_fts_trigram USING fts5(
  title, content, tokenize='trigram'
);

-- 向量索引：sqlite-vec vec0，rowid 对应 chunk.id，BGE-M3 稠密 1024 维余弦
CREATE VIRTUAL TABLE IF NOT EXISTS vec_items USING vec0(
  embedding float[1024] distance_metric=cosine
);

-- 业务条目 ↔ 文件关联（R2 关联与检索联动）
CREATE TABLE IF NOT EXISTS document_link (
  from_id INTEGER NOT NULL,                  -- 业务条目 document.id
  to_id   INTEGER NOT NULL,                  -- 关联文件 document.id
  kind    TEXT,                              -- 关联类型
  PRIMARY KEY (from_id, to_id)
);

-- 索引（多主体 / 三维筛选 JOIN 性能）
CREATE INDEX IF NOT EXISTS idx_doc_type  ON document(type);
CREATE INDEX IF NOT EXISTS idx_de_entity ON document_entity(entity_id);
CREATE INDEX IF NOT EXISTS idx_dt_tag    ON document_tag(tag_id);
CREATE INDEX IF NOT EXISTS idx_chunk_doc ON chunk(document_id);
CREATE INDEX IF NOT EXISTS idx_link      ON document_link(from_id, to_id);
