import { useEffect, useRef, useState } from "react";
import { ask, message, open, save } from "@tauri-apps/plugin-dialog";
import type { AgentTask, FilterState } from "../../api/types";
import { api } from "../../api/client";
import { useAssetsStore } from "../../stores/assetsStore";
import { useSelectionStore } from "../../stores/selectionStore";
import { useTaskStore } from "../../stores/taskStore";
import { statusMeta } from "../../lib/format";
import { PublishProgressList } from "./PublishProgressList";

// 分析中不单独展示，归入"待处理"（NEW）；下载失败不展示。
const STATUS_ORDER = [
  "NEW",
  "CANDIDATE",
  "SELECTED",
  "PUBLISHED",
  "ELIMINATED",
];

const ACTIVE_STATUSES = ["PENDING", "RUNNING", "WAITING_FOR_HUMAN"];

function backupFileName() {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, "0");
  return `douyin-backup-${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}.zip`;
}

export function Sidebar({ width }: { width: number }) {
  const overview = useAssetsStore((s) => s.overview);
  const filter = useAssetsStore((s) => s.filter);
  const setFilter = useAssetsStore((s) => s.setFilter);
  const [busy, setBusy] = useState<"import" | "export" | null>(null);
  // 数据导出：记录待跟踪的导出任务 id，完成后触发保存对话框。
  const exportTaskRef = useRef<number | null>(null);
  const tasks = useTaskStore((s) => s.tasks);

  const exportFileName = () => {
    const d = new Date();
    const p = (n: number) => String(n).padStart(2, "0");
    return `douyin-data-${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}.xlsx`;
  };

  const handleExportReady = async (task: AgentTask) => {
    const filePath = task.payload?.file_path;
    if (typeof filePath !== "string" || !filePath) {
      await message("导出完成但未找到生成的文件。", { title: "数据导出", kind: "error" });
      return;
    }
    const dest = await save({
      title: "保存抖音作品数据清单",
      defaultPath: exportFileName(),
      filters: [{ name: "Excel 工作簿", extensions: ["xlsx"] }],
    });
    if (!dest) return;
    try {
      await api.saveExportFile(filePath, dest);
      const rows = task.payload?.rows ?? 0;
      const images = task.payload?.images ?? 0;
      await message(`已导出 ${rows} 条作品数据（含 ${images} 张封面）。`, {
        title: "数据导出",
      });
    } catch (e) {
      await message(e instanceof Error ? e.message : String(e), {
        title: "数据导出",
        kind: "error",
      });
    }
  };

  // 跟随任务面板轮询，监听导出任务终态。
  useEffect(() => {
    const id = exportTaskRef.current;
    if (id == null) return;
    const task = tasks.find((t) => t.id === id);
    if (!task) return;
    if (task.status === "SUCCEEDED") {
      exportTaskRef.current = null;
      void handleExportReady(task);
    } else if (task.status === "FAILED" || task.status === "CANCELLED") {
      exportTaskRef.current = null;
      void message(task.error ?? "导出任务已结束。", { title: "数据导出", kind: "error" });
    }
  }, [tasks]);

  const handleExportData = async () => {
    if (exportTaskRef.current != null) return;
    try {
      const id = await api.startDouyinExport();
      exportTaskRef.current = id;
      await message("已开始拉取抖音作品数据并生成 Excel，完成后将弹出保存位置。", {
        title: "数据导出",
      });
    } catch (e) {
      await message(e instanceof Error ? e.message : String(e), {
        title: "数据导出",
        kind: "error",
      });
    }
  };

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

  const activeTaskCount = () =>
    useTaskStore.getState().tasks.filter((t) => ACTIVE_STATUSES.includes(t.status)).length;

  const handleExport = async () => {
    const active = activeTaskCount();
    if (active > 0) {
      const ok = await ask(
        `有 ${active} 个任务正在进行，备份可能包含进行到一半的状态。仍要导出吗？`,
        { title: "导出备份", kind: "warning" },
      );
      if (!ok) return;
    }
    const dest = await save({
      title: "保存备份",
      defaultPath: backupFileName(),
      filters: [{ name: "备份压缩包", extensions: ["zip"] }],
    });
    if (!dest) return;
    setBusy("export");
    try {
      const stats = await api.exportBackup(dest);
      await message(
        `导出完成：${stats.files} 个文件，${(stats.bytes / 1024 / 1024).toFixed(1)} MB。`,
        { title: "导出备份" },
      );
    } catch (e) {
      await message(e instanceof Error ? e.message : String(e), {
        title: "导出失败",
        kind: "error",
      });
    } finally {
      setBusy(null);
    }
  };

  const handleImport = async () => {
    const active = activeTaskCount();
    if (active > 0) {
      await message(
        `有 ${active} 个任务正在进行。导入需要重启应用，请先取消或等待任务完成。`,
        { title: "导入备份", kind: "warning" },
      );
      return;
    }
    const picked = await open({
      title: "选择备份压缩包",
      filters: [{ name: "备份压缩包", extensions: ["zip"] }],
    });
    if (!picked || typeof picked !== "string") return;
    setBusy("import");
    try {
      // 第一步：校验并解压到暂存区（此步无副作用，取消不会留标记）。
      const preview = await api.importBackup(picked);
      const ok = await ask(
        `备份包含 ${preview.assets} 个素材、${preview.videos} 个视频（${(preview.bytes / 1024 / 1024).toFixed(1)} MB）。\n\n导入将覆盖当前全部数据（素材 / 任务 / 设置），重启后生效，且不可撤销。确定导入吗？`,
        { title: "导入备份", kind: "warning" },
      );
      if (!ok) return;
      // 第二步：用户确认后才写 pending 标记。
      await api.confirmImport();
      const restart = await ask("导入已就绪，重启应用后生效。现在重启吗？", {
        title: "导入备份",
      });
      if (restart) {
        await api.restartApp();
      } else {
        await message("已安排导入，下次启动应用时生效。", { title: "导入备份" });
      }
    } catch (e) {
      await message(e instanceof Error ? e.message : String(e), {
        title: "导入失败",
        kind: "error",
      });
    } finally {
      setBusy(null);
    }
  };

  return (
    <aside
      style={{ width }}
      className="flex shrink-0 flex-col border-r border-line bg-panel/60"
    >
      <div className="flex-1 overflow-y-auto p-3">
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
      </div>
      <PublishProgressList />
      <div className="flex justify-end gap-2 border-t border-line px-3 py-2">
        <button
          onClick={() => void handleExportData()}
          className="h-7 rounded bg-accent px-2.5 text-xs font-medium text-white transition-colors hover:brightness-110 disabled:cursor-not-allowed disabled:opacity-40"
          title="从抖音创作者平台拉取全部已发布（含定时待发布）作品数据，导出为 Excel 清单"
        >
          数据导出
        </button>
        <button
          onClick={() => void handleImport()}
          disabled={busy !== null}
          className="h-7 rounded border border-line px-2.5 text-xs text-mute transition-colors hover:border-accent/40 hover:text-paper disabled:cursor-not-allowed disabled:opacity-40"
          title="从备份压缩包恢复全部数据（覆盖现有数据，重启后生效）"
        >
          {busy === "import" ? "导入中…" : "导入"}
        </button>
        <button
          onClick={() => void handleExport()}
          disabled={busy !== null}
          className="h-7 rounded border border-line px-2.5 text-xs text-mute transition-colors hover:border-accent/40 hover:text-paper disabled:cursor-not-allowed disabled:opacity-40"
          title="把视频、素材状态、任务记录与设置打包成一个 zip 备份"
        >
          {busy === "export" ? "导出中…" : "导出"}
        </button>
      </div>
    </aside>
  );
}
