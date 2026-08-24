import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { ChatCard, type ChatMessage } from "@/components/ChatCard";
import { DocumentDrawer } from "@/components/DocumentDrawer";
import { askRag, cancelRag, type Citation } from "@/lib/ragClient";
import { getLlmConfig } from "@/lib/configClient";
import { isTauri } from "@/lib/tauri";
import { useFilterStore } from "@/stores/useFilterStore";

export default function QA() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [streaming, setStreaming] = useState(false);
  // null=加载中；true/false=已启用/未启用（未启用时发送禁用并提示，P1-4）
  const [llmEnabled, setLlmEnabled] = useState<boolean | null>(null);
  const [drawerDoc, setDrawerDoc] = useState<{ docId: number; title: string } | null>(null);
  // P2-4：RAG 检索上下文补传 docType/tagId/activeEntityIds（useFilterStore 已有）
  const entityId = useFilterStore((s) => s.entityId);
  const entityIds = useFilterStore((s) => s.entityIds);
  const docType = useFilterStore((s) => s.docType);
  const tagId = useFilterStore((s) => s.tagId);

  useEffect(() => {
    if (!isTauri()) {
      setLlmEnabled(true); // mock 环境允许演示
      return;
    }
    getLlmConfig()
      .then((c) => setLlmEnabled(c.enabled))
      .catch(() => setLlmEnabled(false));
  }, []);

  async function send() {
    const q = input.trim();
    if (!q || streaming) return;
    setInput("");
    setStreaming(true);
    const userMsg: ChatMessage = { role: "user", content: q };
    const aiIndex = messages.length + 1;
    setMessages((m) => [...m, userMsg, { role: "assistant", content: "", streaming: true }]);

    await askRag(
      {
        query: q,
        // P2-8：llmEnabled===true（已配置并启用 AI）时传 useSemantic:true，
        // 后端检索走融合模式（向量不可达自动降级全文，安全）。
        use_semantic: llmEnabled === true,
        // P2-4：主体约束用「顶栏单选 + 多选」并集；类型/标签一并传给检索上下文
        entity_ids: entityId != null || entityIds.length > 0
          ? [...new Set([...(entityId != null ? [entityId] : []), ...entityIds])]
          : null,
        doc_types: docType ? [docType] : null,
        tag_ids: tagId != null ? [tagId] : null,
      },
      {
        onToken: (t) =>
          setMessages((m) => {
            const next = [...m];
            next[aiIndex] = {
              ...next[aiIndex],
              content: next[aiIndex].content + t,
            };
            return next;
          }),
        onCites: (cites: Citation[]) =>
          setMessages((m) => {
            const next = [...m];
            next[aiIndex] = { ...next[aiIndex], cites };
            return next;
          }),
        onDone: () =>
          setMessages((m) => {
            const next = [...m];
            next[aiIndex] = { ...next[aiIndex], streaming: false };
            return next;
          }),
        onError: (e) =>
          setMessages((m) => {
            const next = [...m];
            next[aiIndex] = {
              role: "assistant",
              content: next[aiIndex].content + `\n\n[出错] ${e}`,
              streaming: false,
            };
            return next;
          }),
      }
    );
    setStreaming(false);
  }

  function stop() {
    cancelRag();
    setStreaming(false);
    setMessages((m) => {
      const next = [...m];
      const last = next[next.length - 1];
      if (last?.role === "assistant") next[next.length - 1] = { ...last, streaming: false };
      return next;
    });
  }

  function openDoc(docId: number) {
    // 从当前消息引用清单找标题（找不到则回退 #docId）
    const msg = messages.find((m) => m.cites?.some((c) => c.docId === docId));
    const title =
      msg?.cites?.find((c) => c.docId === docId)?.title ?? `资料 #${docId}`;
    setDrawerDoc({ docId, title });
  }

  return (
    <div className="flex h-full flex-col space-y-3">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold">AI 问答</h1>
        <span className="text-xs text-muted-foreground">
          {entityId || entityIds.length > 0
            ? "仅基于当前筛选主体资料"
            : docType || tagId != null
              ? "按类型/标签筛选资料"
              : "基于全部主体资料"}
        </span>
      </div>

      <div className="flex-1 space-y-3 overflow-y-auto rounded-md border p-3">
        {messages.length === 0 && (
          <p className="text-sm text-muted-foreground">
            配置并启用 AI 后，可就资料提问。回答仅基于检索到的资料并标注出处 [资料N]，点击出处可跳转原文。
          </p>
        )}
        {messages.map((m, i) => (
          <ChatCard key={i} msg={m} onOpenDoc={openDoc} />
        ))}
      </div>

      {llmEnabled === false && (
        <Badge variant="warning" className="w-fit">
          未配置/未启用 AI：无法问答，请先到「配置」页设置并启用（全文检索仍可用）
        </Badge>
      )}

      <div className="flex gap-2">
        <input
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && llmEnabled !== false && send()}
          placeholder="就你的资料提问…"
          className="flex-1 rounded-md border bg-background px-3 py-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
        />
        {streaming ? (
          <Button variant="destructive" onClick={stop}>
            停止生成
          </Button>
        ) : (
          <Button onClick={send} disabled={!input.trim() || llmEnabled === false}>
            发送
          </Button>
        )}
      </div>

      <DocumentDrawer
        open={!!drawerDoc}
        docId={drawerDoc?.docId ?? 0}
        title={drawerDoc?.title ?? ""}
        onClose={() => setDrawerDoc(null)}
      />
    </div>
  );
}
