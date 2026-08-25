import { useEffect, useRef, useState } from "react";
import { api } from "../../api/client";
import { useAssetsStore } from "../../stores/assetsStore";
import { useSelectionStore } from "../../stores/selectionStore";
import { useTaskStore } from "../../stores/taskStore";
import { OllamaStatusTag } from "./OllamaStatusTag";
import { CalendarPopover } from "../calendar/CalendarPopover";

export function TopBar({
  onOpenSettings,
}: {
  onOpenSettings: () => void;
}) {
  const filter = useAssetsStore((s) => s.filter);
  const setFilter = useAssetsStore((s) => s.setFilter);
  const analyzeSelected = useAssetsStore((s) => s.analyzeSelected);
  const reload = useAssetsStore((s) => s.load);
  const ids = useSelectionStore((s) => s.ids);
  const clear = useSelectionStore((s) => s.clear);
  const taskOpen = useTaskStore((s) => s.open);
  const setTaskOpen = useTaskStore((s) => s.setOpen);
  const waitingCount = useTaskStore((s) =>
    s.tasks.filter((t) => t.status === "WAITING_FOR_HUMAN").length,
  );
  const collecting = useTaskStore((s) =>
    s.tasks.some(
      (t) => t.agent_name === "collector" && ["PENDING", "RUNNING"].includes(t.status),
    ),
  );
  const [analyzing, setAnalyzing] = useState(false);
  const [collectStarting, setCollectStarting] = useState(false);
  const [calOpen, setCalOpen] = useState(false);
  const calBtnRef = useRef<HTMLButtonElement | null>(null);
  // 自动巡检开关：每 30 分钟自动同步抖音数据，发现违规作品自动处理。
  const [autoSync, setAutoSync] = useState(false);

  useEffect(() => {
    api.getAutoSyncModerate().then(setAutoSync).catch(() => {});
  }, []);

  const toggleAutoSync = async () => {
    const next = !autoSync;
    setAutoSync(next);
    try {
      await api.setAutoSyncModerate(next);
    } catch (e) {
      setAutoSync(!next); // 回滚
      alert(e instanceof Error ? e.message : String(e));
    }
  };

  const handleCollect = async () => {
    // 点击即置灰，refresh 拉到新任务后再交给 collecting 状态接力（避免轮询空窗内重复点击）。
    setCollectStarting(true);
    try {
      const tasks = await api.startCollection();
      setTaskOpen(true);
      await useTaskStore.getState().refresh();
      setTimeout(() => void useTaskStore.getState().refresh(), 1200);
      return tasks;
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
      return null;
    } finally {
      setCollectStarting(false);
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

      <button
        ref={calBtnRef}
        onClick={() => setCalOpen((v) => !v)}
        className={`h-9 rounded border px-3 text-sm transition-colors ${
          calOpen
            ? "border-accent bg-accent/15 text-accent"
            : "border-line text-mute hover:border-accent/40 hover:text-paper"
        }`}
        title="发布日历：查看已发布/定时作品与数据"
      >
        📅
      </button>
      {calOpen && (
        <CalendarPopover anchorRef={calBtnRef} onClose={() => setCalOpen(false)} />
      )}

      <div className="flex-1" />

      <button
        role="switch"
        aria-checked={autoSync}
        onClick={() => void toggleAutoSync()}
        className={`flex h-9 items-center gap-2 rounded border px-2.5 text-xs transition-colors ${
          autoSync
            ? "border-accent bg-accent/15 text-accent"
            : "border-line text-mute hover:border-accent/40 hover:text-paper"
        }`}
        title="自动巡检：每 30 分钟自动同步抖音数据；发现违规视频时自动处理（设为仅自己可见，今天发布的会把最晚定时作品改为立即发布）"
      >
        <span
          className={`relative h-4 w-7 shrink-0 rounded-full transition-colors ${
            autoSync ? "bg-accent" : "bg-line"
          }`}
        >
          <span
            className={`absolute top-0.5 h-3 w-3 rounded-full bg-white transition-all ${
              autoSync ? "left-3.5" : "left-0.5"
            }`}
          />
        </span>
        自动巡检
      </button>
      <button
        onClick={() => void reload()}
        className="h-9 rounded border border-line px-3 text-sm text-mute hover:border-accent/40 hover:text-paper"
        title="刷新"
      >
        ⟳
      </button>
      <OllamaStatusTag />
      <button
        onClick={() => void handleCollect()}
        disabled={collecting || collectStarting}
        className="h-9 rounded border px-3 text-sm transition-colors disabled:cursor-not-allowed disabled:opacity-40 enabled:border-accent enabled:bg-accent/10 enabled:text-accent enabled:hover:bg-accent/20"
        title={
          collecting
            ? "采集任务执行中，完成后可再次发起"
            : "按设置配额刷 TikTok / Instagram 推荐流采集并下载"
        }
      >
        {collecting ? "采集中…" : "开始采集"}
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
        onClick={() =>
          void api
            .openUrl("https://creator.douyin.com/creator-micro/content/manage")
            .catch((e) => alert(String(e)))
        }
        className="h-9 rounded border border-line px-3 text-sm text-mute hover:border-accent/40 hover:text-paper"
        title="打开抖音创作者中心"
      >
        🏠
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
