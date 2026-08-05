import { create } from "zustand";
import { api } from "../api/client";
import type { FilterState, LibraryOverview, VideoAsset } from "../api/types";
import { buildAssetFilter } from "./helpers";

interface AssetsState {
  assets: VideoAsset[];
  overview: LibraryOverview | null;
  filter: FilterState;
  loading: boolean;
  error: string | null;
  load: () => Promise<void>;
  setFilter: (patch: Partial<FilterState>) => Promise<void>;
  toggleFavorite: (id: number) => Promise<void>;
  removeAsset: (id: number) => Promise<void>;
  removeMany: (ids: number[]) => Promise<void>;
  updateStatus: (id: number, to: string) => Promise<void>;
  importFiles: (paths: string[]) => Promise<void>;
  analyzeSelected: (ids: number[]) => Promise<number>;
}

export const useAssetsStore = create<AssetsState>((set, get) => ({
  assets: [],
  overview: null,
  filter: {},
  loading: false,
  error: null,

  load: async () => {
    set({ loading: true, error: null });
    try {
      const [assets, overview] = await Promise.all([
        api.listAssets(buildAssetFilter(get().filter)),
        api.libraryOverview(),
      ]);
      set({ assets, overview, loading: false });
    } catch (e) {
      set({ loading: false, error: e instanceof Error ? e.message : String(e) });
    }
  },

  setFilter: async (patch) => {
    set((s) => ({ filter: { ...s.filter, ...patch } }));
    await get().load();
  },

  toggleFavorite: async (id) => {
    const asset = get().assets.find((a) => a.id === id);
    if (!asset) return;
    await api.setFavorite(id, !asset.is_favorite);
    await get().load();
  },

  removeAsset: async (id) => {
    await api.deleteAsset(id);
    await get().load();
  },

  removeMany: async (ids) => {
    await Promise.all(ids.map((id) => api.deleteAsset(id)));
    await get().load();
  },

  updateStatus: async (id, to) => {
    await api.updateAssetStatus(id, to);
    await get().load();
  },

  importFiles: async (paths) => {
    await api.importVideos(paths);
    await get().load();
  },

  analyzeSelected: async (ids) => {
    const enqueued = await api.analyzeAssets(ids);
    await get().load();
    return enqueued.length;
  },
}));
