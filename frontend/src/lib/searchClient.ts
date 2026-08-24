/**
 * 搜索客户端：封装 search_documents 命令调用（阶段 4 融合检索）。
 *
 * 仅 Tauri 运行时调用真实后端；浏览器/Vite dev 下走 mock，便于 UI 预览。
 * 所有返回片段含 `<mark>` 高亮，调用方须经 `sanitizeHighlight` 净化后注入（技术设计 §10）。
 */
import { invoke, isTauri } from "./tauri";

export type SearchMode = "keyword" | "semantic" | "hybrid";

export interface SearchHit {
  doc_id: number;
  title: string;
  /** 片段 HTML（含 <mark> 高亮），须净化后注入 */
  snippet: string;
  /** 融合 RRF 得分 */
  score: number;
  /** 是否主要由向量语义召回 */
  semantic: boolean;
  /** 归属主体 id 列表（为空即未归类主体） */
  entity_ids: number[];
}

export interface SearchParams {
  query: string;
  mode?: SearchMode;
  query_vec?: number[];
  entity_ids?: number[] | null;
  doc_types?: string[] | null;
  tag_ids?: number[] | null;
  limit?: number;
}

/** 调用后端融合检索（Rust 参数为结构体 req，键 camelCase，须整体包裹） */
export async function search(p: SearchParams): Promise<SearchHit[]> {
  if (!isTauri()) return mockSearch(p);
  return invoke<SearchHit[]>("search_documents", {
    req: {
      query: p.query,
      mode: p.mode ?? "hybrid",
      queryVec: p.query_vec ?? null,
      entityIds: p.entity_ids ?? null,
      docTypes: p.doc_types ?? null,
      tagIds: p.tag_ids ?? null,
      limit: p.limit ?? 30,
    },
  });
}

/** 非 Tauri 环境下的演示数据（含 <mark> 高亮，展示净化注入链路） */
function mockSearch(p: SearchParams): SearchHit[] {
  const q = p.query.trim();
  if (!q) return [];
  // P2-3：转义正则元字符（[.*+?^${}()|[\]\\]），防止特殊字符查询构造非法 RegExp 崩溃
  const escaped = q.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const mark = (text: string) =>
    text.replace(new RegExp(`(${escaped})`, "g"), "<mark>$1</mark>");
  return [
    {
      doc_id: 1,
      title: "重庆智习室科技有限公司 2024 年度财务报表",
      snippet: mark(
        "本报告涵盖资产负债与现金流分析，<script>alert(1)</script>营业收入同比增长 12.4%。"
      ),
      score: 1.83,
      semantic: p.mode === "semantic",
      entity_ids: [1],
    },
    {
      doc_id: 2,
      title: "双方合作框架协议",
      snippet: mark(
        "甲乙双方就长期合作框架达成一致，约定知识产权归属与保密义务。"
      ),
      score: 1.21,
      semantic: false,
      entity_ids: [2],
    },
  ];
}
