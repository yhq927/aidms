/**
 * 资料（document）客户端：封装 list_documents_with_entities 命令。
 * 返回资料行 + 其归属主体 id 列表（entity_ids 为空即「未归类主体」）。
 * 仅 Tauri 运行时调用真实后端；浏览器/Vite dev 下走 mock，便于 UI 预览。
 */
import { invoke, isTauri } from "./tauri";

let mockNextId = 100;

export interface DocumentRow {
  id: number;
  kind: string;
  title: string;
  doc_type: string | null;
  source: string | null;
  content_text: string | null;
  party: string | null;
  owner: string | null;
  date_field: string | null;
  note: string | null;
  fields: string | null;
  status: string | null;
  sync_status: string | null;
  created_at: string;
  updated_at: string;
}

/** 资料 + 归属主体 id（前端「未归类主体」标示依据） */
export interface DocumentWithEntities extends DocumentRow {
  entity_ids: number[];
}

export interface DocumentFilterInput {
  entity_id?: number | null;
  doc_type?: string | null;
  tag_id?: number | null;
  // R15 高级筛选
  entity_ids?: number[];
  owner?: string | null;
  date_from?: string | null;
  date_to?: string | null;
  source?: string | null;
}

export async function listDocumentsWithEntities(
  f: DocumentFilterInput
): Promise<DocumentWithEntities[]> {
  if (!isTauri()) return mockDocuments(f);
  return invoke<DocumentWithEntities[]>("list_documents_with_entities", {
    entityId: f.entity_id ?? null,
    docType: f.doc_type ?? null,
    tagId: f.tag_id ?? null,
    entityIds: f.entity_ids ?? [],
    owner: f.owner ?? null,
    dateFrom: f.date_from ?? null,
    dateTo: f.date_to ?? null,
    source: f.source ?? null,
  });
}

// ---- 浏览器预览用 mock 数据（含 entity_ids 以演示未归类标示） ----
const allMock: DocumentWithEntities[] = [
  {
    id: 1, kind: "file", title: "重庆智习室 2024 年度财务报表", doc_type: "报表",
    source: "/data/import/重庆智习室_2024年报.pdf", content_text: "营业收入同比增长 12.4%",
    party: null, owner: "张三",
    date_field: "2024-12-31", note: null, fields: null, status: "ok",
    sync_status: null, created_at: "2026-03-01T00:00:00Z", updated_at: "2026-03-01T00:00:00Z",
    entity_ids: [1],
  },
  {
    id: 2, kind: "file", title: "双方合作框架协议", doc_type: "合同",
    source: "/data/import/合作框架协议.docx", content_text: "约定知识产权归属与保密义务",
    party: "上海数智", owner: "李四",
    date_field: "2025-06-01", note: null, fields: null, status: "ok",
    sync_status: null, created_at: "2026-03-02T00:00:00Z", updated_at: "2026-03-02T00:00:00Z",
    entity_ids: [2],
  },
  {
    id: 3, kind: "business", title: "内部会议纪要（待归类）", doc_type: "文书",
    source: null, content_text: "讨论下季度预算", party: null, owner: "张三",
    date_field: null, note: null, fields: null, status: "ok",
    sync_status: null, created_at: "2026-03-03T00:00:00Z", updated_at: "2026-03-03T00:00:00Z",
    entity_ids: [],
  },
  {
    id: 4, kind: "file", title: "营业执照副本扫描件", doc_type: "证件",
    source: "/data/scan/营业执照副本.jpg", content_text: "统一社会信用代码",
    party: null, owner: null,
    date_field: null, note: null, fields: null, status: "ok",
    sync_status: null, created_at: "2026-03-04T00:00:00Z", updated_at: "2026-03-04T00:00:00Z",
    entity_ids: [1, 2],
  },
];

function mockDocuments(f: DocumentFilterInput): DocumentWithEntities[] {
  return allMock.filter((d) => {
    if (f.entity_id != null && !d.entity_ids.includes(f.entity_id)) return false;
    if (f.entity_ids && f.entity_ids.length > 0 && !f.entity_ids.some((id) => d.entity_ids.includes(id))) return false;
    if (f.doc_type && d.doc_type !== f.doc_type) return false;
    if (f.owner && (d.owner ?? "") !== f.owner) return false;
    if (f.date_from && d.date_field && d.date_field < f.date_from) return false;
    if (f.date_to && d.date_field && d.date_field > f.date_to) return false;
    if (f.source && !(d.source ?? "").includes(f.source)) return false;
    return true;
  });
}

export interface NewDocumentInput {
  kind: string;
  title: string;
  doc_type?: string | null;
  source?: string | null;
  content_text?: string | null;
  party?: string | null;
  owner?: string | null;
  date_field?: string | null;
  note?: string | null;
  fields?: string | null;
  status?: string | null;
}

/**
 * ⚠️ 废弃（P2-3）：**勿用于业务条目/文件入库**——`create_document` 是纯 INSERT，
 * 会绕过 FTS5/chunk/向量索引，产生不可搜索的孤立行。
 * 业务条目请用 `submitParsed`（kind="business"），文件导入走 `importFiles`。
 * 当前无任何 UI 调用，保留仅为后端命令兼容（后端删除会影响历史调用，故保留并注释警示）。
 */
export async function createDocument(d: NewDocumentInput): Promise<number> {
  if (!isTauri()) return mockNextId++;
  return invoke<number>("create_document", {
    doc: {
      kind: d.kind,
      title: d.title,
      docType: d.doc_type ?? null,
      source: d.source ?? null,
      contentText: d.content_text ?? null,
      party: d.party ?? null,
      owner: d.owner ?? null,
      dateField: d.date_field ?? null,
      note: d.note ?? null,
      fields: d.fields ?? null,
      status: d.status ?? null,
      createdAt: new Date().toISOString(),
    },
  });
}

/** 删除资料（P2-1）：后端级联清理 FTS5/vec/chunk/归属/标签/关联/自定义字段值 */
export async function deleteDocument(id: number): Promise<void> {
  if (!isTauri()) return;
  return invoke<void>("delete_document", { id });
}

/**
 * 解析/业务条目统一回传入库（P1-A）：业务条目走此路径（而非 create_document 纯 INSERT），
 * 保证 FTS5/chunk/向量/RAG 上下文完整建立，提交后即可被搜索命中。
 * 契约与后端 ParsedInputPayload 一致（字段名 snake_case，无 serde 重命名）。
 */
export interface SubmitParsedInput {
  title: string;
  content_text: string;
  fields?: string | null;
  source?: string | null;
  /** 'file' | 'business' */
  kind: string;
  /** 'txt'|'pdf'|'docx'|'xlsx'|'image'|'business'|... */
  source_kind: string;
  entity_ids: number[];
  doc_type?: string | null;
  party?: string | null;
  owner?: string | null;
  date_field?: string | null;
  note?: string | null;
}

/** 提交解析结果/业务条目入库（submit_parsed 命令），返回新文档 id */
export async function submitParsed(p: SubmitParsedInput): Promise<number> {
  if (!isTauri()) return mockNextId++;
  return invoke<number>("submit_parsed", {
    input: {
      title: p.title,
      content_text: p.content_text,
      fields: p.fields ?? null,
      source: p.source ?? null,
      kind: p.kind,
      source_kind: p.source_kind,
      entity_ids: p.entity_ids ?? [],
      doc_type: p.doc_type ?? null,
      party: p.party ?? null,
      owner: p.owner ?? null,
      date_field: p.date_field ?? null,
      note: p.note ?? null,
    },
  });
}

/** 关联文档到主体（多对多） */
export async function linkEntity(documentId: number, entityId: number): Promise<void> {
  if (!isTauri()) return;
  return invoke<void>("link_entity", { documentId, entityId });
}

/** 取消文档与主体的关联（多对多） */
export async function unlinkEntity(documentId: number, entityId: number): Promise<void> {
  if (!isTauri()) return;
  return invoke<void>("unlink_entity", { documentId, entityId });
}

export type ExportFormat = "csv" | "json";

/**
 * 资料导出（R17）：按当前三维筛选导出 CSV/JSON 文本（含主体标注）。
 * 真实路径由后端生成文本；前端负责下载落盘。mock 路径本地生成等价文本便于预览。
 */
export async function exportDocuments(
  f: DocumentFilterInput,
  format: ExportFormat
): Promise<string> {
  if (!isTauri()) return mockExport(f, format);
  return invoke<string>("export_documents", {
    entityId: f.entity_id ?? null,
    docType: f.doc_type ?? null,
    tagId: f.tag_id ?? null,
    entityIds: f.entity_ids ?? [],
    owner: f.owner ?? null,
    dateFrom: f.date_from ?? null,
    dateTo: f.date_to ?? null,
    source: f.source ?? null,
    format,
  });
}

// mock：本地生成与后端等价的导出文本
function mockExport(f: DocumentFilterInput, format: ExportFormat): string {
  const rows = mockDocuments(f);
  if (format === "json") {
    return JSON.stringify(
      rows.map((r) => ({
        id: r.id,
        kind: r.kind,
        title: r.title,
        type: r.doc_type ?? "",
        source: r.source ?? "",
        party: r.party ?? "",
        owner: r.owner ?? "",
        date_field: r.date_field ?? "",
        note: r.note ?? "",
        status: r.status ?? "",
        created_at: r.created_at,
        entities: r.entity_ids.join(";"),
        tags: "",
      })),
      null,
      2
    );
  }
  const header = [
    "id", "kind", "title", "type", "source", "party", "owner",
    "date_field", "note", "status", "created_at", "entities", "tags",
  ];
  const esc = (v: string) =>
    /[",\n\r]/.test(v) ? `"${v.replace(/"/g, '""')}"` : v;
  const lines = [header.join(",")];
  for (const r of rows) {
    lines.push(
      [
        String(r.id), r.kind, r.title, r.doc_type ?? "", r.source ?? "",
        r.party ?? "", r.owner ?? "", r.date_field ?? "", r.note ?? "",
        r.status ?? "", r.created_at, r.entity_ids.join(";"), "",
      ]
        .map((c) => esc(c))
        .join(",")
    );
  }
  return lines.join("\n");
}
