import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { ask } from "@tauri-apps/plugin-dialog";
import { api } from "../api/client";
import type { VideoAsset } from "../api/types";
import { coverUrl } from "../api/urls";
import { formatCount, formatDuration, platformLabel } from "../lib/format";
import { useAssetsStore } from "../stores/assetsStore";
import { useTaskStore } from "../stores/taskStore";
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
  const analyzeSelected = useAssetsStore((s) => s.analyzeSelected);
  const updateStatus = useAssetsStore((s) => s.updateStatus);
  const removeAsset = useAssetsStore((s) => s.removeAsset);
  const setTaskOpen = useTaskStore((s) => s.setOpen);
  const cover = coverUrl(asset.cover_path);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);

  // 菜单打开后按实际尺寸修正位置，避免超出窗口边缘。
  useLayoutEffect(() => {
    if (!menu || !menuRef.current) return;
    const rect = menuRef.current.getBoundingClientRect();
    if (rect.bottom > window.innerHeight) {
      menuRef.current.style.top = `${Math.max(8, window.innerHeight - rect.height - 8)}px`;
    }
    if (rect.right > window.innerWidth) {
      menuRef.current.style.left = `${Math.max(8, window.innerWidth - rect.width - 8)}px`;
    }
  }, [menu]);

  useEffect(() => {
    if (!menu) return;
    const close = (e: MouseEvent) => {
      if (e.type === "mousedown" && menuRef.current?.contains(e.target as Node)) return;
      setMenu(null);
    };
    const closeAll = () => setMenu(null);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setMenu(null);
    };
    window.addEventListener("mousedown", close);
    window.addEventListener("contextmenu", closeAll, true);
    window.addEventListener("blur", closeAll);
    window.addEventListener("resize", closeAll);
    window.addEventListener("scroll", closeAll, true);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("contextmenu", closeAll, true);
      window.removeEventListener("blur", closeAll);
      window.removeEventListener("resize", closeAll);
      window.removeEventListener("scroll", closeAll, true);
      window.removeEventListener("keydown", onKey);
    };
  }, [menu]);

  const handleOpenPath = async () => {
    setMenu(null);
    try {
      await api.revealInFolder(asset.file_path);
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
    }
  };

  const handleAnalyze = async () => {
    setMenu(null);
    try {
      await analyzeSelected([asset.id]);
      setTaskOpen(true);
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
    }
  };

  const handleGenerateCopy = async () => {
    setMenu(null);
    try {
      await api.generateCopy(asset.id);
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
    }
  };

  const handleEliminate = async () => {
    setMenu(null);
    try {
      await updateStatus(asset.id, "ELIMINATED");
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
    }
  };

  const handleDelete = async () => {
    setMenu(null);
    const ok = await ask(`确定删除视频 #${asset.id}？本地文件与封面将一并删除。`, {
      title: "删除确认",
      kind: "warning",
    });
    if (!ok) return;
    try {
      await removeAsset(asset.id);
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <article
      data-asset-id={asset.id}
      onClick={(e) => {
        if (e.ctrlKey || e.metaKey) onToggle();
        else onFocus();
      }}
      onContextMenu={(e) => {
        e.preventDefault();
        setMenu({ x: e.clientX, y: e.clientY });
      }}
      className={`group relative flex h-full cursor-pointer flex-col overflow-hidden rounded-lg border bg-panel transition-all duration-200 hover:-translate-y-0.5 hover:shadow-lg hover:shadow-black/40 ${
        focused ? "border-accent/70" : selected ? "border-accent/50" : "border-line hover:border-accent/40"
      }`}
    >
      <div className="relative aspect-video shrink-0 overflow-hidden bg-panel2">
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
          <span className="rounded bg-black/80 px-1.5 py-0.5 text-[10px] font-medium text-paper/95 backdrop-blur">
            {platformLabel(asset.source_platform)}
          </span>
          <StatusBadge status={asset.status} />
        </div>
        <div className="absolute right-2 top-2 flex items-center gap-1.5">
          <ScoreBadge score={asset.quality_score} />
        </div>
        <span className="absolute bottom-2 right-2 rounded bg-black/80 px-1.5 py-0.5 text-[10px] font-medium tabular-nums text-paper/95 backdrop-blur">
          {formatDuration(asset.duration_ms)}
        </span>
        {selected && (
          <span className="absolute left-2 bottom-2 flex h-5 w-5 items-center justify-center rounded bg-accent text-[11px] font-bold text-ink shadow">
            ✓
          </span>
        )}
      </div>

      <div className="flex h-29 flex-col overflow-hidden p-3">
        <h3 className="line-clamp-2 text-sm leading-snug text-paper">
          {asset.title ?? `视频 #${asset.id}`}
        </h3>
        <div className="mt-2 flex flex-nowrap gap-1 overflow-hidden">
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
            : "border-white/40 bg-black/80 text-paper/95 hover:border-accent/60"
        }`}
        title="多选切换"
      >
        选
      </button>

      {menu &&
        createPortal(
          <div
            ref={menuRef}
            onContextMenu={(e) => e.preventDefault()}
            className="fixed z-50 w-40 rounded-lg border border-line bg-panel p-1 shadow-2xl shadow-black/50"
            style={{
              left: Math.min(menu.x, window.innerWidth - 150),
              top: Math.min(menu.y, window.innerHeight - 190),
            }}
          >
            <button
              onClick={() => void handleOpenPath()}
              className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-xs text-paper/90 transition-colors hover:bg-accent/15 hover:text-accent"
            >
              打开路径
            </button>
            <button
              onClick={() => void handleAnalyze()}
              disabled={asset.status === "ANALYZING"}
              className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-xs text-paper/90 transition-colors hover:bg-accent/15 hover:text-accent disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent"
            >
              分析视频
            </button>
            <button
              onClick={() => void handleGenerateCopy()}
              className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-xs text-paper/90 transition-colors hover:bg-accent/15 hover:text-accent"
            >
              生成文案
            </button>
            <button
              onClick={() => void handleEliminate()}
              disabled={asset.status === "ELIMINATED" || asset.status === "PUBLISHED"}
              className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-xs text-paper/90 transition-colors hover:bg-accent/15 hover:text-accent disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent"
            >
              淘汰
            </button>
            <div className="my-1 border-t border-line" />
            <button
              onClick={() => void handleDelete()}
              className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-xs text-bad transition-colors hover:bg-bad/10"
            >
              删除
            </button>
          </div>,
          document.body,
        )}
    </article>
  );
}
