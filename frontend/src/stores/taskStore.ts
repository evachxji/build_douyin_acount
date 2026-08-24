import { create } from "zustand";
import { api } from "../api/client";
import type { AgentTask } from "../api/types";
import { useAssetsStore } from "./assetsStore";
import { useDouyinPostsStore } from "./douyinPostsStore";

interface TasksState {
  tasks: AgentTask[];
  open: boolean;
  error: string | null;
  timer: ReturnType<typeof setInterval> | null;
  refresh: () => Promise<void>;
  setOpen: (open: boolean) => void;
  startPolling: () => void;
  resume: (id: number) => Promise<void>;
  cancel: (id: number) => Promise<void>;
}

export const useTaskStore = create<TasksState>((set, get) => ({
  tasks: [],
  open: false,
  error: null,
  timer: null,

  refresh: async () => {
    try {
      const tasks = await api.listTasks();
      const prev = get().tasks;
      set({ tasks, error: null });
      // 任务进入终态（成功/失败/取消）时刷新素材列表，让分析结果与发布状态及时呈现。
      const terminal = ["SUCCEEDED", "FAILED", "CANCELLED"];
      const newlyTerminal = tasks.some(
        (t) =>
          terminal.includes(t.status) &&
          prev.some((p) => p.id === t.id && !terminal.includes(p.status)),
      );
      if (newlyTerminal) void useAssetsStore.getState().load();
      // 同步/违规处理任务终态时刷新发布日历数据。
      const syncTerminal = tasks.some(
        (t) =>
          (t.agent_name === "douyin_sync" || t.agent_name === "douyin_moderate") &&
          terminal.includes(t.status) &&
          prev.some((p) => p.id === t.id && !terminal.includes(p.status)),
      );
      if (syncTerminal) void useDouyinPostsStore.getState().load();
    } catch (e) {
      set({ error: e instanceof Error ? e.message : String(e) });
    }
  },

  setOpen: (open) => {
    set({ open });
  },

  // 常驻轮询（App 启动时开启）：确认窗/兜底弹窗/按钮状态都依赖任务数据，
  // 与底部任务面板是否打开无关。
  startPolling: () => {
    if (get().timer) return;
    const t = setInterval(() => void get().refresh(), 2500);
    set({ timer: t });
    void get().refresh();
  },

  resume: async (id) => {
    await api.resumeTask(id);
    await get().refresh();
  },

  cancel: async (id) => {
    await api.cancelTask(id);
    await get().refresh();
  },
}));
