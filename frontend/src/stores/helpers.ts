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

const FILTER_STATUS_KEY = "bdya.assetFilter.status";

/** 读取上次退出前停留的状态筛选（无记录 = 全部）。storage 可注入便于测试。 */
export function loadPersistedFilterStatus(
  storage?: Pick<Storage, "getItem">,
): string | undefined {
  try {
    return (storage ?? localStorage).getItem(FILTER_STATUS_KEY) || undefined;
  } catch {
    return undefined;
  }
}

/** 持久化状态筛选；undefined（全部）时清除记录。localStorage 不可用时静默忽略。 */
export function persistFilterStatus(
  status: string | undefined,
  storage?: Pick<Storage, "setItem" | "removeItem">,
): void {
  try {
    const s = storage ?? localStorage;
    if (status) s.setItem(FILTER_STATUS_KEY, status);
    else s.removeItem(FILTER_STATUS_KEY);
  } catch {
    /* ignore */
  }
}
