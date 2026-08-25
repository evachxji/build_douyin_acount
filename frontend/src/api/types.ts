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
  category: string | null;
  quality_score: number | null;
  created_at: string;
  updated_at: string;
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
  ytdlp_path: string;
  douyin_sync_script: string;
  account_style: string;
  batch_size: number;
  tiktok_ratio: number;
  instagram_ratio: number;
  max_likes: number;
}

export type DraftStatus = "DRAFT" | "FILLED" | "PUBLISHED" | "CANCELLED";


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
