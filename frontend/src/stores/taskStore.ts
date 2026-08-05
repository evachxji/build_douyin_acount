import { create } from "zustand";
import { api } from "../api/client";
import type { AgentTask } from "../api/types";
import { useAssetsStore } from "./assetsStore";

interface TasksState {
  tasks: AgentTask[];
  open: boolean;
  error: string | null;
  timer: ReturnType<typeof setInterval> | null;
  refresh: () => Promise<void>;
  setOpen: (open: boolean) => void;
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
    } catch (e) {
      set({ error: e instanceof Error ? e.message : String(e) });
    }
  },

  setOpen: (open) => {
    set({ open });
    const { timer } = get();
    if (open && !timer) {
      const t = setInterval(() => void get().refresh(), 2500);
      set({ timer: t });
      void get().refresh();
    } else if (!open && timer) {
      clearInterval(timer);
      set({ timer: null });
    }
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
