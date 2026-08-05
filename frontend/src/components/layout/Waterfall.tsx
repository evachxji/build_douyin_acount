import { useEffect, useRef, useState } from "react";
import { useAssetsStore } from "../../stores/assetsStore";
import { useSelectionStore } from "../../stores/selectionStore";
import { VideoCard } from "../VideoCard";

interface DragState {
  sx: number;
  sy: number;
  cx: number;
  cy: number;
}

export function Waterfall() {
  const assets = useAssetsStore((s) => s.assets);
  const loading = useAssetsStore((s) => s.loading);
  const error = useAssetsStore((s) => s.error);
  const reload = useAssetsStore((s) => s.load);
  const removeMany = useAssetsStore((s) => s.removeMany);
  const favoriteMany = useAssetsStore((s) => s.favoriteMany);
  const ids = useSelectionStore((s) => s.ids);
  const focusId = useSelectionStore((s) => s.focusId);
  const focus = useSelectionStore((s) => s.focus);
  const toggle = useSelectionStore((s) => s.toggle);
  const replace = useSelectionStore((s) => s.replace);
  const clear = useSelectionStore((s) => s.clear);

  const gridRef = useRef<HTMLDivElement | null>(null);
  const dragRef = useRef<DragState | null>(null);
  const [drag, setDrag] = useState<DragState | null>(null);

  // 框选结束：按矩形相交关系计算命中的卡片，Ctrl 按下时为追加，否则替换选择集。
  const finishDrag = (sx: number, sy: number, cx: number, cy: number, additive: boolean) => {
    const grid = gridRef.current;
    if (!grid) return;
    const g = grid.getBoundingClientRect();
    const left = Math.min(sx, cx) - g.left;
    const right = Math.max(sx, cx) - g.left;
    const top = Math.min(sy, cy) - g.top;
    const bottom = Math.max(sy, cy) - g.top;
    const matched: number[] = [];
    grid.querySelectorAll<HTMLElement>("[data-asset-id]").forEach((el) => {
      const r = el.getBoundingClientRect();
      if (
        r.left < g.left + right &&
        r.right > g.left + left &&
        r.top < g.top + bottom &&
        r.bottom > g.top + top
      ) {
        const id = Number(el.dataset.assetId);
        if (Number.isInteger(id)) matched.push(id);
      }
    });
    if (additive) {
      const next = new Set(useSelectionStore.getState().ids);
      matched.forEach((id) => next.add(id));
      replace([...next]);
    } else {
      replace(matched);
    }
  };

  useEffect(() => {
    if (!drag) return;
    const onMove = (e: MouseEvent) => {
      const d = dragRef.current;
      if (!d) return;
      const next = { ...d, cx: e.clientX, cy: e.clientY };
      dragRef.current = next;
      setDrag(next);
    };
    const onUp = (e: MouseEvent) => {
      const d = dragRef.current;
      dragRef.current = null;
      setDrag(null);
      if (d) finishDrag(d.sx, d.sy, e.clientX, e.clientY, e.ctrlKey || e.metaKey);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [drag !== null]);

  const handleBatchDelete = async () => {
    const list = [...ids].filter((id) => assets.some((a) => a.id === id));
    if (list.length === 0) return;
    if (!confirm(`确定删除选中的 ${list.length} 个视频？本地文件与封面将一并删除。`)) return;
    await removeMany(list);
    clear();
  };

  const handleFavoriteAll = async (favorite: boolean) => {
    await favoriteMany([...ids], favorite);
  };

  const marqueeStyle =
    drag && gridRef.current
      ? (() => {
          const g = gridRef.current.getBoundingClientRect();
          return {
            left: Math.min(drag.sx, drag.cx) - g.left,
            top: Math.min(drag.sy, drag.cy) - g.top,
            width: Math.abs(drag.cx - drag.sx),
            height: Math.abs(drag.cy - drag.sy),
          };
        })()
      : null;

  return (
    <div className="flex min-w-0 flex-1 flex-col">
      <main
        className="flex-1 select-none overflow-y-auto p-4"
        onMouseDown={(e) => {
          if (e.button !== 0) return;
          if (e.target instanceof Element && e.target.closest("[data-asset-id]")) return;
          if (e.target instanceof Element && e.target.closest("button, input, select, a")) return;
          const next = { sx: e.clientX, sy: e.clientY, cx: e.clientX, cy: e.clientY };
          dragRef.current = next;
          setDrag(next);
        }}
      >
        {error ? (
          <div className="flex h-full flex-col items-center justify-center gap-3 text-mute">
            <p className="text-sm">加载失败：{error}</p>
            <button
              onClick={() => void reload()}
              className="rounded border border-accent/50 px-3 py-1 text-sm text-accent hover:bg-accent/10"
            >
              重试
            </button>
          </div>
        ) : loading && assets.length === 0 ? (
          <div className="flex h-full items-center justify-center text-sm text-mute">
            加载素材中…
          </div>
        ) : assets.length === 0 ? (
          <div className="flex h-full items-center justify-center text-sm text-mute">
            暂无素材，点击右上角「导入」添加本地视频
          </div>
        ) : (
          <div
            ref={gridRef}
            className="relative grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-3 2xl:grid-cols-4"
          >
            {assets.map((asset) => (
              <VideoCard
                key={asset.id}
                asset={asset}
                selected={ids.has(asset.id)}
                focused={focusId === asset.id}
                onFocus={() => focus(asset.id)}
                onToggle={() => toggle(asset.id)}
              />
            ))}
            {marqueeStyle && (
              <div
                className="pointer-events-none absolute z-20 rounded border border-accent/70 bg-accent/10"
                style={marqueeStyle}
              />
            )}
          </div>
        )}
      </main>

      {ids.size > 0 && (
        <div className="flex h-12 shrink-0 items-center justify-between gap-3 border-t border-line bg-panel px-4">
          <span className="text-sm text-paper/85">已选中 {ids.size} 个视频</span>
          <div className="flex items-center gap-2">
            <button
              onClick={() => replace(assets.map((a) => a.id))}
              className="h-8 rounded border border-line px-3 text-sm text-mute transition-colors hover:border-accent/50 hover:text-paper"
            >
              全选
            </button>
            <button
              onClick={clear}
              className="h-8 rounded border border-line px-3 text-sm text-mute transition-colors hover:border-accent/50 hover:text-paper"
            >
              取消全选
            </button>
            <button
              onClick={() => void handleFavoriteAll(true)}
              className="h-8 rounded border border-accent/50 px-3 text-sm text-accent transition-colors hover:bg-accent/10"
            >
              收藏
            </button>
            <button
              onClick={() => void handleFavoriteAll(false)}
              className="h-8 rounded border border-line px-3 text-sm text-mute transition-colors hover:border-accent/50 hover:text-paper"
            >
              取消收藏
            </button>
            <button
              onClick={() => void handleBatchDelete()}
              className="h-8 rounded border border-bad/50 px-3 text-sm text-bad transition-colors hover:bg-bad/10"
            >
              删除
            </button>
            <button
              onClick={clear}
              className="h-8 rounded border border-line px-3 text-sm text-mute transition-colors hover:text-paper"
            >
              取消
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
