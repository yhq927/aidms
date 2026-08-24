import { invoke, isTauri } from "./tauri";
import { listDocumentsWithEntities, type DocumentWithEntities } from "./documentClient";

export interface DocLink {
  id: number;
  kind: string;
  direction: "out" | "in";
}

// mock 模式下的内存关联表（仅前端演示，真机走 Tauri 命令）
const mockLinks = new Map<number, DocLink[]>();

export async function listLinks(docId: number): Promise<DocLink[]> {
  if (!isTauri()) {
    return mockLinks.get(docId) ?? [];
  }
  return invoke<DocLink[]>("list_links", { docId });
}

export async function createLink(
  fromId: number,
  toId: number,
  kind: string
): Promise<void> {
  if (!isTauri()) {
    const arr = mockLinks.get(fromId) ?? [];
    arr.push({ id: toId, kind, direction: "out" });
    mockLinks.set(fromId, arr);
    return;
  }
  return invoke("create_link", { fromId, toId, kind });
}

export async function deleteLink(fromId: number, toId: number): Promise<void> {
  if (!isTauri()) {
    const arr = (mockLinks.get(fromId) ?? []).filter((l) => l.id !== toId);
    mockLinks.set(fromId, arr);
    return;
  }
  return invoke("delete_link", { fromId, toId });
}

/** 关联目标候选：全部文档（排除自身） */
export async function listAllDocs(): Promise<DocumentWithEntities[]> {
  if (!isTauri()) {
    return listDocumentsWithEntities({});
  }
  return listDocumentsWithEntities({});
}
