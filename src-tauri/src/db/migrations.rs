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

-- 状态模型重构（第五阶段）：READY -> CANDIDATE；SKIPPED/LOW_PRIORITY -> ELIMINATED
UPDATE video_assets SET status = 'CANDIDATE' WHERE status = 'READY';
UPDATE video_assets SET status = 'ELIMINATED' WHERE status IN ('SKIPPED', 'LOW_PRIORITY');
"#;
