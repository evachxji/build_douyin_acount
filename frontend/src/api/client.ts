import { invoke } from "@tauri-apps/api/core";
import type {
  AgentTask,
  AnalysisResult,
  AppSettings,
  AssetListFilterArg,
  ContentCopy,
  DouyinPost,
  LibraryOverview,
  PreparePublishResult,
  PublishDraft,
  TaskLog,
  VideoAsset,
  WhisperDownloadRoute,
  WhisperModelInfo,
  MusicStatus,
  MusicSyncResult,
  MusicFile,
  CoverReport,
  MusicApplyResult,
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
  revealInFolder: (path: string) => call<void>("reveal_in_folder", { path }),
  libraryOverview: () => call<LibraryOverview>("library_overview"),
  analyzeAssets: (ids: number[]) => call<number[]>("analyze_assets", { ids }),
  exportBackup: (dest: string) =>
    call<{ files: number; bytes: number }>("export_backup", { dest }),
  importBackup: (src: string) =>
    call<{ assets: number; videos: number; bytes: number }>("import_backup", { src }),
  confirmImport: () => call<void>("confirm_import"),
  restartApp: () => call<void>("restart_app"),
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
  startDouyinSync: () => call<number>("start_douyin_sync"),
  listDouyinPosts: () => call<DouyinPost[]>("list_douyin_posts"),
  getAutoSyncModerate: () => call<boolean>("get_auto_sync_moderate"),
  setAutoSyncModerate: (enabled: boolean) =>
    call<void>("set_auto_sync_moderate", { enabled }),
  moderateDouyinViolation: (awemeId: string) =>
    call<number>("moderate_douyin_violation", { awemeId }),
  moderateAllViolations: () => call<number>("moderate_all_violations"),
  openUrl: (url: string) => call<void>("open_url", { url }),
  generateCopy: (assetId: number) => call<number>("generate_copy", { assetId }),
  getCopy: (assetId: number) => call<ContentCopy | null>("get_copy", { assetId }),
  updateCopy: (assetId: number, title: string, body: string) =>
    call<ContentCopy>("update_copy", { assetId, title, body }),
  checkOllama: () => call<boolean>("check_ollama"),
  startOllama: () => call<void>("start_ollama"),

  // 剪映音乐库（storage/music；同步是一键全量比对，秒级完成，无进度轮询）
  getMusicStatus: () => call<MusicStatus>("get_music_status"),
  syncMusic: () => call<MusicSyncResult>("sync_music"),
  openMusicDir: () => call<void>("open_music_dir"),
  listMusicFiles: () => call<MusicFile[]>("list_music_files"),
  // 专辑封面增量匹配（只查缺封面的几首；同步后/打开添加音乐弹窗时调用）
  ensureMusicCovers: () => call<CoverReport>("ensure_music_covers"),
  addMusicToVideo: (
    assetId: number,
    musicPath: string,
    muteOriginal: boolean,
    videoVolume: number,
    musicVolume: number,
  ) =>
    call<MusicApplyResult>("add_music_to_video", {
      assetId,
      musicPath,
      muteOriginal,
      videoVolume,
      musicVolume,
    }),
  listWhisperModels: () => call<WhisperModelInfo[]>("list_whisper_models"),
  startWhisperDownload: (model: string, route: WhisperDownloadRoute) =>
    call<void>("start_whisper_download", { model, route }),
  cancelWhisperDownload: (model: string) =>
    call<void>("cancel_whisper_download", { model }),
  deleteWhisperModel: (model: string) =>
    call<void>("delete_whisper_model", { model }),
};
