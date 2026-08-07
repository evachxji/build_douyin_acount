import type { AssetListFilterArg, FilterState } from "../api/types";

export type SelectionAction =
  | { type: "toggle"; id: number }
  | { type: "replace"; ids: number[] }
  | { type: "clear" };

/** 选择集合的纯 reducer，便于单测。 */
export function selectionReducer(prev: Set<number>, action: SelectionAction): Set<number> {
  const next = new Set(prev);
  switch (action.type) {
    case "toggle": {
      if (next.has(action.id)) {
        next.delete(action.id);
      } else {
        next.add(action.id);
      }
      return next;
    }
    case "replace":
      return new Set(action.ids);
    case "clear":
      return new Set();
  }
}

/** 把前端筛选状态转换为后端查询参数（未设置的字段直接省略）。 */
export function buildAssetFilter(filter: FilterState): AssetListFilterArg {
  const out: AssetListFilterArg = {
    limit: 300,
    offset: 0,
  };
  // 左侧栏不单独展示"分析中"：待处理 = NEW + ANALYZING。
  if (filter.status) {
    out.statuses = filter.status === "NEW" ? ["NEW", "ANALYZING"] : [filter.status];
  }
  if (filter.platform) out.platform = filter.platform;
  if (filter.search?.trim()) out.search = filter.search.trim();
  return out;
}
