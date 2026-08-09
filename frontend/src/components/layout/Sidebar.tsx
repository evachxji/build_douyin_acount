import type { FilterState } from "../../api/types";
import { useAssetsStore } from "../../stores/assetsStore";
import { useSelectionStore } from "../../stores/selectionStore";
import { statusMeta } from "../../lib/format";

// 分析中不单独展示，归入"待处理"（NEW）；下载失败不展示。
const STATUS_ORDER = [
  "NEW",
  "CANDIDATE",
  "SELECTED",
  "PUBLISHED",
  "ELIMINATED",
];

export function Sidebar({ width }: { width: number }) {
  const overview = useAssetsStore((s) => s.overview);
  const filter = useAssetsStore((s) => s.filter);
  const setFilter = useAssetsStore((s) => s.setFilter);

  // 状态分类切换后默认聚焦新列表第一个视频（列表为空则不跳转）。
  const applyFilter = async (patch: Partial<FilterState>) => {
    await setFilter(patch);
    const first = useAssetsStore.getState().assets[0];
    if (first) useSelectionStore.getState().focus(first.id);
  };

  const statusCount = (status: string) => {
    if (status === "NEW") {
      return (overview?.statuses.find((s) => s.status === "NEW")?.count ?? 0) +
        (overview?.statuses.find((s) => s.status === "ANALYZING")?.count ?? 0);
    }
    return overview?.statuses.find((s) => s.status === status)?.count ?? 0;
  };

  return (
    <aside
      style={{ width }}
      className="shrink-0 overflow-y-auto border-r border-line bg-panel/60 p-3"
    >
      <div className="mb-1 px-2 text-[11px] font-semibold uppercase tracking-wider text-mute">
        状态
      </div>
      <button
        onClick={() => void applyFilter({ status: undefined })}
        className={`mb-0.5 flex w-full items-center justify-between rounded px-2 py-1.5 text-left text-sm transition-colors ${
          filter.status === undefined
            ? "bg-accent/15 text-accent"
            : "text-paper/80 hover:bg-white/5"
        }`}
      >
        <span>全部</span>
        <span className="tabular-nums text-mute">{overview?.total ?? 0}</span>
      </button>
      {STATUS_ORDER.map((status) => {
        const count = statusCount(status);
        return (
          <button
            key={status}
            onClick={() => void applyFilter({ status: filter.status === status ? undefined : status })}
            className={`mb-0.5 flex w-full items-center justify-between rounded px-2 py-1.5 text-left text-sm transition-colors ${
              filter.status === status ? "bg-accent/15 text-accent" : "text-paper/80 hover:bg-white/5"
            }`}
          >
            <span>{statusMeta(status).label}</span>
            <span className="tabular-nums text-mute">{count}</span>
          </button>
        );
      })}
    </aside>
  );
}
