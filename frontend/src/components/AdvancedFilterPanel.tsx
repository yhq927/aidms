/**
 * 高级筛选面板（R15）：时间范围 / 来源 / 负责人 / 多主体组合过滤。
 * 与三维筛选（主体×类型×标签）叠加使用；状态持久化于 useFilterStore。
 * UI 极简：折叠面板 + 原生控件（与项目现有 select/input 风格一致）。
 */
import { useState } from "react";
import { ChevronDown, ChevronUp, Filter } from "lucide-react";
import { cn } from "@/lib/utils";
import { useFilterStore } from "@/stores/useFilterStore";
import { useCatalogStore } from "@/stores/useCatalogStore";

const inputCls =
  "rounded-md border border-input bg-background px-2 py-1.5 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring";

export function AdvancedFilterPanel() {
  const [open, setOpen] = useState(false);

  const entityIds = useFilterStore((s) => s.entityIds);
  const dateFrom = useFilterStore((s) => s.dateFrom);
  const dateTo = useFilterStore((s) => s.dateTo);
  const source = useFilterStore((s) => s.source);
  const owner = useFilterStore((s) => s.owner);
  const setEntityIds = useFilterStore((s) => s.setEntityIds);
  const setDateFrom = useFilterStore((s) => s.setDateFrom);
  const setDateTo = useFilterStore((s) => s.setDateTo);
  const setSource = useFilterStore((s) => s.setSource);
  const setOwner = useFilterStore((s) => s.setOwner);

  const entities = useCatalogStore((s) => s.entities);

  const activeCount =
    (entityIds.length > 0 ? 1 : 0) +
    (dateFrom ? 1 : 0) +
    (dateTo ? 1 : 0) +
    (source ? 1 : 0) +
    (owner ? 1 : 0);

  function toggleEntity(id: number) {
    setEntityIds(
      entityIds.includes(id)
        ? entityIds.filter((x) => x !== id)
        : [...entityIds, id]
    );
  }

  return (
    <div className="space-y-2">
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        className="inline-flex items-center gap-1.5 rounded-md px-2 py-1 text-xs font-medium text-muted-foreground hover:bg-accent"
      >
        <Filter className="h-3.5 w-3.5" />
        高级筛选
        {activeCount > 0 && (
          <span className="rounded-full bg-primary/10 px-1.5 text-[11px] text-primary">
            {activeCount}
          </span>
        )}
        {open ? <ChevronUp className="h-3.5 w-3.5" /> : <ChevronDown className="h-3.5 w-3.5" />}
      </button>

      {open && (
        <div className="flex flex-wrap items-end gap-3 rounded-md border bg-card p-3">
          {/* 多主体（并集） */}
          <div className="space-y-1">
            <div className="text-xs text-muted-foreground">多主体（并集）</div>
            <div className="flex max-w-md flex-wrap gap-1.5">
              {entities.length === 0 && (
                <span className="text-xs text-muted-foreground">暂无主体</span>
              )}
              {entities.map((e) => (
                <button
                  key={e.id}
                  type="button"
                  onClick={() => toggleEntity(e.id)}
                  className={cn(
                    "rounded-full border px-2.5 py-0.5 text-xs transition-colors",
                    entityIds.includes(e.id)
                      ? "border-transparent bg-primary text-primary-foreground"
                      : "border-input bg-background hover:bg-accent"
                  )}
                >
                  {e.name}
                </button>
              ))}
            </div>
          </div>

          {/* 时间范围 */}
          <div className="flex items-center gap-1.5">
            <span className="text-xs text-muted-foreground">日期</span>
            <input
              type="date"
              value={dateFrom ?? ""}
              onChange={(e) => setDateFrom(e.target.value || null)}
              className={inputCls}
              title="起始日期（含当天）"
            />
            <span className="text-xs text-muted-foreground">至</span>
            <input
              type="date"
              value={dateTo ?? ""}
              onChange={(e) => setDateTo(e.target.value || null)}
              className={inputCls}
              title="截止日期（含当天）"
            />
          </div>

          {/* 来源（LIKE 模糊） */}
          <div className="flex items-center gap-1.5">
            <span className="text-xs text-muted-foreground">来源</span>
            <input
              value={source ?? ""}
              onChange={(e) => setSource(e.target.value || null)}
              placeholder="路径/来源关键字"
              className={cn(inputCls, "w-40")}
            />
          </div>

          {/* 负责人（精确） */}
          <div className="flex items-center gap-1.5">
            <span className="text-xs text-muted-foreground">负责人</span>
            <input
              value={owner ?? ""}
              onChange={(e) => setOwner(e.target.value || null)}
              placeholder="负责人姓名"
              className={cn(inputCls, "w-32")}
            />
          </div>

          {activeCount > 0 && (
            <button
              type="button"
              onClick={() => {
                setEntityIds([]);
                setDateFrom(null);
                setDateTo(null);
                setSource(null);
                setOwner(null);
              }}
              className="rounded-md px-2 py-1.5 text-xs text-muted-foreground hover:bg-accent"
            >
              清除高级条件
            </button>
          )}
        </div>
      )}
    </div>
  );
}
