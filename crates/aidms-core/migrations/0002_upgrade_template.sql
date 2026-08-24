-- =====================================================================
-- 0002_upgrade_template.sql — V2+ 升级迁移模板（示例，默认不改变任何现有表）
-- =====================================================================
--
-- 升级流程说明（配合 crates/aidms-core/src/db.rs `run_migrations`）：
--   1. 在 migrations/ 目录新增 `000N_*.sql`（数字前缀决定执行顺序，按文件名升序执行）；
--   2. 每个迁移文件必须**幂等**（应用会每次启动全量重跑，非一次性版本记录）；
--   3. 迁移脚本只做 DDL/DML，不做应用逻辑判断（逻辑判断放 Rust 侧）。
--
-- 幂等写法示例：
--   ✅ 建表/建索引：`CREATE TABLE IF NOT EXISTS ...` / `CREATE INDEX IF NOT EXISTS ...`
--   ⚠️ SQLite **不支持** `ALTER TABLE ... ADD COLUMN IF NOT EXISTS`：
--      需要给现有表加列时，把「PRAGMA table_info 检查 + 条件 ALTER」放在 Rust 侧
--      （参考 schema.rs 列断言），或改用「新表 + 数据迁移 + 原子换名」流程。
--
-- 下方是「新增一张元信息表」的幂等示例（不影响既有 12 表与 schema.rs 断言），
-- 若你的升级不需要新表，可整段删除，只保留本注释头即可。

CREATE TABLE IF NOT EXISTS app_meta (
  key   TEXT PRIMARY KEY,
  value TEXT,
  updated_at TEXT DEFAULT (datetime('now'))
);

-- 示例：写入一条应用元信息（幂等 upsert，可重复执行）
INSERT OR IGNORE INTO app_meta (key, value) VALUES ('schema_minor_version', '1');
