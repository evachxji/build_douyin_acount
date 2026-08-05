import type { VideoAsset } from "../api/types";
import { coverUrl } from "../api/urls";
import { formatCount, formatDuration, platformLabel } from "../lib/format";
import { ScoreBadge } from "./ScoreBadge";
import { StatusBadge } from "./StatusBadge";

interface Props {
  asset: VideoAsset;
  selected: boolean;
  focused: boolean;
  onFocus: () => void;
  onToggle: () => void;
}

export function VideoCard({ asset, selected, focused, onFocus, onToggle }: Props) {
  const cover = coverUrl(asset.cover_path);
  return (
    <article
      onClick={onFocus}
      className={`group relative mb-4 break-inside-avoid cursor-pointer overflow-hidden rounded-lg border bg-panel transition-all duration-200 hover:-translate-y-0.5 hover:shadow-lg hover:shadow-black/40 ${
        focused ? "border-accent/70" : selected ? "border-accent/50" : "border-line hover:border-accent/40"
      }`}
    >
      <div className="relative aspect-video overflow-hidden bg-panel2">
        {cover ? (
          <img
            src={cover}
            alt={asset.title ?? "视频封面"}
            loading="lazy"
            className="h-full w-full object-cover transition-transform duration-300 group-hover:scale-[1.03]"
          />
        ) : (
          <div className="flex h-full w-full items-center justify-center text-4xl text-mute/40">
            ▶
          </div>
        )}
        <div className="absolute left-2 top-2 flex items-center gap-1.5">
          <span className="rounded bg-black/60 px-1.5 py-0.5 text-[10px] text-paper/90 backdrop-blur">
            {platformLabel(asset.source_platform)}
          </span>
          <StatusBadge status={asset.status} />
        </div>
        <div className="absolute right-2 top-2 flex items-center gap-1.5">
          <ScoreBadge score={asset.quality_score} />
          {asset.is_favorite && <span className="text-accent text-sm leading-none">♥</span>}
        </div>
        <span className="absolute bottom-2 right-2 rounded bg-black/60 px-1.5 py-0.5 text-[10px] tabular-nums text-paper/90 backdrop-blur">
          {formatDuration(asset.duration_ms)}
        </span>
        {selected && (
          <span className="absolute left-2 bottom-2 flex h-5 w-5 items-center justify-center rounded bg-accent text-[11px] font-bold text-ink shadow">
            ✓
          </span>
        )}
      </div>

      <div className="p-3">
        <h3 className="line-clamp-2 text-sm leading-snug text-paper">
          {asset.title ?? `视频 #${asset.id}`}
        </h3>
        <div className="mt-2 flex flex-wrap gap-1">
          {[asset.category, ...(asset.tags ?? [])]
            .filter(Boolean)
            .slice(0, 4)
            .map((tag) => (
              <span
                key={tag}
                className="rounded bg-white/5 px-1.5 py-0.5 text-[10px] text-mute"
              >
                {tag}
              </span>
            ))}
        </div>
        <div className="mt-2 flex items-center justify-between text-[11px] text-mute">
          <span>{asset.author ?? "未知作者"}</span>
          <span className="tabular-nums">
            ♥ {formatCount(asset.likes)} · {formatCount(asset.comments)} 评
          </span>
        </div>
      </div>

      {/* 多选开关：阻止冒泡，只切换选择 */}
      <button
        onClick={(e) => {
          e.stopPropagation();
          onToggle();
        }}
        className={`absolute right-2 top-9 rounded border px-1.5 py-0.5 text-[10px] transition-colors ${
          selected
            ? "border-accent bg-accent text-ink"
            : "border-white/20 bg-black/50 text-paper/80 hover:border-accent/60"
        }`}
        title="多选切换"
      >
        选
      </button>
    </article>
  );
}
