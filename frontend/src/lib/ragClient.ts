/**
 * RAG 问答客户端：封装 ask_rag（Tauri Channel 流式）+ cancel_rag。
 *
 * 仅 Tauri 运行时调用真实后端（经 Rust 代理调 LLM，前端不直连）；浏览器/Vite dev 走 mock 流式。
 * 引用清单（[资料N] ↔ doc_id）经独立 Channel 回传，供引用卡点击跳转原文（P1-4）。
 */
import { invoke, isTauri } from "./tauri";
import { Channel } from "@tauri-apps/api/core";

export interface Citation {
  index: number;
  docId: number;
  title: string;
}

export interface AskParams {
  query: string;
  entity_ids?: number[] | null;
  doc_types?: string[] | null;
  tag_ids?: number[] | null;
  use_semantic?: boolean;
}

export interface AskHandlers {
  onToken: (t: string) => void;
  /** 引用清单（[资料N] ↔ doc_id），流式开始时即回传 */
  onCites?: (cites: Citation[]) => void;
  onDone?: () => void;
  onError?: (e: string) => void;
}

/** 发起流式问答。token 与引用清单分别经 Channel 回传。 */
export async function askRag(p: AskParams, h: AskHandlers): Promise<void> {
  if (!isTauri()) {
    h.onCites?.(mockCites);
    for (const t of mockStream(p.query)) h.onToken(t);
    h.onDone?.();
    return;
  }
  const tokenChannel = new Channel<string>();
  tokenChannel.onmessage = (msg: string) => h.onToken(msg);
  const citesChannel = new Channel<Citation[]>();
  citesChannel.onmessage = (cites: Citation[]) => h.onCites?.(cites);
  try {
    await invoke("ask_rag", {
      req: {
        query: p.query,
        entityIds: p.entity_ids ?? null,
        docTypes: p.doc_types ?? null,
        tagIds: p.tag_ids ?? null,
        useSemantic: p.use_semantic ?? false,
      },
      onToken: tokenChannel,
      onCites: citesChannel,
    });
    h.onDone?.();
  } catch (e) {
    // R5-P2-1：区分超时/网络错误，给出更友好的提示（原始信息附后便于排查）
    h.onError?.(friendlyRagError(e));
  }
}

/** 把 RAG 后端错误映射为友好提示（R5-P2-1：区分超时/网络错误） */
function friendlyRagError(e: unknown): string {
  const raw = e instanceof Error ? e.message : String(e);
  const s = raw.toLowerCase();
  // 流式读间隔超时（后端 60s 无新数据）或任何 timeout
  if (s.includes("流式读取超时") || s.includes("timeout") || s.includes("timed out")) {
    return "请求超时：LLM 服务响应过慢或无响应（超过 60 秒无新数据），请检查服务状态后重试";
  }
  // 连接类错误（拒绝连接 / DNS / 无法连接等）
  if (
    s.includes("connect") ||
    s.includes("connection") ||
    s.includes("refused") ||
    s.includes("dns") ||
    s.includes("网络") ||
    s.includes("无法连接") ||
    s.includes("unreachable")
  ) {
    return `网络错误：无法连接 LLM 服务，请检查 base_url 与网络（${raw}）`;
  }
  return raw;
}

/** 停止生成（翻转后端取消标志，Channel 关闭即终止流式） */
export function cancelRag(): void {
  if (!isTauri()) return;
  invoke("cancel_rag").catch(() => {});
}

/** 浏览器预览用模拟引用清单 */
const mockCites: Citation[] = [
  { index: 1, docId: 1, title: "重庆智习室 2024 年度财务报表" },
  { index: 2, docId: 2, title: "双方合作框架协议" },
];

/** 浏览器预览用模拟流式输出 */
function mockStream(q: string): string[] {
  const ans = `依据现有资料，关于「${q}」：甲公司 2024 年度营业收入同比增长 12.4%（见[资料1]）。合作协议约定双方知识产权归属与保密义务（见[资料2]）。`;
  return ans.match(/[\s\S]{1,6}/g) ?? [ans];
}
