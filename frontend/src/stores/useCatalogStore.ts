/**
 * 目录 store：承载实体（主体）与标签两种「选择源」数据。
 * 数据来自后端（非持久化到本地，刷新即从 DB 重载），供切换器 / 三维筛选 / 导入选择器消费。
 */
import { create } from "zustand";
import { listEntities, listTags, type EntityRow, type TagRow } from "@/lib/entityClient";

interface CatalogState {
  entities: EntityRow[];
  tags: TagRow[];
  loading: boolean;
  load: () => Promise<void>;
  addEntityLocal: (e: EntityRow) => void;
  updateEntityLocal: ((id: number, patch: Partial<EntityRow>) => void);
  removeEntityLocal: (id: number) => void;
}

export const useCatalogStore = create<CatalogState>((set, get) => ({
  entities: [],
  tags: [],
  loading: false,
  load: async () => {
    if (get().loading) return;
    set({ loading: true });
    try {
      const [entities, tags] = await Promise.all([listEntities(), listTags()]);
      set({ entities, tags });
    } finally {
      set({ loading: false });
    }
  },
  addEntityLocal: (e) => set((s) => ({ entities: [...s.entities, e] })),
  updateEntityLocal: (id, patch) =>
    set((s) => ({
      entities: s.entities.map((e) => (e.id === id ? { ...e, ...patch } : e)),
    })),
  removeEntityLocal: (id) =>
    set((s) => ({ entities: s.entities.filter((e) => e.id !== id) })),
}));
