import { create } from "zustand";
import { persist } from "zustand/middleware";

/** 高级筛选：三维正交（主体 × 类型 × 标签）+ 日期范围 + 状态 + 关键词 + 多主体 + 来源/负责人（R15） */
interface FilterState {
  entityId: number | null; // 顶栏快捷主体（单选）
  docType: string | null;
  tagId: number | null;
  // R15 高级筛选扩展
  entityIds: number[]; // 多主体（与 entityId 取并集参与筛选）
  dateFrom: string | null;
  dateTo: string | null;
  status: string | null;
  keyword: string; // 资料库内文检索（非全库语义）
  source: string | null; // 来源（LIKE 模糊匹配）
  owner: string | null; // 负责人（精确匹配）
  /** 未归类快捷筛选（P2-2）：entity_ids 为空的文档；与 entityId/entityIds 互斥 */
  unclassified: boolean;
  setEntity: (id: number | null) => void;
  setType: (t: string | null) => void;
  setTag: (id: number | null) => void;
  setEntityIds: (ids: number[]) => void;
  toggleEntity: (id: number) => void;
  setDateFrom: (d: string | null) => void;
  setDateTo: (d: string | null) => void;
  setStatus: (s: string | null) => void;
  setKeyword: (k: string) => void;
  setSource: (s: string | null) => void;
  setOwner: (o: string | null) => void;
  setUnclassified: (b: boolean) => void;
  reset: () => void;
  /** 派生的全部生效主体（顶栏单选 + 多选） */
  activeEntityIds: () => number[];
}

export const useFilterStore = create<FilterState>()(
  persist(
    (set, get) => ({
      entityId: null,
      docType: null,
      tagId: null,
      entityIds: [],
      dateFrom: null,
      dateTo: null,
      status: null,
      keyword: "",
      source: null,
      owner: null,
      unclassified: false,
      setEntity: (id) => set({ entityId: id, unclassified: false }),
      setType: (t) => set({ docType: t }),
      setTag: (id) => set({ tagId: id }),
      setEntityIds: (ids) => set({ entityIds: ids, unclassified: false }),
      toggleEntity: (id) =>
        set((s) => ({
          entityIds: s.entityIds.includes(id)
            ? s.entityIds.filter((x) => x !== id)
            : [...s.entityIds, id],
          unclassified: false,
        })),
      setDateFrom: (d) => set({ dateFrom: d }),
      setDateTo: (d) => set({ dateTo: d }),
      setStatus: (s) => set({ status: s }),
      setKeyword: (k) => set({ keyword: k }),
      setSource: (s) => set({ source: s }),
      setOwner: (o) => set({ owner: o }),
      setUnclassified: (b) => set({ unclassified: b, entityId: null, entityIds: [] }),
      reset: () =>
        set({
          entityId: null,
          docType: null,
          tagId: null,
          entityIds: [],
          dateFrom: null,
          dateTo: null,
          status: null,
          keyword: "",
          source: null,
          owner: null,
          unclassified: false,
        }),
      activeEntityIds: () => {
        const s = get();
        const set = new Set<number>();
        if (s.entityId != null) set.add(s.entityId);
        s.entityIds.forEach((x) => set.add(x));
        return [...set];
      },
    }),
    { name: "aidms-filter" }
  )
);
