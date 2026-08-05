import { create } from "zustand";
import { selectionReducer } from "./helpers";

interface SelectionState {
  /** 多选集合（批量操作使用）。 */
  ids: Set<number>;
  /** 详情面板聚焦的资产。 */
  focusId: number | null;
  focus: (id: number) => void;
  toggle: (id: number) => void;
  replace: (ids: number[]) => void;
  clear: () => void;
}

export const useSelectionStore = create<SelectionState>((set) => ({
  ids: new Set(),
  focusId: null,
  focus: (id) => set({ ids: new Set([id]), focusId: id }),
  toggle: (id) =>
    set((s) => ({ ids: selectionReducer(s.ids, { type: "toggle", id }) })),
  replace: (ids) => set({ ids: new Set(ids) }),
  clear: () => set({ ids: new Set() }),
}));
