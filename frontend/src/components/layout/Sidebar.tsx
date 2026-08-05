import { useAssetsStore } from "../../stores/assetsStore";
import { statusMeta } from "../../lib/format";

const STATUS_ORDER = [
  "NEW",
  "ANALYZING",
  "CANDIDATE",
  "SELECTED",
  "PUBLISHED",
  "ELIMINATED",
  "DOWNLOAD_FAILED",
];

export function Sidebar() {
  const overview = useAssetsStore((s) => s.overview);
  const filter = useAssetsStore((s) => s.filter);
  const setFilter = useAssetsStore((s) => s.setFilter);

  const categoryActive = (name: string) =>
    (filter.category ?? "") === name;

  return (
    <aside className="w-56 shrink-0 overflow-y-auto border-r border-line bg-panel/60 p-3">
      <div className="mb-1 px-2 text-[11px] font-semibold uppercase tracking-wider text-mute">
        状态
      </div>
      <button
        onClick={() => void setFilter({ status: undefined })}
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
        const count = overview?.statuses.find((s) => s.status === status)?.count ?? 0;
        return (
          <button
            key={status}
            onClick={() => void setFilter({ status: filter.status === status ? undefined : status })}
            className={`mb-0.5 flex w-full items-center justify-between rounded px-2 py-1.5 text-left text-sm transition-colors ${
              filter.status === status ? "bg-accent/15 text-accent" : "text-paper/80 hover:bg-white/5"
            }`}
          >
            <span>{statusMeta(status).label}</span>
            <span className="tabular-nums text-mute">{count}</span>
          </button>
        );
      })}

      <div className="mb-1 mt-5 px-2 text-[11px] font-semibold uppercase tracking-wider text-mute">
        分类
      </div>
      <button
        onClick={() => void setFilter({ category: undefined })}
        className={`mb-0.5 flex w-full items-center justify-between rounded px-2 py-1.5 text-left text-sm transition-colors ${
          filter.category === undefined
            ? "bg-accent/15 text-accent"
            : "text-paper/80 hover:bg-white/5"
        }`}
      >
        <span>全部视频</span>
        <span className="tabular-nums text-mute">{overview?.total ?? 0}</span>
      </button>
      {(overview?.categories ?? []).map((c) => (
        <button
          key={c.name}
          onClick={() => void setFilter({ category: c.name })}
          className={`mb-0.5 flex w-full items-center justify-between rounded px-2 py-1.5 text-left text-sm transition-colors ${
            categoryActive(c.name) ? "bg-accent/15 text-accent" : "text-paper/80 hover:bg-white/5"
          }`}
        >
          <span className="truncate">{c.name}</span>
          <span className="tabular-nums text-mute">{c.count}</span>
        </button>
      ))}
    </aside>
  );
}
