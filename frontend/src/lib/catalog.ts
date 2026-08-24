/** 目录辅助：按 id 反查实体名称（用于资料卡展示归属主体） */
import { useCatalogStore } from "@/stores/useCatalogStore";

export function useEntityNameMap(): Map<number, string> {
  const entities = useCatalogStore((s) => s.entities);
  const map = new Map<number, string>();
  for (const e of entities) map.set(e.id, e.name);
  return map;
}
