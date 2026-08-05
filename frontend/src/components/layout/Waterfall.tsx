import { useAssetsStore } from "../../stores/assetsStore";
import { useSelectionStore } from "../../stores/selectionStore";
import { VideoCard } from "../VideoCard";

export function Waterfall() {
  const assets = useAssetsStore((s) => s.assets);
  const loading = useAssetsStore((s) => s.loading);
  const error = useAssetsStore((s) => s.error);
  const reload = useAssetsStore((s) => s.load);
  const ids = useSelectionStore((s) => s.ids);
  const focusId = useSelectionStore((s) => s.focusId);
  const focus = useSelectionStore((s) => s.focus);
  const toggle = useSelectionStore((s) => s.toggle);

  if (error) {
    return (
      <main className="flex flex-1 flex-col items-center justify-center gap-3 text-mute">
        <p className="text-sm">加载失败：{error}</p>
        <button
          onClick={() => void reload()}
          className="rounded border border-accent/50 px-3 py-1 text-sm text-accent hover:bg-accent/10"
        >
          重试
        </button>
      </main>
    );
  }

  return (
    <main className="flex-1 overflow-y-auto p-4">
      {loading && assets.length === 0 ? (
        <div className="flex h-full items-center justify-center text-sm text-mute">
          加载素材中…
        </div>
      ) : assets.length === 0 ? (
        <div className="flex h-full items-center justify-center text-sm text-mute">
          暂无素材，点击右上角「导入」添加本地视频
        </div>
      ) : (
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-3 2xl:grid-cols-4">
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
        </div>
      )}
    </main>
  );
}
