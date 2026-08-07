import { invoke } from "@tauri-apps/api/core";
import type {
  AgentTask,
  AnalysisResult,
  AppSettings,
  AssetListFilterArg,
  ContentCopy,
  LibraryOverview,
  PreparePublishResult,
  PublishDraft,
  TaskLog,
  VideoAsset,
} from "./types";

/** 后端 DomainError 统一转成的可读错误。 */
export class ApiError extends Error {
  constructor(
    public code: string,
    message: string,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

function normalizeError(e: unknown): ApiError {
  if (e && typeof e === "object" && "code" in e && "message" in e) {
    const o = e as { code: string; message: string };
    return new ApiError(o.code, o.message);
  }
  const msg =
    typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e);
  return new ApiError("INVOKE_FAILED", msg);
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw normalizeError(e);
  }
}

export const api = {
  appInfo: () => call<{ name: string; phase: number }>("app_info"),
  listAssets: (filter: AssetListFilterArg) =>
    call<VideoAsset[]>("list_assets", { filter }),
  getAsset: (id: number) => call<VideoAsset | null>("get_asset", { id }),
  getAnalysis: (assetId: number) =>
    call<AnalysisResult | null>("get_analysis", { assetId }),
  deleteAsset: (id: number) => call<void>("delete_asset", { id }),
  updateAssetStatus: (id: number, to: string) =>
    call<void>("update_asset_status", { id, to }),
  importVideos: (paths: string[], platform?: string) =>
    call<VideoAsset[]>("import_videos", { paths, platform: platform ?? null }),
  revealInFolder: (path: string) => call<void>("reveal_in_folder", { path }),
  libraryOverview: () => call<LibraryOverview>("library_overview"),
  analyzeAssets: (ids: number[]) => call<number[]>("analyze_assets", { ids }),
  listTasks: (status?: string) =>
    call<AgentTask[]>("list_tasks", { status: status ?? null }),
  getTask: (id: number) =>
    call<{ task: AgentTask | null; logs: TaskLog[] }>("get_task", { id }),
  cancelTask: (id: number) => call<void>("cancel_task", { id }),
  resumeTask: (id: number) => call<void>("resume_task", { id }),
  getSettings: () => call<AppSettings>("get_settings"),
  updateSettings: (settings: AppSettings) =>
    call<AppSettings>("update_settings", { settings }),
  preparePublish: (assetId: number) =>
    call<PreparePublishResult>("prepare_publish", { assetId }),
  listDrafts: () => call<PublishDraft[]>("list_drafts"),
  publishConfirmed: (draftId: number) =>
    call<void>("publish_confirmed", { draftId }),
  copyText: (text: string) => call<void>("copy_to_clipboard", { text }),
  startCollection: () =>
    call<{ platform: string; count: number; task_id: number }[]>("start_collection"),
  generateCopy: (assetId: number) => call<number>("generate_copy", { assetId }),
  getCopy: (assetId: number) => call<ContentCopy | null>("get_copy", { assetId }),
};
