export type AssetStatus =
  | "NEW"
  | "ANALYZING"
  | "CANDIDATE"
  | "SELECTED"
  | "PUBLISHED"
  | "ELIMINATED"
  | "DOWNLOAD_FAILED";

export type TaskStatus =
  | "PENDING"
  | "RUNNING"
  | "WAITING_FOR_HUMAN"
  | "SUCCEEDED"
  | "FAILED"
  | "CANCELLED";

export interface VideoAsset {
  id: number;
  dedup_key: string | null;
  file_path: string;
  cover_path: string | null;
  source_platform: string;
  source_url: string | null;
  author: string | null;
  title: string | null;
  description: string | null;
  published_at: string | null;
  tags: string[];
  likes: number;
  comments: number;
  plays: number;
  duration_ms: number;
  status: AssetStatus;
  eliminate_reason: string | null;
  /** 已合成音乐的信息（未合成为 null）。 */
  music_info: MusicInfo | null;
  category: string | null;
  quality_score: number | null;
  created_at: string;
  updated_at: string;
}

/** 素材已合成音乐的信息（再次合成从 origin_file_path 重来，不叠音）。 */
export interface MusicInfo {
  /** 首次合成前的原始视频路径。 */
  origin_file_path: string;
  /** 选用的音乐文件名。 */
  music_name: string;
  /** 是否关闭原声。 */
  mute_original: boolean;
  /** 原声音量百分比 0-100（保留原声混音时生效；旧记录缺省 30）。 */
  video_volume: number;
  /** 音乐音量百分比 0-100（保留原声混音时生效；旧记录缺省 70）。 */
  music_volume: number;
}

export interface AnalysisResult {
  id: number;
  asset_id: number;
  category: string | null;
  tags: string[];
  quality_score: number | null;
  has_foreign_language: boolean;
  has_large_amount_english_subtitle: boolean;
  summary: string | null;
  recommended_caption: string | null;
  asr_text: string | null;
  asr_language: string | null;
  frame_paths: string[];
  model_name: string | null;
  prompt_version: string | null;
  created_at: string;
}

export interface AgentTask {
  id: number;
  agent_name: string;
  task_type: string;
  payload: Record<string, unknown>;
  status: TaskStatus;
  attempts: number;
  max_attempts: number;
  next_retry_at: string | null;
  human_required: boolean;
  error: string | null;
  created_at: string;
  updated_at: string;
}

export interface TaskLog {
  id: number;
  task_id: number;
  level: string;
  message: string;
  created_at: string;
}

export interface CategoryCount {
  name: string;
  count: number;
}

export interface StatusCount {
  status: string;
  count: number;
}

export interface LibraryOverview {
  total: number;
  categories: CategoryCount[];
  statuses: StatusCount[];
}

export type AiProvider = "ollama" | "openai";

export interface AppSettings {
  ollama_url: string;
  vision_model: string;
  llm_model: string;
  whisper_python: string;
  whisper_script: string;
  whisper_model: string;
  quality_min_keep: number;
  hit_conditions: string[];
  exclude_conditions: string[];
  condition_basis: Record<string, "画面" | "音频" | "不限">;
  node_bin: string;
  publisher_script: string;
  publisher_browser: string;
  collector_script: string;
  douyin_sync_script: string;
  account_style: string;
  batch_size: number;
  tiktok_ratio: number;
  instagram_ratio: number;
  max_likes: number;
  /** 视觉模型提供方：ollama 本地 / openai OpenAI 兼容云端。 */
  vision_provider: AiProvider;
  /** 文本 LLM 提供方（文案生成跟随此开关）。 */
  llm_provider: AiProvider;
  /** 视觉模型的 OpenAI 兼容云端配置（与文本模型相互独立）。 */
  openai_vision_base_url: string;
  openai_vision_api_key: string;
  openai_vision_model: string;
  /** 文本模型的 OpenAI 兼容云端配置。 */
  openai_llm_base_url: string;
  openai_llm_api_key: string;
  openai_llm_model: string;
  /** 发布时自动设置「自主声明 → 内容为转载信息 → 取材站外」（默认开启）。 */
  publish_declaration: boolean;
}

export type DraftStatus = "DRAFT" | "FILLED" | "PUBLISHED" | "CANCELLED";

/** 内置 yt-dlp 本地状态（storage/tools/yt-dlp.exe）。 */
export interface YtDlpStatus {
  installed: boolean;
  current_version: string | null;
  exe_path: string;
}

/** yt-dlp 更新检查结果（GitHub 最新版本号）。 */
export interface YtDlpUpdateInfo {
  latest_version: string;
  update_available: boolean;
}

/** yt-dlp 一键更新进度（后端内存态，前端 1s 轮询）。 */
export interface YtDlpUpdateProgress {
  stage: "downloading" | "verifying" | "done" | "error";
  downloaded: number;
  total: number;
  speed_bps: number;
  /** done：新版本号或「已是最新」；error：错误详情。 */
  message: string;
}

/** yt-dlp 每日更新检查缓存（每天启动时后台静默检查填充；设置页读它，不联网）。 */
export interface CachedYtDlpUpdate {
  state: "checking" | "ok" | "failed";
  info: YtDlpUpdateInfo | null;
}

/** 内置 FFmpeg 本地状态（storage/tools/ffmpeg.exe，结构与 yt-dlp 卡片一致）。 */
export interface FfmpegStatus {
  installed: boolean;
  current_version: string | null;
  exe_path: string;
}

/** FFmpeg 更新检查结果（gyan.dev release 最新版本号）。 */
export interface FfmpegUpdateInfo {
  latest_version: string;
  update_available: boolean;
}

/** FFmpeg 一键更新进度（后端内存态，前端 1s 轮询；verifying 含 zip 解压）。 */
export interface FfmpegUpdateProgress {
  stage: "downloading" | "verifying" | "done" | "error";
  downloaded: number;
  total: number;
  speed_bps: number;
  /** done：新版本号或「已是最新」；error：错误详情。 */
  message: string;
}

/** FFmpeg 每日更新检查缓存（语义同 CachedYtDlpUpdate）。 */
export interface CachedFfmpegUpdate {
  state: "checking" | "ok" | "failed";
  info: FfmpegUpdateInfo | null;
}

/** 剪映音乐库本地状态（storage/music）。 */
export interface MusicStatus {
  /** 已同步到本地音乐目录的音频文件数。 */
  count: number;
  /** 本地音乐目录绝对路径。 */
  dir: string;
  /** 本机是否存在剪映音乐缓存。 */
  cache_found: boolean;
}

/** 剪映音乐一键同步结果。 */
export interface MusicSyncResult {
  copied: number;
  skipped: number;
  /** 未匹配歌名而跳过（未复制）的文件数。 */
  unmatched: number;
}

/** 音乐库文件（添加音乐弹窗列表项）。 */
export interface MusicFile {
  /** 文件名去扩展名（展示用）。 */
  name: string;
  /** 完整文件名。 */
  file_name: string;
  /** 绝对路径。 */
  path: string;
  size_bytes: number;
  /** 专辑封面绝对路径（未匹配到为 null）。 */
  cover_path: string | null;
}

/** 专辑封面增量匹配结果（iTunes Search API）。 */
export interface CoverReport {
  /** 本次新下载到封面的数量。 */
  downloaded: number;
  /** 本次查询后仍未找到/下载失败的数量。 */
  not_found: number;
  /** 已有封面（或近期已查过未找到）而跳过的数量。 */
  skipped: number;
}

/** 添加音乐合成结果（素材 file_path 已指向合成产物）。 */
export interface MusicApplyResult {
  file_path: string;
  music_info: MusicInfo;
}

/** Whisper 模型下载进度（后端 progress 文件原样透传；interrupted 由后端按心跳判定后标注）。 */
export interface WhisperDownloadProgress {
  model: string;
  stage:
    | "connecting"
    | "downloading"
    | "verifying"
    | "done"
    | "error"
    | "cancelled"
    | "interrupted";
  route: string; // official / mirror
  current_file: string;
  file_index: number;
  file_count: number;
  file_downloaded: number;
  file_total: number;
  total_downloaded: number;
  total_bytes: number;
  speed_bps: number;
  url: string;
  dest: string;
  message: string;
  updated_at: number;
}

/** Whisper 模型规格与本地安装状态（storage/models/whisper/<名称>/）。 */
export interface WhisperModelInfo {
  name: string;
  size_mb: number;
  vram_hint: string;
  file_count: number;
  installed: boolean;
  dir: string;
  download: WhisperDownloadProgress | null;
}

/** 下载路由：auto=官方优先、失败/过慢自动切国内镜像。 */
export type WhisperDownloadRoute = "auto" | "official" | "mirror";

export interface PublishDraft {
  id: number;
  asset_id: number;
  style_id: number | null;
  title: string | null;
  body: string | null;
  hashtags: string[];
  status: DraftStatus;
  created_at: string;
  updated_at: string;
}

export interface PreparePublishResult {
  draft: PublishDraft;
  task_id: number;
}

export interface ContentCopy {
  id: number;
  asset_id: number;
  style_id: number | null;
  title: string;
  body: string;
  hashtags: string[];
  created_at: string;
}

/** 抖音创作者中心同步回来的作品快照（发布日历数据源）。 */
export interface DouyinPost {
  id: number;
  aweme_id: string;
  title: string | null;
  cover_path: string | null;
  cover_url: string | null;
  status: string; // published / scheduled / reviewing / private / prohibited / deleted
  play_count: number;
  digg_count: number;
  comment_count: number;
  share_count: number;
  completion_rate: number | null;
  bounce_rate_2s: number | null;
  violation_status: number | null; // review.status 原值（流量减少=5）
  violation_text: string | null; // 审核结果摘要；非空即违规
  violation_brief: string | null; // 违规原因（如「原创性不足」）
  violation_url: string | null; // 违规申诉/详情页链接
  create_time: number; // epoch 秒
  public_time: number; // epoch 秒；定时发布为未来时间
  synced_at: string;
  updated_at: string;
}

export interface AssetListFilterArg {
  statuses?: string[];
  platform?: string;
  search?: string;
  limit?: number;
  offset?: number;
}

export interface FilterState {
  status?: string;
  platform?: string;
  favorite?: boolean;
  search?: string;
}
