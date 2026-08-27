/// 数据库 schema 迁移（v1：全部基础表）。
pub const MIGRATIONS: &str = r#"
CREATE TABLE IF NOT EXISTS video_assets (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  dedup_key TEXT UNIQUE,
  file_path TEXT NOT NULL,
  cover_path TEXT,
  source_platform TEXT NOT NULL DEFAULT 'local',
  source_url TEXT,
  author TEXT,
  title TEXT,
  description TEXT,
  published_at TEXT,
  tags_json TEXT NOT NULL DEFAULT '[]',
  likes INTEGER NOT NULL DEFAULT 0,
  comments INTEGER NOT NULL DEFAULT 0,
  plays INTEGER NOT NULL DEFAULT 0,
  duration_ms INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL DEFAULT 'NEW',
  eliminate_reason TEXT,
  music_info_json TEXT,
  is_favorite INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS analysis_results (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  asset_id INTEGER NOT NULL UNIQUE REFERENCES video_assets(id) ON DELETE CASCADE,
  category TEXT,
  tags_json TEXT NOT NULL DEFAULT '[]',
  quality_score INTEGER,
  has_foreign_language INTEGER NOT NULL DEFAULT 0,
  has_large_amount_english_subtitle INTEGER NOT NULL DEFAULT 0,
  summary TEXT,
  recommended_caption TEXT,
  asr_text TEXT,
  asr_language TEXT,
  frame_paths_json TEXT NOT NULL DEFAULT '[]',
  model_name TEXT,
  prompt_version TEXT,
  condition_results_json TEXT NOT NULL DEFAULT '[]',
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_analysis_asset ON analysis_results(asset_id);

CREATE TABLE IF NOT EXISTS account_styles (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL UNIQUE,
  description TEXT,
  tone_prompt TEXT,
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS publish_drafts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  asset_id INTEGER NOT NULL REFERENCES video_assets(id) ON DELETE CASCADE,
  style_id INTEGER REFERENCES account_styles(id),
  title TEXT,
  body TEXT,
  hashtags_json TEXT NOT NULL DEFAULT '[]',
  status TEXT NOT NULL DEFAULT 'DRAFT',
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS agent_tasks (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  agent_name TEXT NOT NULL,
  task_type TEXT NOT NULL,
  payload_json TEXT NOT NULL DEFAULT '{}',
  status TEXT NOT NULL DEFAULT 'PENDING',
  attempts INTEGER NOT NULL DEFAULT 0,
  max_attempts INTEGER NOT NULL DEFAULT 3,
  next_retry_at TEXT,
  human_required INTEGER NOT NULL DEFAULT 0,
  error TEXT,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_agent_tasks_agent_status ON agent_tasks(agent_name, status);

CREATE TABLE IF NOT EXISTS task_logs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  task_id INTEGER NOT NULL REFERENCES agent_tasks(id) ON DELETE CASCADE,
  level TEXT NOT NULL DEFAULT 'INFO',
  message TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_task_logs_task ON task_logs(task_id);

CREATE TABLE IF NOT EXISTS recommendations (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  batch_id TEXT NOT NULL,
  asset_id INTEGER NOT NULL REFERENCES video_assets(id) ON DELETE CASCADE,
  category TEXT,
  rank INTEGER NOT NULL,
  reason TEXT,
  score REAL NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_recommendations_batch ON recommendations(batch_id);

CREATE TABLE IF NOT EXISTS content_copies (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  asset_id INTEGER NOT NULL UNIQUE REFERENCES video_assets(id) ON DELETE CASCADE,
  style_id INTEGER REFERENCES account_styles(id),
  title TEXT,
  body TEXT,
  hashtags_json TEXT NOT NULL DEFAULT '[]',
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS app_settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL,
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 抖音创作者中心同步回来的作品快照（发布日历数据源）。
-- 时间用 epoch 秒（源数据即 epoch）；率类指标列表 API 不返回时留 NULL，UI 显示 —。
CREATE TABLE IF NOT EXISTS douyin_posts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  aweme_id TEXT NOT NULL UNIQUE,
  title TEXT,
  cover_path TEXT,
  cover_url TEXT,
  status TEXT NOT NULL DEFAULT 'published',
  play_count INTEGER NOT NULL DEFAULT 0,
  digg_count INTEGER NOT NULL DEFAULT 0,
  comment_count INTEGER NOT NULL DEFAULT 0,
  share_count INTEGER NOT NULL DEFAULT 0,
  completion_rate REAL,
  bounce_rate_2s REAL,
  violation_status INTEGER,
  violation_text TEXT,
  violation_brief TEXT,
  violation_url TEXT,
  create_time INTEGER NOT NULL DEFAULT 0,
  public_time INTEGER NOT NULL DEFAULT 0,
  synced_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_douyin_posts_public_time ON douyin_posts(public_time);

-- 音乐文件 ↔ 专辑封面映射（封面落盘 storage/music/covers/，此处记映射与查询状态）。
-- status: URL_READY（同步时从剪映缓存拿到精确封面URL，待下载）/ FOUND（cover_path 有效）
-- / NOT_FOUND（网上未找到，queried_at 起 7 天后才重试，避免每次同步重复打外网）。
CREATE TABLE IF NOT EXISTS music_covers (
  file_name TEXT PRIMARY KEY,
  cover_path TEXT,
  cover_url TEXT,
  status TEXT NOT NULL,
  queried_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 音乐库采集的分类映射（music_collect）：一首歌可属多个分类（多行），
-- ordinal 是分类在剪映音乐库栏目里的顺序（弹窗分类栏按它排序）。
-- song_id 是剪映/抖音 19 位歌曲 id，存 TEXT（超 i64::MAX，禁 f64）。
CREATE TABLE IF NOT EXISTS music_categories (
  song_id TEXT NOT NULL,
  file_name TEXT NOT NULL,
  category TEXT NOT NULL,
  ordinal INTEGER NOT NULL DEFAULT 0,
  duration_s INTEGER,
  PRIMARY KEY (song_id, category)
);
CREATE INDEX IF NOT EXISTS idx_music_categories_file ON music_categories(file_name);

-- 状态模型重构（第五阶段）：READY -> CANDIDATE；SKIPPED/LOW_PRIORITY -> ELIMINATED
UPDATE video_assets SET status = 'CANDIDATE' WHERE status = 'READY';
UPDATE video_assets SET status = 'ELIMINATED' WHERE status IN ('SKIPPED', 'LOW_PRIORITY');
"#;
