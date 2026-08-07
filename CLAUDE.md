# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目概述

PC 端 AI 短视频运营助手（Windows 桌面应用）：从 TikTok / Instagram Reels 采集短视频，用本地 AI（Ollama Vision/LLM + Whisper ASR）完成理解、分类、筛选与文案生成，辅助运营者发布到抖音。**人机协作工具，不是无人值守机器人**——最终发布按钮永远由用户本人点击，代码库中不存在自动点击发布的路径。

设计文档在 `docs/01~05`（需求/架构/技术选型/目录结构/里程碑）。注意 `docs/02-architecture.md` 中两处已与实现偏离，**以代码为准**：
- 浏览器自动化不是 Playwright 独立 Profile，而是 **opencli 桥接用户已登录的 Chrome**（用户明确要求）；
- 前后端没有 event 推送，前端是**轮询**（taskStore 每 2.5s 拉 `list_tasks`）。

## 常用命令

```powershell
# 开发（根目录，自动拉起前端 vite + Tauri）
pnpm tauri dev

# 打包正式版（产物在 src-tauri/target/release/bundle/）
pnpm tauri build

# 后端测试 / lint（先停掉运行中的 app，否则 Windows 下 exe 被锁导致链接失败）
Get-Process build_douyin_acount,cargo -ErrorAction SilentlyContinue | Stop-Process -Force
cd src-tauri
cargo test                                    # 全部测试
cargo test --test agents_test                 # 单个测试文件
cargo test filter_english                     # 按名字过滤
cargo clippy --all-targets                    # 要求零警告

# 真实环境联调（不走测试桩，需 Ollama/FFmpeg 就绪）
cargo run --example e2e_analysis -- <video.mp4>

# GUI 开发期灌模拟数据（10 条示例资产含模拟分析结果，幂等，不依赖真实 AI）
cargo run --example seed_sample_data [sample_video.mp4]

# 前端
cd frontend
pnpm dev          # vite，端口固定 1420（strictPort）
pnpm test         # vitest run 全部
pnpm vitest run src/lib/format.test.ts   # 单个测试文件
pnpm build        # tsc 类型检查 + vite 打包（无 eslint，类型检查靠它）
```

前置外部依赖：Ollama（`qwen2.5vl:7b` + `qwen2.5:7b`）、FFmpeg、yt-dlp、Node（sidecar）、Whisper（`scripts/whisper-server/.venv`，faster-whisper Python 环境）、opencli（浏览器桥接，daemon 端口 19825）。路径均可在应用设置页/`app_settings` 表配置。

## 架构要点

### 分层与进程模型

- **React 前端只做 UI**：`frontend/src/api/client.ts` 封装全部 `invoke` 调用；后端 `DomainError` 统一转成前端 `ApiError { code, message }`。
- **Rust 主进程承载全部业务逻辑**：commands（IPC 校验）→ agents（任务编排）→ analysis/collector/publish（能力实现）→ db/repos（rusqlite）。
- **Node sidecar 做浏览器与下载**：`scripts/collector/collect.js`（opencli 刷推荐流 + yt-dlp 下载）、`scripts/douyin-publisher/publish.js`（抖音 creator 后台填表）。Rust Agent 用 `Command` spawn sidecar，**通过文件异步通信**：sidecar 写 `progress.json` / `videos.jsonl` / `imported.jsonl` 到 `storage/collect/<task_id>/`，Agent 每轮轮询读文件判断进度。
- **AI 能力在外部进程**：Ollama（HTTP）、Whisper（Python 脚本子进程）、FFmpeg（子进程）。

### Agent Runtime（核心机制）

- 统一契约 `agents/mod.rs`：`trait Agent { fn name(); fn run(ctx, task) -> Result<AgentOutcome> }`，`AgentOutcome` 三态：`Succeeded(json)` / `WaitingForHuman(json)` / `Continue`（保持 RUNNING，下轮继续轮询——sidecar 长任务全靠这个）。
- 任务持久化在 SQLite `agent_tasks` 表（PENDING/RUNNING/WAITING_FOR_HUMAN/SUCCEEDED/FAILED/CANCELLED + 指数退避重试），应用重启可续跑。
- `lib.rs` setup 起两个后台线程，每 2s 各调一次 `runtime.run_once(...)`：一个专跑 `vision`，另一个轮询 `publisher`/`collector`/`content`。**同一 Agent 串行**（本地 AI 资源有限）。
- `AppState::build_runtime()` / `build_pipeline()`（`state.rs`）每次调用时**按当前设置现场装配** agent 和管线——设置改动即时生效，无需重启。

### 分析管线与状态机

`AnalysisPipeline`（`analysis/pipeline.rs`）= FFmpeg 抽帧 → Whisper ASR → Ollama Vision 逐帧 → LLM 综合成严格 JSON，全部能力抽象为 trait（`FrameExtractor`/`Asr`/`Vision`/`Llm`），测试用桩实现注入。提示词模板在 `resources/prompts/`（`vision-frame.md` 单帧分析、`vision-synthesize.md` 帧+ASR 综合——`docs/04` 列出的 filter/content 提示词不存在，文案生成 prompt 内嵌在 `agents/content.rs`）。

资产状态机：`NEW → ANALYZING → CANDIDATE / ELIMINATED → SELECTED → PUBLISHED`（另有 `LOW_PRIORITY` / `SKIPPED` 等）。Filter 判定顺序：排除条件命中 → 淘汰；命中条件全未命中 → 淘汰；`quality_score < quality_min_keep` → LOW_PRIORITY。命中/排除条件由用户在设置页配置（空列表 = 不限制），LLM 逐条判定结果存 `analysis_results.condition_results_json`。

### 数据与存储

- SQLite 单文件 `storage/db/app.db`，schema 在 `db/migrations.rs`（启动时执行，含旧库补列逻辑）。`storage/` 是运行时数据，gitignored；`storage_root()` 定位逻辑见 `lib.rs`（开发期取仓库根 `storage/`）。
- 核心表：`video_assets`（1:1）`analysis_results`，（1:N）`publish_drafts`；`agent_tasks`（1:N）`task_logs`；`app_settings` 单行 JSON。

## 关键约定（踩过的坑，别再踩）

- **`app_settings` 行是完整 JSON**：缺字段会导致反序列化失败、整体回退默认值。改设置必须写全所有字段。
- **`AssetStatus` serde 必须 SCREAMING_SNAKE_CASE**（曾因序列化配置把 `LOW_PRIORITY` 变成 `LOWPRIORITY`）。
- **`window.confirm` 在 WebView2 会静默返回 false**：删除等确认必须用 `@tauri-apps/plugin-dialog` 的 `ask()`。
- **PowerShell 内联含中文的脚本会因 GBK 崩溃**：含中文的脚本一律写成文件再执行；读 `docs/` 的 UTF-8 文档用 `Get-Content -Encoding UTF8`。
- **测试不依赖真实外部进程**：`src-tauri/tests/fixtures/` 有 `fake_collect.cjs` / `fake_publish.cjs`（假 sidecar）和 `sample_1s.mp4`；测试用 `tempfile::tempdir()` 建独立 storage。新加 collector/publisher 相关测试时复用这套桩。
- **采集完成统计以 `imported.jsonl` 行数为准**：sidecar 逐条追加 `videos.jsonl`、Agent 在 RUNNING 阶段已增量导入大部分视频，任务 Done 时若只统计最终轮新增会把累计数算成 0/1（`imported` 与 `first_asset` 都犯过）。统计须在清理临时目录前读 `imported.jsonl`；回归测试 `collector_imported_is_task_total_not_final_round`（`fake_collect_batch.cjs` 分批时序）守护此行为。
- **Whisper ASR 会静默失败**：sidecar 是 `scripts/whisper-server/whisper_transcribe.py`（faster-whisper），`whisper_python` 必须解析为绝对路径（相对路径在应用进程里找不到脚本）；CUDA 库缺失时已做 CPU 回退。改 ASR 相关代码后必须用真实视频验证一次转写，失败不会抛出明显错误。
- **本机调试脚本（`storage/logs/*.py`，gitignored，仅此机器存在）**：`cdp_helper.py`（经 WebView2 CDP 驱动运行中的应用：`list|eval|click|shot`，需启动 app 前设 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--remote-debugging-port=9222 --remote-allow-origins=*'`）、`status_snapshot.py`（库/任务快照）、`watch_progress.py`（盯采集 progress.json）、`export_ig_cookies.py`（IG 下载 cookie 导出，需先完全关闭 Chrome）。
- **安全红线（验收标准）**：不得引入对抖音发布按钮的自动点击；发布完成标记只能由用户手动确认触发；自动化识别不到表单元素时必须停止并提示，不得盲目操作。

## 提交规范

中文 commit message，格式 `type(范围): 描述`，如 `feat(分析): LLM 逐条判定命中/排除条件并随分析结果入库`。type 用 feat/fix/test/docs/refactor 等。
