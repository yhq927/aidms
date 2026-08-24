import { useState, useEffect, useCallback } from "react";
import { Link2, FileText, List, LayoutGrid } from "lucide-react";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";
import { sanitizeHighlight } from "@/lib/sanitize";
import { search, type SearchMode, type SearchHit } from "@/lib/searchClient";
import { getLlmConfig } from "@/lib/configClient";
import { isTauri } from "@/lib/tauri";
import { useFilterStore } from "@/stores/useFilterStore";
import { useEntityNameMap } from "@/lib/catalog";
import { ThreeDimensionalFilter } from "@/components/ThreeDimensionalFilter";
import { UnclassifiedBadge } from "@/components/UnclassifiedBadge";
import { DocumentDrawer } from "@/components/DocumentDrawer";

const MODES: { key: SearchMode; label: string }[] = [
  { key: "keyword", label: "全文" },
  { key: "hybrid", label: "融合" },
  { key: "semantic", label: "语义" },
];

/** 需要嵌入模型的模式（未配置时禁用，PRD §6.5.3 / P0-2） */
const NEEDS_EMBED: SearchMode[] = ["hybrid", "semantic"];

export default function Search() {
  const [q, setQ] = useState("");
  const [mode, setMode] = useState<SearchMode>("hybrid");
  const [results, setResults] = useState<SearchHit[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [drawerId, setDrawerId] = useState<number | null>(null);
  // null=加载中；true/false=已配置/未配置（未配置时禁用语义/融合）
  const [llmEnabled, setLlmEnabled] = useState<boolean | null>(null);
  // 视图：list 列表 / card 卡片（P2-7）
  const [view, setView] = useState<"list" | "card">("list");
  const drawerHit = results.find((h) => h.doc_id === drawerId) ?? null;

  const docType = useFilterStore((s) => s.docType);
  const entityId = useFilterStore((s) => s.entityId);
  const tagId = useFilterStore((s) => s.tagId);
  const entityNames = useEntityNameMap();

  // 进入页面查询 AI 配置状态：未配置时语义/融合模式禁用并提示先配置（P0-2）
  useEffect(() => {
    if (!isTauri()) {
      setLlmEnabled(true); // mock 环境允许语义演示
      return;
    }
    getLlmConfig()
      .then((c) => setLlmEnabled(c.enabled))
      .catch(() => setLlmEnabled(false));
  }, []);

  // 未配置时不允许停留在语义/融合模式（自动回退全文）
  useEffect(() => {
    if (llmEnabled === false && NEEDS_EMBED.includes(mode)) {
      setMode("keyword");
    }
  }, [llmEnabled, mode]);

  // 筛选维度变化时，若已有查询则实时重搜（三维筛选与结果联动）
  const run = useCallback(async () => {
    const query = q.trim();
    if (!query) return;
    setLoading(true);
    setError(null);
    try {
      const hits = await search({
        query,
        mode,
        doc_types: docType ? [docType] : null,
        entity_ids: entityId ? [entityId] : null,
        tag_ids: tagId ? [tagId] : null,
      });
      setResults(hits);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setResults([]);
    } finally {
      setLoading(false);
    }
  }, [q, mode, docType, entityId, tagId]);

  useEffect(() => {
    if (q.trim()) run();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [docType, entityId, tagId]);

  return (
    <div className="space-y-4">
      <h1 className="text-2xl font-semibold">搜索</h1>

      <div className="flex gap-2">
        <input
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && run()}
          placeholder="关键词 / 自然语言…"
          autoFocus
          className="flex-1 rounded-md border bg-background px-3 py-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
        />
        <Button onClick={run} disabled={loading}>
          {loading ? "检索中…" : "搜索"}
        </Button>
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <div className="flex rounded-md border p-0.5">
          {MODES.map((m) => {
            // P2-5：llmEnabled===null（配置加载中）时语义/融合同样禁用，避免误用未就绪的嵌入
            const disabled =
              (llmEnabled === false || llmEnabled === null) && NEEDS_EMBED.includes(m.key);
            return (
              <button
                key={m.key}
                onClick={() => {
                  if (disabled) {
                    if (llmEnabled === null) return; // 配置加载中：忽略点击
                    setError("未配置 AI 模型，请先到「配置」页设置并启用");
                    return;
                  }
                  setMode(m.key);
                }}
                title={
                  disabled
                    ? llmEnabled === null
                      ? "AI 配置加载中…"
                      : "未配置 AI 模型（请先到配置页设置并启用）"
                    : undefined
                }
                className={
                  "rounded px-3 py-1 text-sm transition-colors " +
                  (disabled
                    ? "cursor-not-allowed text-muted-foreground/50"
                    : mode === m.key
                      ? "bg-primary text-primary-foreground"
                      : "text-muted-foreground hover:text-foreground")
                }
              >
                {m.label}
              </button>
            );
          })}
        </div>
        <span className="text-xs text-muted-foreground">
          {entityId ? "已按主体范围检索" : "全部主体"}
        </span>
        {llmEnabled === false && (
          <Badge variant="outline" title="到「配置」页设置 AI 后可启用语义/融合">
            未配置 AI：语义/融合已禁用
          </Badge>
        )}
        {/* 列表 / 卡片视图切换（P2-7） */}
        <div className="ml-auto flex rounded-md border p-0.5">
          <button
            onClick={() => setView("list")}
            title="列表视图"
            className={cn(
              "rounded p-1 transition-colors",
              view === "list" ? "bg-primary text-primary-foreground" : "text-muted-foreground hover:text-foreground"
            )}
          >
            <List className="h-4 w-4" />
          </button>
          <button
            onClick={() => setView("card")}
            title="卡片视图"
            className={cn(
              "rounded p-1 transition-colors",
              view === "card" ? "bg-primary text-primary-foreground" : "text-muted-foreground hover:text-foreground"
            )}
          >
            <LayoutGrid className="h-4 w-4" />
          </button>
        </div>
      </div>

      <ThreeDimensionalFilter />

      {error && (
        <Badge variant="error" className="mt-2">
          {error}
        </Badge>
      )}

      <div className={view === "card" ? "grid grid-cols-2 gap-3" : "space-y-3"}>
        {results.length === 0 && !loading && (
          <Card className={view === "card" ? "col-span-2" : undefined}>
            <CardContent className="text-muted-foreground">
              {llmEnabled === false
                ? "未配置 AI 模型：语义/融合已禁用，请先到「配置」页设置并启用。当前仅可全文检索。"
                : "暂无结果。未配置 LLM 时语义检索自动降级为全文，仍可作关键词检索。"}
            </CardContent>
          </Card>
        )}
        {results.map((h) => (
          <Card key={h.doc_id} className={view === "card" ? "overflow-hidden" : undefined}>
            {view === "card" && (
              <div className="flex aspect-video items-center justify-center border-b bg-muted/40 text-muted-foreground">
                <FileText className="h-7 w-7" />
              </div>
            )}
            <CardHeader className="pb-1">
              <div className="flex items-center justify-between gap-2">
                <CardTitle className={cn("text-base", view === "card" && "truncate")}>
                  {h.title}
                </CardTitle>
                <div className="flex shrink-0 items-center gap-1.5">
                  {h.semantic && (
                    <Badge variant="info">语义</Badge>
                  )}
                  {h.entity_ids.length === 0 ? (
                    <UnclassifiedBadge />
                  ) : (
                    h.entity_ids.slice(0, 2).map((id) => (
                      <Badge key={id} variant="secondary">
                        {entityNames.get(id) ?? `主体 #${id}`}
                      </Badge>
                    ))
                  )}
                </div>
              </div>
            </CardHeader>
            <CardContent>
              <p
                className={cn(
                  "text-sm leading-relaxed text-muted-foreground",
                  view === "card" && "line-clamp-3"
                )}
                dangerouslySetInnerHTML={{ __html: sanitizeHighlight(h.snippet) }}
              />
              <div className="mt-2 flex items-center justify-between">
                <span className="text-xs text-muted-foreground">
                  相关度 {h.score.toFixed(2)}
                </span>
                <Button variant="ghost" size="sm" onClick={() => setDrawerId(h.doc_id)}>
                  <Link2 className="h-4 w-4" /> 关联
                </Button>
              </div>
            </CardContent>
          </Card>
        ))}
      </div>

      <DocumentDrawer
        open={!!drawerHit}
        docId={drawerHit?.doc_id ?? 0}
        title={drawerHit?.title ?? ""}
        entityIds={drawerHit?.entity_ids ?? []}
        onClose={() => setDrawerId(null)}
      />
    </div>
  );
}
