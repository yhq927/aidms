/**
 * 三维筛选器：主体(entity) × 类型(type) × 标签(tag) 正交组合（非树）。
 * 与全局 useFilterStore 共享状态，搜索 / 问答实时联动；「按公司」快速视图一键按主体过滤。
 */
import { useEffect } from "react";
import { X } from "lucide-react";
import { cn } from "@/lib/utils";
import { useFilterStore } from "@/stores/useFilterStore";
import { useCatalogStore } from "@/stores/useCatalogStore";
import { DOC_TYPES } from "@/lib/docTypes";

const selectCls =
  "rounded-md border border-input bg-background px-2 py-1.5 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring";

export function ThreeDimensionalFilter() {
  const entityId = useFilterStore((s) => s.entityId);
  const docType = useFilterStore((s) => s.docType);
  const tagId = useFilterStore((s) => s.tagId);
  const setEntity = useFilterStore((s) => s.setEntity);
  const setType = useFilterStore((s) => s.setType);
  const setTag = useFilterStore((s) => s.setTag);
  const reset = useFilterStore((s) => s.reset);

  const entities = useCatalogStore((s) => s.entities);
  const tags = useCatalogStore((s) => s.tags);
  const load = useCatalogStore((s) => s.load);
  useEffect(() => {
    load();
  }, [load]);

  const activeCount = [entityId, docType, tagId].filter(Boolean).length;

  return (
    <div className="space-y-2">
      {/* 主体 × 类型 × 标签 正交组合 */}
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-xs font-medium text-muted-foreground">筛选</span>
        <select
          value={entityId ?? ""}
          onChange={(e) => setEntity(e.target.value ? Number(e.target.value) : null)}
          className={selectCls}
        >
          <option value="">全部主体</option>
          {entities.map((e) => (
            <option key={e.id} value={e.id}>
              {e.name}
            </option>
          ))}
        </select>
        <select
          value={docType ?? ""}
          onChange={(e) => setType(e.target.value || null)}
          className={selectCls}
        >
          <option value="">全部类型</option>
          {DOC_TYPES.map((t) => (
            <option key={t} value={t}>
              {t}
            </option>
          ))}
        </select>
        <select
          value={tagId ?? ""}
          onChange={(e) => setTag(e.target.value ? Number(e.target.value) : null)}
          className={selectCls}
        >
          <option value="">全部标签</option>
          {tags.map((t) => (
            <option key={t.id} value={t.id}>
              {t.name}
            </option>
          ))}
        </select>
        {activeCount > 0 && (
          <button
            onClick={reset}
            className="inline-flex items-center gap-1 rounded-md px-2 py-1.5 text-xs text-muted-foreground hover:bg-accent"
          >
            <X className="h-3 w-3" /> 清除（{activeCount}）
          </button>
        )}
      </div>

      {/* 「按公司」快速视图（非树）：一键按主体过滤 */}
      {entities.length > 0 && (
        <div className="flex flex-wrap items-center gap-1.5">
          <span className="text-xs text-muted-foreground">按公司</span>
          {entities.map((e) => (
            <button
              key={e.id}
              onClick={() => setEntity(entityId === e.id ? null : e.id)}
              className={cn(
                "rounded-full border px-2.5 py-0.5 text-xs transition-colors",
                entityId === e.id
                  ? "border-transparent bg-primary text-primary-foreground"
                  : "border-input bg-background text-foreground hover:bg-accent"
              )}
            >
              {e.name}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
