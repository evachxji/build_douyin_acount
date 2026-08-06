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
  account_style: string;
  batch_size: number;
  tiktok_ratio: number;
  instagram_ratio: number;
  max_likes: number;
}

export type DraftStatus = "DRAFT" | "FILLED" | "PUBLISHED" | "CANCELLED";

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

export interface RecommendationItem {
  asset_id: number;
  title: string | null;
  author: string | null;
  likes: number;
  quality_score: number | null;
  score: number;
  category: string;
  reason: string | null;
}

export interface RecommendationGroup {
  category: string;
  items: RecommendationItem[];
}

export interface RecommendationResult {
  batch_id: string;
  groups: RecommendationGroup[];
}

export interface AssetListFilterArg {
  category?: string;
  status?: string;
  platform?: string;
  search?: string;
  limit?: number;
  offset?: number;
}

export interface FilterState {
  category?: string;
  status?: string;
  platform?: string;
  favorite?: boolean;
  search?: string;
}
