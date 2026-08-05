import { useEffect, useState } from "react";
import { DetailPanel } from "./components/layout/DetailPanel";
import { Sidebar } from "./components/layout/Sidebar";
import { TopBar } from "./components/layout/TopBar";
import { Waterfall } from "./components/layout/Waterfall";
import { SettingsDrawer } from "./components/settings/SettingsDrawer";
import { ApprovalDialog } from "./components/tasks/ApprovalDialog";
import { TaskQueuePanel } from "./components/tasks/TaskQueuePanel";
import { useAssetsStore } from "./stores/assetsStore";
import { useSelectionStore } from "./stores/selectionStore";
import { useTaskStore } from "./stores/taskStore";

function App() {
  const load = useAssetsStore((s) => s.load);
  const assets = useAssetsStore((s) => s.assets);
  const focusId = useSelectionStore((s) => s.focusId);
  const focus = useSelectionStore((s) => s.focus);
  const taskOpen = useTaskStore((s) => s.open);
  const [settingsOpen, setSettingsOpen] = useState(false);

  useEffect(() => {
    void load();
  }, [load]);

  // 素材加载后默认聚焦第一张，方便直接看详情。
  useEffect(() => {
    if (assets.length > 0 && focusId === null) {
      focus(assets[0].id);
    }
  }, [assets, focusId, focus]);

  return (
    <div className="flex h-screen flex-col overflow-hidden bg-ink text-paper">
      <TopBar onOpenSettings={() => setSettingsOpen(true)} />
      <div className="flex min-h-0 flex-1">
        <Sidebar />
        <Waterfall />
        <DetailPanel />
      </div>
      {taskOpen && <TaskQueuePanel />}
      <SettingsDrawer open={settingsOpen} onClose={() => setSettingsOpen(false)} />
      <ApprovalDialog />
    </div>
  );
}

export default App;
