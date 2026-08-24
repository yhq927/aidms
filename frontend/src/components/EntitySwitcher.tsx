/**
 * 主体切换器（顶栏）：把多主体作为一等维度，切换即改全局筛选状态（useFilterStore.entityId）。
 * ≤5 用 Badge ToggleGroup 平铺；超过 5 个时横向滚动 + 折叠提示（典型 ≤5 为软目标，不设硬上限）。
 */
import { useEffect } from "react";
import { Building2 } from "lucide-react";
import { cn } from "@/lib/utils";
import { useFilterStore } from "@/stores/useFilterStore";
import { useCatalogStore } from "@/stores/useCatalogStore";

const VISIBLE_LIMIT = 5;

export function EntitySwitcher() {
  const entityId = useFilterStore((s) => s.entityId);
  const setEntity = useFilterStore((s) => s.setEntity);
  const unclassified = useFilterStore((s) => s.unclassified);
  const setUnclassified = useFilterStore((s) => s.setUnclassified);
  const entities = useCatalogStore((s) => s.entities);
  const load = useCatalogStore((s) => s.load);

  useEffect(() => {
    load();
  }, [load]);

  const collapsed = entities.length > VISIBLE_LIMIT;
  const shown = collapsed ? entities.slice(0, VISIBLE_LIMIT) : entities;

  return (
    <div id="entity-switcher" className="flex items-center gap-2 overflow-hidden">
      <Building2 className="h-4 w-4 shrink-0 text-muted-foreground" />
      {/* 全部主体：清空 entityId / 未归类 */}
      <button
        onClick={() => {
          setEntity(null);
          setUnclassified(false);
        }}
        className={cn(
          "shrink-0 rounded-full border px-3 py-1 text-xs font-medium transition-colors",
          entityId == null && !unclassified
            ? "border-transparent bg-primary text-primary-foreground"
            : "border-input bg-background text-foreground hover:bg-accent"
        )}
        title="显示全部主体的资料"
      >
        全部主体
      </button>

      {/* 未归类（entity_ids 为空的文档，P2-2）：列表端过滤，见 Library.tsx */}
      <button
        onClick={() => setUnclassified(!unclassified)}
        className={cn(
          "shrink-0 rounded-full border px-3 py-1 text-xs font-medium transition-colors",
          unclassified
            ? "border-transparent bg-primary text-primary-foreground"
            : "border-input bg-background text-foreground hover:bg-accent"
        )}
        title="仅显示未归类主体的资料"
      >
        未归类
      </button>

      <div className="flex items-center gap-1.5 overflow-x-auto">
        {shown.map((e) => (
          <button
            key={e.id}
            onClick={() => setEntity(entityId === e.id ? null : e.id)}
            className={cn(
              "shrink-0 whitespace-nowrap rounded-full border px-3 py-1 text-xs font-medium transition-colors",
              entityId === e.id
                ? "border-transparent bg-primary text-primary-foreground"
                : "border-input bg-background text-foreground hover:bg-accent"
            )}
            title={e.name}
          >
            {e.name}
          </button>
        ))}
        {collapsed && (
          <span className="shrink-0 text-xs text-muted-foreground">
            +{entities.length - VISIBLE_LIMIT} 更多（侧栏「主体管理」查看）
          </span>
        )}
      </div>
    </div>
  );
}
