# AIDMS 企业资料管理系统（Tauri 2 桌面端）

本地优先的企业资料管理：多公司主体（≤5）资料归集、OCR、融合检索（FTS5 + 向量 RRF）、RAG 问答。数据不出本机。

## 工程结构

```
aidms/
├─ crates/aidms-core/   # 核心数据层（不依赖 Tauri，可独立 cargo test）
│  ├─ src/
│  │  ├─ db.rs          # 连接 + vec0 auto_extension 注册 + 迁移
│  │  ├─ schema.rs      # 表/列名常量
│  │  ├─ tokenize.rs    # jieba Search 模式预切词
│  │  ├─ entities.rs    # 实体/文档/标签 CRUD（参数化防注入）+ 三维筛选
│  │  ├─ index.rs       # FTS5 双表 + 切块 + vec0 KNN/子集
│  │  ├─ search.rs      # 融合检索：FTS5 + vec0 → RRF + <mark> 高亮
│  │  ├─ config.rs      # LLM 配置（DB 非敏感字段；密钥走 keyring）
│  │  ├─ rag.rs         # RAG 上下文检索 + 提示注入隔离（分角色 + 数据边界标记）
│  │  ├─ security.rs    # 路径校验 + 日志脱敏
│  │  ├─ parse.rs       # 类型/资源上限白名单 + 纯文本/CSV/md/PDF 文本层解析
│  │  └─ ingest.rs      # 入库编排 + 索引缺口补偿 + 状态机
│  ├─ migrations/0001_init.sql   # 全字段基线迁移（内嵌执行）
│  ├─ tests/            # 阶段 2 + 阶段 3 集成测试
│  └─ dict/             # jieba 字典
├─ src-tauri/          # Tauri 2 后端（需装齐系统库后 cargo tauri dev/build）
│  ├─ src/{lib,commands,net}.rs
│  ├─ src/ocr.rs        # OCR 调度（feature=ocr；Rust 侧 tesseract + RapidOCR 降级）
│  ├─ tauri.conf.json  # CSP dev/prod 分离 + 拖拽 + 窗口
│  └─ capabilities/     # 最小权限（默认全关）
└─ frontend/           # React + Vite + 经典 shadcn/ui（Tailwind v3 + HSL Token）
```

## 已完成进度

- **阶段 0 Spike**：核心数据层技术路线验证（见 `/workspace/docs/spike-report.md`）
- **阶段 1**：Tauri 2 脚手架 + 前端骨架（Tailwind v3.4.1 + shadcn + PRD §6.4 HSL Token + 5 页面路由 + 侧栏主体切换器/主题切换 + Zustand persist）+ 安全基线（路径校验 / 日志脱敏 / DOMPurify 净化 / SSRF 出网客户端占位）
- **阶段 2**：数据模型全字段建表 + 迁移 + jieba 预切词 + FTS5(unicode61)/trigram/vec0 索引写入 + CRUD（参数化防注入）
- **阶段 3**：入库流水线
  - `parse.rs`：格式识别（扩展名 + magic 白名单）+ §3.4 资源上限（大小/页数/超时/文本上限）+ 纯文本/CSV/md 解析 + PDF 文本层（`pdf-extract` 纯 Rust 无系统库）；docx/xlsx 主路径在前端 mammoth/SheetJS，图片 OCR 在 Rust 侧
  - `ingest.rs`：编排（解析结果/业务字段 → document + 多主体关联 + FTS5 双表 + 切块 + 向量可选）+ **嵌入不可达时跳过向量、仅保留 FTS5 降级路径** + 索引缺口补偿（启动时/定时补建）+ 状态机（`ok`/`parse_failed`/`ocr_pending`）
  - `src-tauri` 命令层：`submit_parsed`（单一受限回传入口，含 source 授权集合校验 + 长度上限 + 调用 `aidms_core::ingest`）、`authorize_sources`、`submit_parse_failed`、`submit_ocr_pending`、`complete_ocr`、`reindex_missing`；`net.rs` 追加 `embed_text`（Ollama 兼容 `/api/embed`，经 SSRF 客户端）；`ocr.rs`（feature=ocr，tesseract + RapidOCR 降级）
  - **验证**：`cargo test` 阶段 3 集成测试 10 项全绿（多类型 FTS5 命中 / 降级 vec 空但可搜 / 已配嵌入 vec=chunk / 索引缺口补偿 / 解析失败可见 / 业务字段可搜 / 多主体 link）+ 阶段 2/security 共 9 项 = **19 项全绿**
- **阶段 4**：融合检索 + 搜索 UI
  - `search.rs`：FTS5 路（jieba 主表 AND/OR 主干 + trigram 子串兜底合并为单路 rank）+ vec0 路（查询向量 KNN，**per-entity 子集约束** + 同文档取最相关 chunk 的 min 距离）→ 两路各自排 rank 后 **RRF(1/(k+rank)) 融合**；片段直接对 `document.content_text` 生成 `<mark>` 高亮（前端经 DOMPurify 仅放行 `<mark>` 后注入）；entity/type/tag 过滤；`SearchMode` 门控（未配向量仅全文）
  - `src-tauri` `search_documents` 命令：调用 `aidms_core::search`，**语义/融合模式且前端未传向量时自动调用嵌入模型**（`net::embed_text` + SSRF 客户端，密钥不落日志），失败则降级全文；返回结构化 `SearchHit`（doc_id/title/snippet/score/semantic）
  - 前端 `Search.tsx` + `searchClient.ts`：结果卡片（标题/净化高亮片段/相关度/语义徽标）+ 模式切换（全文/融合/语义）+ 结果内类型筛选 + 主体范围（接全局筛选 store）；`index.css` 加 `mark` 主题化高亮（背景 `--warning` 0.28 / 文字 `--foreground`）
  - **验证**：核心数据层 `cargo test` 阶段 4 集成测试 7 项全绿（recall@10≥0.9 / 多主体隔离 / trigram 子串兜底 / 语义降级 / 模式门控 / 过滤 / 高亮含 `<mark>`）；前端 `vite build` 通过；阶段 2/3/security 无回归 = **26 项全绿**
- **阶段 5**：LLM 配置 + RAG 问答（流式）
  - `config.rs`（aidms-core）：`LlmConfig`（provider/base_url/embed_model/gen_model/enabled，不含 api_key）+ `get/upsert/set_enabled`（llm_config 表 id=1 单例），密钥不落 DB
  - `rag.rs`（aidms-core）：`retrieve_context` 复用阶段 4 `search` 取 Top-K（受 entity/type/tag 约束，多主体贯穿）+ `build_messages`（**固定 system 指令 + 检索片段用 `<<<RETRIEVED_DATA_START/END>>>` 边界标记包裹**分角色、不带 tools、转义隔离）
  - `src-tauri`：`config.rs`（save/get/set_enabled 命令，**api_key 存 OS keyring 不落明文**）；`rag.rs`（`ask_rag` 异步命令经 **Tauri Channel 流式**回传 + `cancel_rag` 翻转 `AtomicBool` 中断生成，全程走 `net` SSRF 客户端，密钥不落日志）；`net.rs` 加异步客户端 + `post_stream`（SSE 流式）
  - 前端 `QA.tsx` + `ChatCard.tsx` + `ragClient.ts`：对话卡（流式打字指示 + 停止生成）+ 引用 `[资料N]` 徽标 + 主体范围（接全局筛选 store）；未配置/未启用 AI 时发送禁用
  - **验证**：核心数据层 `cargo test` 阶段 5 单测 **4 项全绿**（配置 roundtrip + 上下文受主体约束 + **≥3 个提示注入样本隔离**：忽略上文/伪装系统指令/注入型 query → system 不被污染、数据标记包裹、答案不泄露系统指令）；前端 `vite build` 通过；阶段 2/3/4/security 无回归 = **30 项全绿**
- **阶段 6**：多主体切换器 + 三维筛选 + 归属提示（UI 与后端联动）
  - `entities.rs`（aidms-core）：补全实体 CRUD `update_entity` / `count_entity_documents` / `delete_entity_guard`（**删除有归属资料的主体时拦截**）+ `list_documents_with_entities`（返回每条文档的 `entity_ids`，供「未归类主体」徽标真实路径使用，不靠打桩）；`SearchHit` 扩展 `entity_ids` 字段
  - `src-tauri` 命令层：`list_entities` / `create_entity` / `update_entity` / `delete_entity` / `list_tags` / `list_documents_with_entities`（`entity_ids` 透传）；`lib.rs` 全量注册
  - 前端客户端：`entityClient.ts`（实体+标签 CRUD，含 mock）、`documentClient.ts`（`list_documents` / `list_documents_with_entities`，含 mock）、`catalog.ts`（entity_id↔name 映射）、`docTypes.ts`（类型常量）
  - 前端 store：`useCatalogStore`（实体+标签，依赖后端、非持久化）；`useFilterStore`（entity/type/tag 三维持久化）驱动搜索/问答/资料库/切换器
  - 组件：`EntitySwitcher`（顶栏主体切换器，≤5 Badge ToggleGroup 软目标）、`ThreeDimensionalFilter`（entity×type×tag 正交 + 按公司快速视图）、`UnclassifiedBadge`（未归类主体标示）、`EntityPicker`（导入多主体多选 + R16 关键词归属建议，仅提示不自动判定）
  - 页面：`Layout`（顶栏嵌入切换器）、`Sidebar`（移除占位块保留主题）、`Library`（列表接 catalog+三维筛选+主体/未归类徽标+导入选择器）、`Search`（结果卡接 `entity_ids` 未归类徽标 + 筛选实时联动）、`Entities`（增删改，删除有归属拦截提示）、`Settings`
  - **验证**：核心数据层 `cargo test` 阶段 6 新增 **3 项全绿**（实体 CRUD roundtrip + 删除有归属拦截 + `list_documents_with_entities` 返回 `entity_ids`）；前端 `vite build` 零告警通过；阶段 2/3/4/5/security 无回归 = **33 项全绿**
- **阶段 7（分批推进，沙箱可验证部分先行）**：配置页 / 导入引导 / 文件夹监控 / 导出 / 自定义字段
  - **R17 资料导出（已交付）**：`export.rs`（aidms-core，可 `cargo test`）：`export_documents(conn, &DocumentFilter, ExportFormat)` 复用三维筛选 + 主体/标签关联，CSV（含表头 + 字段转义）/ JSON（含 `entities` 主体标注 + `tags`）；`ExportFormat::parse` 拒绝非法格式；`src-tauri` 命令 `export_documents`（透传 entity/doc_type/tag + format）；前端 `documentClient.exportDocuments`（含 mock 本地生成）+ `ExportDialog.tsx`（CSV/JSON 选择 + Blob 下载）+ 资料库「导出」按钮（按当前筛选导出）
  - **验证**：核心数据层 `cargo test` 阶段 7 新增 **1 项全绿**（CSV/JSON 含主体标注 + 三维筛选过滤 + 格式解析）；前端 `vite build` 零告警通过；阶段 2/3/4/5/6/security 无回归 = **34 项全绿**
  - **R2 业务预置表单 + R12 自定义字段（已交付）**：`fields.rs`（aidms-core，可 `cargo test`）：`seed_preset_field_defs`（建库自动播种企业通用预置字段，覆盖 客户/合同/项目/供应商/资质，幂等）+ `get_field_defs` / `add_field_def`（用户自定义）/ `set_field_value`（upsert，**写入后自动 `rebuild_document_fts` 重建 FTS5**）/ `get_field_values`；`index.rs` 的 `index_document_fts` 自动并入 `field_value` 使其可全文检索，`rebuild_document_fts` 从主表回读重写两路 FTS5（向量重建由「已配嵌入」时另行触发，与阶段 3 降级口径一致）；`src-tauri` 命令 `get_field_defs` / `set_field_value` / `add_field_def` / `link_entity` / `unlink_entity`；前端 `fieldClient.ts` + `documentClient.createDocument`/`linkEntity` + `BusinessForm.tsx`（按业务类型动态渲染预置/自定义字段 + 多主体归属 + 提交建文档/关联/写字段）+ 资料库「业务条目」按钮
  - **验证**：核心数据层 `cargo test` 阶段 7 新增 **1 项全绿**（自定义字段值可被检索 + 改后旧值失效新值命中 + field_value 读写）；**预置字段随建库自动播种，全量 35 项测试无回归**（integration 11 / ingest 10 / search 7 / security 7）；前端 `vite build` 零告警通过
  - **导入引导（已交付）**：`Onboarding.tsx`（driver.js 聚光灯引导，localStorage `aidms-onboarding-done` 首次触发、缺失元素自动跳过；driver.css import 打包同源零 CSP 放宽），Layout 挂载，Sidebar/Library/EntitySwitcher 补锚点 id
  - **文件夹监控（已交付）**：`watch.rs` `start_watch` 支持默认归属主体 `default_entity_ids`（自动入库按目录默认归属，不打断用户）+ `is_watching()`；命令 `start_folder_watch` / `stop_folder_watch` / `get_folder_watch_status`（返回 `{running, path}` 对象契约）；前端 `watchClient.ts`（mock 内存状态）+ Settings 页「文件夹监控」设置区（目录 + 默认主体多选 + 启停 + 状态 Badge）
  - **高级筛选 R15（已交付）**：`entities.rs` `DocumentFilter` 扩展 `entity_ids`（多主体并集）/ `owner`（精确）/ `date_from`/`date_to`（date() 日历序）/ `source`（LIKE + `\`%`_` 转义防通配符注入），全部参数化；命令层 `doc_filter()` 统一组装；前端 `AdvancedFilterPanel.tsx`（折叠面板 + 状态持久化）+ Library 接入；新增 `advanced_filter_r15_owner_date_source` 集成测试（含通配符转义/日期范围/多主体并存断言）
  - **关联文件联动 UI（已交付）**：`DocumentDrawer` 双向关联展示 + 方向感知删除（in 方向删对方→本件记录）+ 目标排除自身/已关联；`BusinessForm` 创建业务条目可选关联文件（`createLink(id, fid, "业务关联")`）
  - **真机契约修复（三轮 QA 回归，P0/P1 清零）**：watch.rs `unwatch` 补 path 参数（P0 编译错误）；`get_folder_watch_status` 改返回结构体；`DocumentWithEntities` 加 `#[serde(flatten)]` 扁平化；`NewDocumentInput`/`SearchRequestInput`/`AskRequest`/`NewEntityInput`/`UpdateEntityInput` 加 `#[serde(rename_all = "camelCase")]` 且前端统一 `{req/input/doc:{...}}` 包裹；全库 21 处 invoke 逐一核对无扁平传参遗漏
  - ⚠️ 待真机联调对齐（前端暂无调用，Rust 未改避免无工具链风险）：~~`submit_parsed` 系列 / `save_llm_config` / `authorize_sources` / `complete_ocr` / `reindex_missing`~~（**2026-08-24 已全部接线**，见下方对齐修复批次）；另 `frontend/dist-old*` 为构建产物备份，确认后可删

### ✅ 2026-08-24 与开发文档对齐修复（审计驱动，QA 最终回归可交付）

双视角审计（QA+架构师）发现 P0/P1/P2 偏离后，三批次修复完成，QA 独立回归通过（aidms-core **47 项测试全绿** 15/12/13/7、src-tauri cargo check + net 单测 4 项、前端 vite build）：

- **P0-1 SSRF 本地放行**：`net.rs` `is_restricted_ip` 环回（127.0.0.1/::1）从受限摘出（本地 Ollama 主场景可用），private/link-local/unspecified 仍拉黑；4 个单测实跑通过。
- **P0-2 语义/向量链路**：`read_embed_config` 改读 llm_config 正确列（embed_model + keyring + enabled=1）；submit_parsed/reindex_missing 传真实嵌入闭包；`ingest.rs rebuild_missing_vectors` 补历史缺向量文档；Search.tsx 未启用时语义/融合禁用 + 回退全文。
- **P0-3 文件导入主链路**：Rust 侧 `parse.rs` 补 docx/xlsx 纯文本解析（zip crate 手写 XML，5 单测）；新命令 `import_files`（canonicalize + 授权 + extract_text + ingest，逐文件 ok/parse_failed/ocr_pending）；前端 dialog 多选 → importFiles → toast 汇总刷新。
- **P0-4 AI 配置页**：`configClient.ts` + Settings.tsx 本地/云端二选一表单（行内校验 base_url 协议 + Key 必填、状态徽标），密钥仅 IPC 传 Rust 存 keyring。
- **P1**：prod CSP 补 `style-src-attr/style-src-elem 'unsafe-inline'`（driver.js/sonner 内联放行，script/connect 未放宽）；watch.rs 监控入库解析内容（extract_text 填 content_text）；OCR 接入状态机（`ocr_doc_if_possible` feature 门控，未启用保持 ocr_pending + 前端"待 OCR/解析失败"徽标）；RAG 引用可点击跳转（`on_cites` Channel + ChatCard 点击开抽屉 + QA 未配置禁用）；`docs/spike-report.md` 补齐（按实际代码证据，如实标注未实装项）。
- **P2（12 项）**：窗口 1024×720；EntitySwitcher「未归类」chip（列表端过滤）；DocumentDrawer 归属即点即存；parse 页数/超时上限（MAX_PAGES=500/MAX_PARSE_SECS=30，lopdf 页数预检 + run_with_timeout）；mark radius 3px；搜索 autoFocus + 列表/卡片视图切换；收藏/最近 Tab（localStorage 演示级）；BusinessForm 自定义字段 UI；`log.rs` 统一 redact_log 脱敏日志出口；keyring 失败降级 crypto.rs 加密文件（`.aidms_keyring_fallback.b64`）；`migrations/0002_upgrade_template.sql` + db.rs 按文件名升序执行迁移。

**遗留 minor（不阻塞）**：net.rs `get_text`/`post_json` dead_code 告警（预留 API）；aidms-core ingest.rs 1 处 eprintln（已脱敏）；OCR feature=ocr 实跑需系统 libtesseract（README 已知限制）；拖拽导入（tauri://drag-drop）未接线（当前走 dialog 主链路）。

### 🏁 2026-08-24 五轮循环深度审计收官（QA 最终判定：核心链路全绿，可交付）

五轮双视角循环审计（QA 需求覆盖 + 架构师技术设计一致性）全部完成，第 5 轮（末轮）QA 判定：**64 项核心测试 + 9 项 src-tauri 单测全绿、cargo check 通过、前端构建通过，无 P0/P1 新增，达到可交付状态**。五轮累计修复 20+ 项（P0/P1 归零）：

| 轮次 | 发现并修复 |
|---|---|
| 第 1 轮 | P0-5 云端 provider 分路 + P1-A 业务走 ingest + P1-B 索引补偿定时 + P1-C SSRF IPv6 拉黑 + P1-D zip 上限 + P2×10 |
| 第 2 轮 | vec0 维度守卫（写侧） + ingest 事务 + 大文件预检 + xlsx 聚合上限 + embed /v1 归一 + OCR 声明 + P2×10 |
| 第 3 轮 | RAG 语义 query_vec 链路 + 查侧维度守卫 + 维度探测缓存（永不收敛防护）+ HTTP 超时 + P2×6 |
| 第 4 轮 | ask_rag/search_documents 锁外嵌入（三段式） + 流式读间隔超时 + ok 缓存对称 + mismatch 用户提示 + 重建失败补偿标记 |
| 第 5 轮 | 复验全部落地，**判定可交付**（仅文档化已知限制） |

**发布前必选动作（唯一遗留 P1 级）**：R14 OCR 默认 feature 关闭，发布构建需启用 `ocr` feature（系统 libtesseract + chi_sim.traineddata）或按 PRD 补 RapidOCR 降级；当前降级路径（扫描件标记"待 OCR"、不阻塞导入）可接受。其余已知限制（持锁补建嵌入、拖拽导入、慢 prefill 首 token 60s 取舍）均已文档化。

### 🔁 2026-08-24 循环深度审计第 1 轮修复（R1b 批次，本批次）

第 1 轮双视角审计（QA+架构师）发现 P0-5 云端分路不完整与 P1×5/P2 高价值项，本批次完成剩余修复（P0-5 验证补齐 + P1-A~P1-D + P2 若干）：

- **P0-5 云端模式 provider 分路（补齐）**：`net.rs` 已有 `embed_url_for`（Ollama→`/api/embed`；openai_compat→`{base}/embeddings`）与 `parse_embed_response`（兼容 `data[].embedding` / `embeddings` 两结构）；补齐 `commands.rs` `read_embed_config` 返回 provider 并传入 `embed_text`（此前漏传 provider 导致编译错误）；`rag.rs` chat URL 对已含 `/v1` 的 base_url 去重（避免 `/v1/v1/chat/completions`）。
- **P1-A 业务条目创建改走 submit_parsed**：`BusinessForm.tsx` 提交从 `create_document`（纯 INSERT 不可搜）改为 `submit_parsed`（kind=business、source_kind=business、fields=JSON 全字段、content_text 拼入基础字段值），入库即建 FTS5/chunk/向量，业务条目（含仅基础字段）提交后即可被搜索命中、RAG 上下文非空。
- **P1-B 索引缺口补偿触发点**：`lib.rs` setup 后 spawn 后台线程——延迟 8s 调一次 `rebuild_missing_indexes`+`rebuild_missing_vectors`，之后每 10 分钟循环；补 `log::info` 记录补建条数（与 Tauri 主循环共存，失败仅记日志不 panic）。
- **P1-C SSRF IPv6 拉黑补充**：`net.rs` `is_restricted_ip` IPv6 分支补 `is_unicast_link_local`（fe80::/10）、`is_unique_local`（fc00::/7）、IPv4 映射段 `::ffff:0:0/96` 转回 V4 判定；::1 与 ::ffff:127.0.0.1 保持放行（本地 Ollama）；新增 5 断言单测。
- **P1-D docx/xlsx 解压炸弹防护**：`parse.rs` 新增 `ZipTooLarge` 变体；`read_zip_entry_text` 用 `entry.size()` 预检 + `take(MAX_ZIP_ENTRY_BYTES)` 限长兜底（防伪造 size 元数据）；docx document.xml / xlsx sharedStrings 与 sheet 均受限；新增 docx/xlsx 高压缩比（Deflate 压 2M+ 字符）拒绝单测。
- **P2-1 删除资料闭环**：`entities::delete_document` 补 `DELETE FROM field_value WHERE document_id=?`；新增 `delete_document` 命令并注册；前端 `documentClient.deleteDocument` + Library 卡片「删除」按钮（confirm 后刷新）。
- **P2-2 watch 未知扩展名**：`watch.rs` 改为先读字节再 `kind_from_ext`→`kind_from_magic`，无法识别记 parse_failed（不再按 Txt 索引二进制）。
- **P2-3 mock 特殊字符**：`searchClient.ts` 对查询串转义正则元字符，特殊字符不再抛 RegExp 异常。
- **P2-4 RAG 传 doc_types/tag_ids**：`QA.tsx` 补传 `doc_types`/`tag_ids`（useFilterStore 已有 docType/tagId），主体用「单选+多选」并集。
- **P2-5 搜索窗口期禁用**：`Search.tsx` `llmEnabled===null`（加载中）时语义/融合按钮同样禁用（点击忽略 + title 提示加载中）。
- **P2-7 注释同步**：`submit_parsed` 注释「嵌入暂降级 None」→「经 embed_closure 嵌入，不可达降级 FTS5」。
- **P2-8 自定义字段可删**：`fields.rs` 新增 `remove_field_def`（仅删 is_preset=0，级联删 field_value 并重建受影响文档 FTS5）；命令 `remove_field_def` 注册；`fieldClient.removeFieldDef` + BusinessForm 自定义字段「删除」按钮。
- **P2-9 真机导入弹主体多选器**：`Library.tsx` 真机 `import_files` 成功后弹 `EntityPicker` 批量 `linkEntity`（失败文件不参与）。
- **P2-10 锁外嵌入 TODO**：`ingest.rs reindex` 嵌入持锁网络调用处加 TODO 注释（记录「嵌入阻塞」已知限制，下方「环境与已知限制」同步）。
- **P2-11 keyring 降级口令风险注释**：`config.rs device_passphrase` 加熵风险标注（兜底仅防明文，非高安全）。

### 🔁 2026-08-24 循环深度审计第 2 轮修复（R2b 批次，本批次）

第 2 轮审计发现 P1×6 + 高价值 P2×10，本批次完成全部 P1 + 高价值 P2（状态逐项标注）：

- **P1-1 vec0 维度守卫（完成）**：`index.rs` 新增 `EMBEDDING_DIM=1024`；`write_embedding` 写入前校验维度，不符视为「嵌入不可用」——返回 Ok 但不写 vec（不抛错、不产生半成品，文档保持 FTS 可搜），向量缺口由 `rebuild_missing_vectors` 幂等补偿。单测：维度不符不写库且不报错 / 维度正确写库。
- **P1-5 ingest 事务（完成）**：`ingest.rs` `ingest`/`ingest_failed`/`ingest_ocr_pending`/`complete_ocr` 均包 `unchecked_transaction`（创建 document + 关联 entity + FTS/trigram + 切块 + 向量，任一步失败整体回滚）；新增 `InvalidKind` 校验（仅 file/business）。单测：非法 kind 不落库；触发器模拟中途失败 → document/FTS/chunk 全回滚。
- **P1-2 大文件预检（完成）**：`commands.rs import_one_file` 与 `watch.rs handle_new_file` 读前先 `std::fs::metadata().len()` 预检（>50MB 直接拒，记 parse_failed/错误结果），不再先全量读入内存再拒绝（防内存 DoS）；`check_size` 保留二次兜底。
- **P1-3 xlsx 聚合总量上限（完成）**：`parse.rs` 新增 `MAX_ZIP_ENTRIES=5000`（zip 条目数超限拒绝）；多 sheet 文本**累积过程中**按 `MAX_CHARS` 提前截断（超限报 `ZipTooLarge`，不再先全量累积再 take）；`xlsx_shared_strings` 累计字符同样受限。单测：多 sheet 聚合超总量被拒 / 条目数超限被拒。
- **P1-4 embed_url_for /v1 归一（完成）**：`net.rs embed_url_for` 对 openai_compat 与 chat 端一致的 /v1 归一（trim 尾斜杠 → 不含 /v1 补 /v1 → +/embeddings），修复用户填 `https://api.siliconflow.cn`（无 /v1）时「chat 可用 embed 404」；ollama 分支不变。单测：openai 带/不带 /v1、尾斜杠、ollama 三场景 + `parse_embed_response` 双结构（P2-9）。
- **P1-6 OCR 端到端声明（完成，务实范围）**：`tauri.conf.json` bundle 加 `resources: []` 占位（JSON 不支持注释，阶段 8 打包说明见 `ocr.rs` 文件头 + 本 README「环境与已知限制」：加入 tessdata/*.traineddata + ONNX）；`ocr.rs` 文件头补 RapidOCR 降级为 TODO 注释（需 ort + PP-OCRv4 ONNX，见技术设计 §5）；README 同步 OCR 状态。**未引入无法验证的 ort 依赖**。
- **P2-1 标签功能死区（部分完成）**：后端补齐 `create_tag` / `add_document_tag` / `remove_document_tag` 命令（entities.rs 已有 CRUD 实现，此前仅暴露 `list_tags`）；三维筛选 tag 维度在无标签时优雅空态（`list_documents` 的 tag 子查询不匹配即空结果，不报错）。**标签 UI（管理/打标入口）为 P2 后续项**，README 记录。
- **P2-2 submit_parsed 长度口径（完成）**：`commands.rs` content_text/fields 上限改按字符数 `chars().count()` 校验（与 parse.rs `MAX_CHARS` 按字符一致，中文 2M 字符不再被字节数误拒）。
- **P2-3 create_document 命令残留（完成）**：前端 `documentClient.createDocument` 加废弃注释（勿用于业务条目，会绕过索引）；Rust `create_document` 命令保留（兼容）但注释警示。
- **P2-4 业务自定义字段未写 field_value（完成）**：`BusinessForm.tsx` 提交时对每个自定义字段调 `set_field_value` 写 field_value 表（字段写失败仅 warning，不阻断主创建）；头注释同步。
- **P2-5 set_field_value 只重建 FTS 不重建向量（完成）**：`ingest.rs` 新增 `rebuild_document_index`（单文档完整重建）；`set_field_value` 命令在已配置嵌入时调用它同步重建向量；未配置则跳过（仅 FTS5）。
- **P2-6 embed_closure 每次新建 client（完成）**：`SafeHttpClient` 在闭包外只建一次再 move 进闭包，避免每 chunk 重建 reqwest client。
- **P2-7 docx/xlsx 无解析超时（完成）**：`parse.rs extract_text` 对 docx/xlsx 复用 `run_with_timeout`（与 PDF 一致，MAX_PARSE_SECS=30）。
- **P2-8 RAG 前端未传 use_semantic（完成）**：`QA.tsx` 在 `llmEnabled===true` 时传 `useSemantic:true`（后端不可达自动降级全文，安全）。
- **P2-9 P0-5 无单测（完成）**：`net.rs` 补 `embed_url_for`（两 provider/尾斜杠//v1）与 `parse_embed_response`（Ollama embeddings / OpenAI data[] 双结构）单测。
- **P2-10 N+1 查询（完成一处）**：`entities.rs list_documents_with_entities` 改单条 `IN (...)` 一次取全部归属映射，替代每文档一条 SELECT；其余 N+1（search/export/rag）标注为性能优化项（数据量小，当前不阻塞）。

### 🔁 2026-08-24 循环深度审计第 3 轮修复（R3b 批次，本批次）

第 3 轮审计发现 P1×4（无 P0），本批次完成全部 P1 + 顺手 P2×6（状态逐项标注）：

- **P1-1 RAG 语义检索恒失效（完成）**：core `rag.rs retrieve_context` 新增 `query_vec: Option<Vec<f32>>` 参数（mode 用 Hybrid + 配 query_vec 即走语义融合）；src-tauri `ask_rag` 在 `use_semantic=true` 时经 `commands::embed_query` 嵌入查询向量（pub(crate) 暴露）后传入。`retrieve_context` 旧调用点（2 处测试）已迁移。单测：无关词 + 匹配 query_vec 走语义路命中 / 无 query_vec 降级全文无关词不命中。
- **P1-2 查询侧向量维度无守卫（完成）**：core `search.rs` 在 `use_vec=true` 时校验 `query_vec.len() == EMBEDDING_DIM`，不符 eprintln + 降级 Keyword（不报错）。与写侧（write_embedding）对称。
- **P1-3 rebuild_missing_vectors 维度不符永不收敛（完成）**：core `ingest.rs` 利用 `0002_upgrade_template.sql` 的 `app_meta` 表实现「模型维度探测前置」——首次调用嵌入短探测文本：维度符合写 `embed_dim_probe=ok`、不符写 `mismatch` 并立即返回 0（不重嵌）。缓存 mismatch 后所有调用直接返回 0 不再触发嵌入（不消耗网络/DB 锁）。模型变更时 `sync_embed_dim_probe_model` 由 src-tauri `embed_closure` 调用以清缓存恢复。单测：坏维度缓存后二次调用计数=0 / 模型修复后正常补建。
- **P1-4 SafeHttpClient 无 HTTP 超时（完成）**：net.rs 两 client（blocking + async）均加 `.connect_timeout(10s)` + `.timeout(120s)`；嵌入超时失败走既有降级不阻塞入库；流式问答总超时 2 分钟保护 cancel 无法中断已挂起 await 的永久挂起。
- **P2-1 ingest_failed/ingest_ocr_pending kind 白名单（完成）**：两函数入口补 `if kind != "file" && kind != "business" → InvalidKind`，与 ingest 主入口一致。单测：非法 kind 全部三入口均拒绝。
- **P2-2 set_field_value 同步 document.fields JSON（完成）**：fields.rs 拆出 `sync_document_fields_json`（upset 单字段到 fields JSON 整体），与 field_value upsert + FTS5 重建同事务（避免双写漂移）。`r.get::<_, Option<String>>(0)` 处理 NULL。
- **P2-3 标签前端打标 UI 最小实现（完成）**：后端 `entities::list_document_tags` + 命令 `list_document_tags` + entityClient 新增 createTag/addDocumentTag/removeDocumentTag/listDocumentTags；DocumentDrawer 新增「标签」区：点选即打/取消 + ＋ 新建并立即打标（Enter 也触发）。
- **P2-4 bundle.icon 补 icon.ico（完成）**：tauri.conf.json `icon: ["icons/icon.png", "icons/icon.ico"]`（icon.ico 已生成）。
- **P2-5 delete_document / rebuild_missing_vectors 包事务（完成）**：entities.rs `delete_document` 整体包 `unchecked_transaction`（任一步失败整体回滚）；ingest.rs `rebuild_missing_vectors` 与 `rebuild_missing_indexes` 每条文档独立事务（单文档失败不回滚其它）。
- **P2-6 chat_url_for 抽公共函数（完成）**：net.rs 新增 `pub fn chat_url_for(base_url)`，与 `embed_url_for` 同一文件维护；src-tauri `rag.rs` 复用（去重 base.rs chat URL 构造逻辑）。

### 🔁 2026-08-24 循环深度审计第 4 轮修复（R4b 批次，本批次）

第 4 轮审计（QA 视角）发现 P1×1 + P2×4，本批次全部修复：

- **R5-P1 锁内同步嵌入（完成，P1）**：`ask_rag`（rag.rs）与 `search_documents`（commands.rs）原在持 `Mutex<Connection>` 期间做阻塞嵌入 HTTP（use_semantic=true 的每次问答 / 语义搜索都让全应用 DB 排队，端点挂起最坏 120s 冻结）。改为三段式：① 锁内只读 `read_embed_config`（llm_config 非敏感，立即释放）；② 释放锁后经 `tokio::task::spawn_blocking` 调 `embed_query_with_config`（网络不占 async worker，也不占 DB 锁）；③ 重新取锁执行 `retrieve_context` / `search`。锁只在实际 DB 访问时短暂持有。
- **R5-P2-1 流式问答 120s 总超时截断（完成）**：net.rs async client 去掉 `.timeout(120s)`（reqwest async 的 timeout 是「建连到读完」总时长，长答案/慢 prefill >120s 被截断），保留 `connect_timeout(10s)`；阻塞 client（嵌入/非流式）仍保留 120s。流式读取改由 rag.rs `tokio::time::timeout(60s, stream.next())` 按**读间隔**超时——60s 无新数据才报错（服务挂起仍可退出），长答案不再截断。前端 ragClient 错误提示区分超时/网络错误。
- **R5-P2-2 ok 缓存 write-only（完成）**：`rebuild_missing_vectors` 此前「ok」只写不读、每轮仍重复探测嵌入文本；现在与 mismatch 对称——缓存 ok 后跳过探测，直接进入缺向量补建 SELECT。正确性由 `sync_embed_dim_probe_model`（模型名变更清缓存）保证。单测：ok 缓存后二次调用嵌入计数=0。
- **R5-P2-3 mismatch 无用户可见信号（完成）**：`reindex_missing` 返回 `ReindexMissingOut{reindexed, dim_mismatch}`；新增 `get_embed_probe_status` 命令；前端配置页在保存模型/加载时检测 `embed_dim_probe=mismatch` 展示警告「嵌入模型维度与内置 1024 维不符，语义检索已降级为关键词」。
- **R5-P2-4 set_field_value 向量重建失败无补偿（完成）**：字段事务已提交后 `rebuild_document_index` 失败不再向上抛错（字段已保存，不应因索引失败回滚业务）；记录日志 + `ingest::mark_document_reindex_pending` 写 app_meta 待补建标记；`rebuild_missing_indexes` 消费该标记（补建成功移除、失败保留下轮再试），与既有 FTS/向量缺口补偿闭环。单测：pending 标记触发补建并移除。

## 构建

```bash
# 前端（已验证可 build）
cd frontend && npm install && npm run build

# 核心数据层（已验证 cargo test 全绿）
cd crates/aidms-core && cargo test

# 桌面端（需先装系统库，见下）
cd src-tauri && cargo tauri dev
```

> ✅ **2026-08-24 Rust 工具链已在本机（Windows）配置完成**：rustup stable 1.98.0（msvc）+ VS Build Tools 2022（C++ 负载，rusqlite bundled / jieba C++17 编译必需）。
> `cargo test` 实测 **47+ 项全绿**（lib / ingest / integration / search，含第 1 轮审计新增单测）；`src-tauri cargo check` 通过；前端 `vite build` 通过。
>
> ⚠️ **Windows 中文路径两处实坑（已修复）**：
> 1. **jieba 字典打开崩溃**（0xc0000409）：cppjieba 内部用 `std::ifstream` 按系统 ANSI 代码页（GBK）解释路径，工程路径含中文（UTF-8）时字典必打不开。已修复：`tokenize.rs` 检测路径含非 ASCII 时先把 5 个字典复制到 `%TEMP%/aidms_jieba_dict` 再加载。
> 2. **安全测试平台差异**：`canonicalize_rejects_escape` 原用 Linux 专属 `/etc/passwd` 作逃逸锚点，Windows 上不存在导致断言失败。已改为跨平台构造（在临时目录建真实根外文件 + `../` 逃逸）。
> 3. **bash 环境 Rust 路径**：`$USERPROFILE` 是 Windows 风格（`C:\Users\...`），bash 里须用 `$HOME`（`/c/Users/...`）；且**不要显式设 `RUSTUP_HOME/CARGO_HOME`**（会解析成 `f:/c/Users/...` 错误路径），让 rustup 用默认值即可。
> 4. **首次真机编译修复**（src-tauri）：移除未装插件的 `asset:default` capability、PIL 生成 icons（icon.png/icon.ico）、`post_stream` 移入 `impl SafeHttpClient`、删多余函数参数 `#[serde(default)]`、`Kind::File`→`kind_from_ext`、`unwatch` 补 `mut`、补 `use tauri::Manager;`。`get_text`/`post_json` 为预留方法（dead_code 警告可忽略）。

## 发布与打包（阶段 8 · 双平台：Windows + macOS）

双平台（Windows + macOS）打包配置与 CI 已就绪；本机与 CI 职责明确划分。**Linux 不在范围内**（不构建 `.deb`/`.AppImage`，CI 矩阵已移除 ubuntu runner）。

### 本机可直接做
- `cargo test`（core 64 项）/ `cargo check` / `vite build` 全绿（见上「构建」）。
- 出 **Windows app 二进制**：`frontend/node_modules/.bin/tauri build --no-bundle`（从工程根 `aidms/` 运行；只编译 `target/release/aidms.exe`，不碰安装包，绕开本机缺的 WiX/NSIS）。
- `npm run dev` 开发模式、功能联调。

### 本机做不到（交由 CI / 联网打包机）
- **安装包**（`.msi`/`.exe`/`.dmg`）：本机未装 WiX、NSIS、macOS 公证工具链 → 由 `.github/workflows/release.yml` 双平台矩阵构建（Windows runner 出 msi/nsis，macOS runner 出 app/dmg 并公证）。
- **OCR feature 真编译**：`ocr` 依赖系统 `libtesseract`/`libleptonica`，本机未装 → 由 CI 在装齐系统库的 runner 上 `--features ocr` 编译。
- **词库下载**：`.traineddata`（≈30MB）不入 git，CI 联网执行 `scripts/download-tessdata.sh` 拉到 `src-tauri/resources/tessdata/` 随包分发。

### CI 是什么、现在在跑吗
- **CI = GitHub Actions 自动出包流程**，配置文件即 `.github/workflows/release.yml`。可理解为「云上一台自动打包机器」：将来要发版时，它会在 GitHub 提供的 Windows / macOS 虚拟机上自动编译并产出安装包。
- **它现在没有在跑**，原因：① 这个 workflow 文件只是放在本地项目里，还没推送到 GitHub；② 即使推送了，它也只在你**推送 `v*` 标签（发版）**或**在 GitHub 手动点 `workflow_dispatch`** 时才启动，不会闲着一直跑；③ 出可分发安装包前还需在仓库配置签名/公证 Secrets（Apple 证书、Windows 签名私钥）。
- 当前状态：**已配好、待触发**。你本机后台跑的 `tauri build --no-bundle` 是另一次**本地编译**，与 CI 无关。

### 安全回归与发布门禁
- `scripts/security-check.sh`：校验 prod CSP 无 dev 放宽项、日志无 `api_key`/`content_text` 全文、密钥经 keyring；阶段 8 实跑 PASS。
- `docs/release-checklist.md`：OCR 启用、双平台打包/签名、安全回归、安装后冒烟、G6 统计门禁的逐项清单。

## 环境与已知限制

- **sqlite-vec 0.1.9 为静态链接**：通过 `sqlite3_auto_extension` 自动注册 `vec0` 虚拟表，**无需 `load_extension`、不随包分发 `.so/.dylib/.dll`、无需运行时完整性哈希**。向量索引须走自建 rusqlite 连接（非 Tauri 内置 sqlite 插件）。
- **jieba 0.1.3 在 C++17 工具链下编译失败**（`cjieba` 的 `limonp/StdExtension.hpp` 用 `std::tr1::unordered_map`，与 C++17 冲突）。本工程已在构建环境将 `#elif(__cplusplus == 201103L)` 改为 `>= 201103L` patch 生效；正式工程建议用 `[patch.crates-io]` 指向修复后的 fork，避免每台机器重踩。另：0.1.3 无 `Jieba::new()`，须 `Jieba::with_dict(JiebaDict::new(dict,hmm,user,idf,stop))` 传 5 个字典路径。
- **Tauri 桌面端需系统库**：`webkit2gtk-4.1` / `javascriptcoregtk-4.1` / `libsoup-3.0` / `gtk+-3.0`（Linux）。本机（Windows）已配齐 Rust 工具链 + WebView2，可出 **app 二进制**（`tauri build --no-bundle` → `target/release/aidms.exe`）；但本机**未装 WiX / NSIS / macOS 公证 / Linux deb 工具链**，且**未装 libtesseract**，故**安装包与 `ocr` feature 真编译交由 CI 三平台矩阵**（`scripts/security-check.sh` + `.github/workflows/release.yml`，统一 `--features ocr`）。核心数据层与前端已独立验证。
- **OCR 状态（已实现 feature 门控）**：`Cargo.toml` 以 `ocr = ["dep:tesseract"]` 默认关闭（不拉系统 `libtesseract`，保证无库环境可 `cargo check`/编译）；开启 `ocr` feature 后接入 Rust 侧 `tesseract`（system lib）+ `chi_sim`/`eng` 词库。`tauri.conf.json` bundle 已配 `resources: ["resources/tessdata"]`，运行时经 `app.path().resource_dir()` 解析词库目录（见 `commands.rs::resolve_tessdata_dir`：优先 `TESSDATA_PREFIX` → 否则 `resource_dir()/resources/tessdata` → 退 `tessdata`，不依赖 cwd，打包后桌面端也能找到词库）。`.traineddata`（≈30MB 二进制）不入 git，由 `scripts/download-tessdata.sh|.ps1` 在 CI/联网打包机下载到 `src-tauri/resources/tessdata/`；未启用 `ocr` 时图片保持 `ocr_pending`（前端「扫描件待 OCR」徽标）；RapidOCR 降级为 TODO（不引入未验证的 `ort`+ONNX 依赖）。
- **解析路线定稿（阶段 3）**：PDF 有文本层走隔离 webview `pdfjs-dist`（主路径）或 Rust 侧 `pdf-extract` 降级；PDF 扫描件/图片走 Rust 侧 OCR（`feature=ocr` 的 `tesseract` + RapidOCR 降级）；docx/xlsx 走前端 `mammoth`/`SheetJS`（主路径）。aidms-core 已落地的纯 Rust 解析子集（txt/CSV/md/PDF 文本层）可独立验证，docx/xlsx/OCR 的真实工程集成在装齐系统库的机器上验证。
- **嵌入降级**：未配置/不可达/维度不符嵌入模型时 `submit_parsed` 仅建 FTS5 索引（不阻塞入库），配置就绪后经 `reindex_missing` 增量补嵌；`net::embed_text` 已按 provider 分路（P0-5/P1-4：Ollama `/api/embed`、openai_compat `/v1/embeddings` 自动归一，均经 SSRF 客户端）。维度不符（mismatch）时前端配置页显示「语义检索已降级为关键词」提示（R5-P2-3）。
- **嵌入阻塞已知限制（P2-10 / R5-P1 部分缓解）**：**问答（ask_rag）与搜索（search_documents）的查询嵌入已移到 DB 锁外**（R5-P1：锁内只读配置 → 释放 → 锁外嵌入 → 重新取锁），这两条交互路径不再因嵌入 HTTP 阻塞全应用 DB；但**入库/补建路径**（`ingest::reindex` 持 `Mutex<Connection>` 逐 chunk 嵌入）仍为持锁同步网络调用，嵌入模型慢时会阻塞其它写操作；当前以「嵌入失败不阻塞入库 + 缺口补偿重试」务实缓解，低成本重构（落库后异步补嵌）留待后续。
- **流式问答超时（R5-P2-1）**：流式读取不再受 120s 总超时限制（长答案/慢 prefill 不再被截断）；采用 60s **读间隔**超时（60s 无新数据报错，服务挂起仍可退出，用户也可点「停止生成」）。若 LLM 服务长时间不产出任何字节且不关闭连接，仍会按 60s 读间隔超时中止。
- **字段变更向量重建失败补偿（R5-P2-4）**：`set_field_value` 的字段值事务已提交后若向量重建失败，不阻塞业务（字段已保存）；失败经日志 + `app_meta reindex_pending_docs` 标记，由后台 `rebuild_missing_indexes` 下轮自动补建（补建成功移除标记）。极端情况（DB 级写入失败）下语义检索可能短暂陈旧，直至下轮补偿。
- **密钥存储 keyring 兜底（已实现，P2-11 风险标注）**：云端 API Key 仅经 OS keyring（`keyring` crate）存储，绝不落 SQLite 明文、绝不暴露前端。Linux 无 secret-service 守护时 keyring 失败，已降级用 `aidms_core::crypto`（Argon2id 派生密钥 + AES-256-GCM）加密写本地文件（home 下 `.aidms_keyring_fallback.b64`），**非明文**。⚠️ 兜底口令为「主机名 + 固定盐」派生（熵较低，攻击者拿到文件+主机名可离线爆破），仅作避免明文落盘的降级方案；高安全场景应安装 gnome-keyring/libsecret 走系统 keychain。
