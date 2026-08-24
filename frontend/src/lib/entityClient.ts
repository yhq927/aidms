/**
 * 主体（entity）/ 标签（tag）客户端：封装阶段 6 实体 CRUD 命令。
 * 仅 Tauri 运行时调用真实后端；浏览器/Vite dev 下走 mock，便于 UI 预览。
 */
import { invoke, isTauri } from "./tauri";

export interface EntityRow {
  id: number;
  name: string;
  credit_code: string | null;
  note: string | null;
  created_at: string;
}

export interface TagRow {
  id: number;
  name: string;
}

export interface EntityInput {
  name: string;
  credit_code?: string | null;
  note?: string | null;
}

export async function listEntities(): Promise<EntityRow[]> {
  if (!isTauri()) return mockEntities.slice();
  return invoke<EntityRow[]>("list_entities");
}

export async function createEntity(input: EntityInput): Promise<number> {
  if (!isTauri()) {
    const id = mockEntities.length + 1;
    mockEntities.push({
      id,
      name: input.name,
      credit_code: input.credit_code ?? null,
      note: input.note ?? null,
      created_at: new Date().toISOString(),
    });
    return id;
  }
  return invoke<number>("create_entity", {
    input: {
      name: input.name,
      creditCode: input.credit_code ?? null,
      note: input.note ?? null,
    },
  });
}

export async function updateEntity(id: number, input: EntityInput): Promise<void> {
  if (!isTauri()) {
    const e = mockEntities.find((x) => x.id === id);
    if (e) {
      e.name = input.name;
      e.credit_code = input.credit_code ?? null;
      e.note = input.note ?? null;
    }
    return;
  }
  return invoke<void>("update_entity", {
    input: {
      id,
      name: input.name,
      creditCode: input.credit_code ?? null,
      note: input.note ?? null,
    },
  });
}

export async function deleteEntity(id: number): Promise<void> {
  if (!isTauri()) {
    const i = mockEntities.findIndex((x) => x.id === id);
    if (i >= 0) mockEntities.splice(i, 1);
    return;
  }
  return invoke<void>("delete_entity", { id });
}

export async function listTags(): Promise<TagRow[]> {
  if (!isTauri()) return mockTags.slice();
  return invoke<TagRow[]>("list_tags");
}

/**
 * 新建标签（P2-3 + R2 补齐）：名称重复时返回后端 UNIQUE 错误由前端 toast 提示。
 * mock 环境仅内存模拟。
 */
export async function createTag(name: string): Promise<number> {
  if (!isTauri()) {
    const id = mockTags.length + 1;
    mockTags.push({ id, name });
    return id;
  }
  return invoke<number>("create_tag", { input: { name } });
}

/** 给文档打标签（P2-3）：多对多，幂等 */
export async function addDocumentTag(documentId: number, tagId: number): Promise<void> {
  if (!isTauri()) return;
  return invoke<void>("add_document_tag", { documentId, tagId });
}

/** 取消文档标签（P2-3） */
export async function removeDocumentTag(documentId: number, tagId: number): Promise<void> {
  if (!isTauri()) return;
  return invoke<void>("remove_document_tag", { documentId, tagId });
}

/** 列出某文档的标签 id（P2-3：DocumentDrawer 展示当前已打标签） */
export async function listDocumentTags(documentId: number): Promise<number[]> {
  if (!isTauri()) return [];
  return invoke<number[]>("list_document_tags", { documentId });
}

// ---- 浏览器预览用 mock 数据 ----
const mockEntities: EntityRow[] = [
  { id: 1, name: "重庆智习室科技有限公司", credit_code: "91500000MA5X", note: "母公司", created_at: "2026-01-01T00:00:00Z" },
  { id: 2, name: "上海数智信息技术有限公司", credit_code: "91310000MA6Y", note: "子公司", created_at: "2026-02-01T00:00:00Z" },
];
const mockTags: TagRow[] = [
  { id: 1, name: "重要" },
  { id: 2, name: "年度" },
  { id: 3, name: "待审阅" },
];
