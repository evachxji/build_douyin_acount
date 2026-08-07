import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "../../api/client";
import { useAssetsStore } from "../../stores/assetsStore";
import { useSelectionStore } from "../../stores/selectionStore";
import { useTaskStore } from "../../stores/taskStore";
import { OllamaStatusTag } from "./OllamaStatusTag";

export function TopBar({
  onOpenSettings,
}: {
  onOpenSettings: () => void;
}) {
  const filter = useAssetsStore((s) => s.filter);
  const setFilter = useAssetsStore((s) => s.setFilter);
  const importFiles = useAssetsStore((s) => s.importFiles);
  const analyzeSelected = useAssetsStore((s) => s.analyzeSelected);
  const reload = useAssetsStore((s) => s.load);
  const ids = useSelectionStore((s) => s.ids);
  const clear = useSelectionStore((s) => s.clear);
  const taskOpen = useTaskStore((s) => s.open);
  const setTaskOpen = useTaskStore((s) => s.setOpen);
  const waitingCount = useTaskStore((s) =>
    s.tasks.filter((t) => t.status === "WAITING_FOR_HUMAN").length,
  );
  const [analyzing, setAnalyzing] = useState(false);

  const handleImport = async () => {
    const picked = await open({
      multiple: true,
      title: "选择要导入的视频",
      filters: [{ name: "视频", extensions: ["mp4", "mov", "mkv", "webm"] }],
    });
    if (!picked) return;
    const paths = typeof picked === "string" ? [picked] : picked;
    await importFiles(paths);
  };

  const handleCollect = async () => {
    try {
      const tasks = await api.startCollection();
      setTaskOpen(true);
      setTimeout(() => void useTaskStore.getState().refresh(), 1200);
      return tasks;
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
      return null;
    }
  };

  const handleAnalyze = async () => {
    if (ids.size === 0) return;
    setAnalyzing(true);
    try {
      const n = await analyzeSelected([...ids]);
      setTaskOpen(true);
      if (n > 0) clear();
    } finally {
      setAnalyzing(false);
    }
  };

  return (
    <header className="flex h-14 shrink-0 items-center gap-3 border-b border-line bg-panel px-4">
      <div className="flex items-baseline gap-2">
        <h1 className="text-base font-bold tracking-wide text-paper">
          短视频素材运营台
        </h1>
      </div>

      <input
        value={filter.search ?? ""}
        onChange={(e) => void setFilter({ search: e.target.value })}
        placeholder="搜索标题 / 作者 / 摘要…"
        className="ml-4 h-9 w-64 rounded border border-line bg-ink px-3 text-sm text-paper placeholder:text-mute/60 focus:border-accent/60 focus:outline-none"
      />

      <select
        value={filter.platform ?? ""}
        onChange={(e) =>
          void setFilter({ platform: e.target.value === "" ? undefined : e.target.value })
        }
        className="h-9 rounded border border-line bg-ink px-2 text-sm text-paper focus:border-accent/60 focus:outline-none"
      >
        <option value="">全部平台</option>
        <option value="tiktok">TikTok</option>
        <option value="instagram">Instagram</option>
        <option value="local">本地</option>
      </select>

      <div className="flex-1" />

      <OllamaStatusTag />
      <button
        onClick={() => void reload()}
        className="h-9 rounded border border-line px-3 text-sm text-mute hover:border-accent/40 hover:text-paper"
        title="刷新"
      >
        ⟳
      </button>
      <button
        onClick={() => void handleImport()}
        className="h-9 rounded border border-line px-3 text-sm text-paper hover:border-accent/50 hover:bg-accent/10"
      >
        导入视频
      </button>
      <button
        onClick={() => void handleCollect()}
        className="h-9 rounded border border-accent bg-accent/10 px-3 text-sm text-accent hover:bg-accent/20"
        title="按设置配额刷 TikTok / Instagram 推荐流采集并下载"
      >
        开始采集
      </button>
      <button
        onClick={() => void handleAnalyze()}
        disabled={ids.size === 0 || analyzing}
        className="h-9 rounded border px-3 text-sm transition-colors disabled:cursor-not-allowed disabled:opacity-40 enabled:border-accent enabled:bg-accent enabled:text-ink enabled:hover:brightness-110"
        title="对选中的素材排队执行 AI 分析"
      >
        {analyzing ? "排队中…" : `AI 分析${ids.size > 0 ? ` (${ids.size})` : ""}`}
      </button>
      <button
        onClick={() => setTaskOpen(!taskOpen)}
        className={`relative h-9 rounded border px-3 text-sm transition-colors ${
          taskOpen
            ? "border-accent bg-accent/15 text-accent"
            : "border-line text-mute hover:border-accent/40 hover:text-paper"
        }`}
      >
        任务
        {waitingCount > 0 && (
          <span className="absolute -right-1.5 -top-1.5 flex h-4 min-w-4 items-center justify-center rounded-full bg-bad px-1 text-[10px] font-bold text-white">
            {waitingCount}
          </span>
        )}
      </button>
      <button
        onClick={onOpenSettings}
        className="h-9 rounded border border-line px-3 text-sm text-mute hover:border-accent/40 hover:text-paper"
        title="设置"
      >
        ⚙
      </button>
    </header>
  );
}
