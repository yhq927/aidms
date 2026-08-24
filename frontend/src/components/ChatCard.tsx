import { Card, CardContent } from "@/components/ui/card";
import type { Citation } from "@/lib/ragClient";

export interface ChatMessage {
  role: "user" | "assistant";
  content: string;
  streaming?: boolean;
  /** 引用清单（[资料N] ↔ doc_id，后端 on_cites 回传；P1-4） */
  cites?: Citation[];
}

/** 从回答文本提取 [资料N] 引用编号 */
export function extractCitations(text: string): number[] {
  const set = new Set<number>();
  const re = /\[资料(\d+)\]/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) set.add(Number(m[1]));
  return [...set].sort((a, b) => a - b);
}

export function ChatCard({
  msg,
  onOpenDoc,
}: {
  msg: ChatMessage;
  onOpenDoc?: (docId: number) => void;
}) {
  if (msg.role === "user") {
    return (
      <div className="flex justify-end">
        <div className="max-w-[80%] whitespace-pre-wrap rounded-lg bg-primary px-3 py-2 text-sm text-primary-foreground">
          {msg.content}
        </div>
      </div>
    );
  }
  const cites = msg.cites ?? [];
  const citeMap = new Map(cites.map((c) => [c.index, c]));
  // 以文本中的 [资料N] 为准（模型可能未引用全部检索项），引用清单提供 doc_id 映射
  const textCites = extractCitations(msg.content);
  return (
    <Card>
      <CardContent className="whitespace-pre-wrap text-sm leading-relaxed">
        {msg.content}
        {msg.streaming && (
          <span className="ml-1 inline-block h-4 w-1.5 translate-y-0.5 animate-pulse bg-foreground/60" />
        )}
      </CardContent>
      {textCites.length > 0 && (
        <CardContent className="flex flex-wrap gap-1 pt-0">
          {textCites.map((idx) => {
            const c = citeMap.get(idx);
            return (
              <button
                key={idx}
                type="button"
                disabled={!c}
                onClick={() => c && onOpenDoc?.(c.docId)}
                title={c ? `打开「${c.title}」` : "引用资料"}
                className={
                  "inline-flex items-center rounded-full border px-2.5 py-0.5 text-xs font-semibold transition-colors " +
                  (c
                    ? "cursor-pointer border-transparent bg-info text-info-foreground hover:opacity-80"
                    : "cursor-default border-transparent bg-info/60 text-info-foreground")
                }
              >
                [资料{idx}]
              </button>
            );
          })}
        </CardContent>
      )}
    </Card>
  );
}
