import { create } from "zustand";
import { api } from "../api/client";
import type { DouyinPost } from "../api/types";

/** 发布日历数据：抖音作品快照（douyin_posts 表）。 */
interface DouyinPostsState {
  posts: DouyinPost[];
  loaded: boolean;
  error: string | null;
  load: () => Promise<void>;
}

export const useDouyinPostsStore = create<DouyinPostsState>((set) => ({
  posts: [],
  loaded: false,
  error: null,

  load: async () => {
    try {
      const posts = await api.listDouyinPosts();
      set({ posts, loaded: true, error: null });
    } catch (e) {
      set({ error: e instanceof Error ? e.message : String(e) });
    }
  },
}));
