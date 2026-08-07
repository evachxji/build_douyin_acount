import { useEffect, useState } from "react";
import { DetailPanel } from "./components/layout/DetailPanel";
import { ResizeHandle } from "./components/layout/ResizeHandle";
import { Sidebar } from "./components/layout/Sidebar";
import { TopBar } from "./components/layout/TopBar";
import { Waterfall } from "./components/layout/Waterfall";
import { SettingsDrawer } from "./components/settings/SettingsDrawer";
import { ApprovalDialog } from "./components/tasks/ApprovalDialog";
import { TaskQueuePanel } from "./components/tasks/TaskQueuePanel";
import { useAssetsStore } from "./stores/assetsStore";
import { useSelectionStore } from "./stores/selectionStore";
import { useTaskStore } from "./stores/taskStore";

function readLayout(key: string, fallback: number): number {
  const v = Number(localStorage.getItem(`layout.${key}`));
  return Number.isFinite(v) && v > 0 ? v : fallback;
}

function persistLayout(key: string, v: number) {
  localStorage.setItem(`layout.${key}`, String(v));
}

const clamp = (v: number, min: number, max: number) =>
  Math.min(Math.max(v, min), max);

function App() {
  const load = useAssetsStore((s) => s.load);
  const assets = useAssetsStore((s) => s.assets);
  const focusId = useSelectionStore((s) => s.focusId);
  const focus = useSelectionStore((s) => s.focus);
  const taskOpen = useTaskStore((s) => s.open);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [leftWidth, setLeftWidth] = useState(() => readLayout("leftWidth", 224));
  const [rightWidth, setRightWidth] = useState(() => readLayout("rightWidth", 380));
  const [bottomHeight, setBottomHeight] = useState(() =>
    readLayout("bottomHeight", 224),
  );

  useEffect(() => {
    void load();
  }, [load]);

  // 素材加载后默认聚焦第一张，方便直接看详情。
  useEffect(() => {
    if (assets.length > 0 && focusId === null) {
      focus(assets[0].id);
    }
  }, [assets, focusId, focus]);

  const resizeLeft = (d: number) =>
    setLeftWidth((w) => {
      const next = clamp(w + d, 160, 400);
      persistLayout("leftWidth", next);
      return next;
    });
  const resizeRight = (d: number) =>
    setRightWidth((w) => {
      const next = clamp(w - d, 280, 640);
      persistLayout("rightWidth", next);
      return next;
    });
  const resizeBottom = (d: number) =>
    setBottomHeight((h) => {
      const next = clamp(h - d, 120, 480);
      persistLayout("bottomHeight", next);
      return next;
    });

  return (
    <div className="flex h-screen flex-col overflow-hidden bg-ink text-paper">
      <TopBar onOpenSettings={() => setSettingsOpen(true)} />
      <div className="flex min-h-0 flex-1">
        <Sidebar width={leftWidth} />
        <ResizeHandle direction="vertical" onResize={resizeLeft} />
        <Waterfall />
        <ResizeHandle direction="vertical" onResize={resizeRight} />
        <DetailPanel width={rightWidth} />
      </div>
      {taskOpen && (
        <>
          <ResizeHandle direction="horizontal" onResize={resizeBottom} />
          <TaskQueuePanel height={bottomHeight} />
        </>
      )}
      <SettingsDrawer open={settingsOpen} onClose={() => setSettingsOpen(false)} />
      <ApprovalDialog />
    </div>
  );
}

export default App;
