import { create } from "zustand";
import { api } from "../api/client";
import type { AgentTask } from "../api/types";

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
      set({ tasks, error: null });
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
