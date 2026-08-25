# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目概述

PC 端 AI 短视频运营助手（Windows 桌面应用）：从 TikTok / Instagram Reels 采集短视频，用本地 AI（Ollama Vision/LLM + Whisper ASR）完成理解、分类、筛选与文案生成，辅助运营者发布到抖音。**人机协作工具，不是无人值守机器人**——最终发布动作永远由用户在应用内对单个任务显式点勾授权触发（sidecar 据此在页面代点一次发布按钮），代码库中不存在自动/定时/批量/无人值守触发发布点击的路径。

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
- **Node sidecar 做浏览器与下载**：`scripts/collector/collect.js`（opencli 刷推荐流 + yt-dlp 下载）、`scripts/douyin-publisher/publish.js`（抖音 creator 后台填表：正文 fill + 话题逐字键入并点选联想项、自动执行「自主声明→内容为转载信息→取材站外→确定」）。Rust Agent 用 `Command` spawn sidecar，**通过文件异步通信**：sidecar 写 `progress.json` / `videos.jsonl` / `imported.jsonl` 到 `storage/collect/<task_id>/`，Agent 每轮轮询读文件判断进度。
- **AI 能力在外部进程**：Ollama（HTTP）、Whisper（Python 脚本子进程）、FFmpeg（子进程）。

### Agent Runtime（核心机制）

- 统一契约 `agents/mod.rs`：`trait Agent { fn name(); fn run(ctx, task) -> Result<AgentOutcome> }`，`AgentOutcome` 四态：`Succeeded(json)` / `WaitingForHuman(json)` / `Cancelled(json)`（用户中止，如关闭浏览器标签页，置 CANCELLED 终态不重试）/ `Continue`（保持 RUNNING，下轮继续轮询——sidecar 长任务全靠这个）。
- 任务持久化在 SQLite `agent_tasks` 表（PENDING/RUNNING/WAITING_FOR_HUMAN/SUCCEEDED/FAILED/CANCELLED + 指数退避重试），应用重启可续跑。
- `lib.rs` setup 起两个后台线程，每 2s 轮询：一个专跑 `vision`（`run_once`），另一个跑 `publisher`（`run_active`，**并行模型**——每轮经 `TasksRepo::claim_all_active` 驱动全部活跃发布任务；publisher 不涉本地 AI，sidecar 会话与 progress/cancel/confirm 标记文件均按 draft_id 隔离，天然可并行）与 `collector`/`douyin_sync`/`douyin_moderate`/`douyin_export`/`content`（`run_once`）。**除 publisher 外同一 Agent 串行**（本地 AI 资源有限）。
- `AppState::build_runtime()` / `build_pipeline()`（`state.rs`）每次调用时**按当前设置现场装配** agent 和管线——设置改动即时生效，无需重启。

### 分析管线与状态机

`AnalysisPipeline`（`analysis/pipeline.rs`）= FFmpeg 抽帧 → Whisper ASR → Ollama Vision 逐帧 → LLM 综合成严格 JSON，全部能力抽象为 trait（`FrameExtractor`/`Asr`/`Vision`/`Llm`），测试用桩实现注入。提示词模板在 `resources/prompts/`（`vision-frame.md` 单帧分析、`vision-synthesize.md` 帧+ASR 综合——`docs/04` 列出的 filter/content 提示词不存在，文案生成 prompt 内嵌在 `agents/content.rs`）。

资产状态机：`NEW → ANALYZING → CANDIDATE / ELIMINATED → SELECTED → PUBLISHED`（另有 `LOW_PRIORITY` / `SKIPPED` 等）。Filter 判定顺序：排除条件命中 → 淘汰；命中条件全未命中 → 淘汰；`quality_score < quality_min_keep` → LOW_PRIORITY。命中/排除条件由用户在设置页配置（空列表 = 不限制），LLM 逐条判定结果存 `analysis_results.condition_results_json`。

### 数据与存储

- SQLite 单文件 `storage/db/app.db`，schema 在 `db/migrations.rs`（启动时执行，含旧库补列逻辑）。`storage/` 是运行时数据，gitignored；`storage_root()` 定位逻辑见 `lib.rs`（开发期取仓库根 `storage/`）。
- 核心表：`video_assets`（1:1）`analysis_results`，（1:N）`publish_drafts`；`agent_tasks`（1:N）`task_logs`；`app_settings` 单行 JSON；`douyin_posts`（抖音作品快照，发布日历数据源，`aweme_id` UNIQUE 幂等 upsert）。

## 关键约定（踩过的坑，别再踩）

- **`app_settings` 行是完整 JSON**：缺字段会导致反序列化失败、整体回退默认值。改设置必须写全所有字段。
- **`AssetStatus` serde 必须 SCREAMING_SNAKE_CASE**（曾因序列化配置把 `LOW_PRIORITY` 变成 `LOWPRIORITY`）。
- **`window.confirm` 在 WebView2 会静默返回 false**：删除等确认必须用 `@tauri-apps/plugin-dialog` 的 `ask()`。
- **PowerShell 内联含中文的脚本会因 GBK 崩溃**：含中文的脚本一律写成文件再执行；读 `docs/` 的 UTF-8 文档用 `Get-Content -Encoding UTF8`。
- **测试不依赖真实外部进程**：`src-tauri/tests/fixtures/` 有 `fake_collect.cjs` / `fake_publish.cjs`（假 sidecar）和 `sample_1s.mp4`；测试用 `tempfile::tempdir()` 建独立 storage。新加 collector/publisher 相关测试时复用这套桩。
- **采集完成统计以 `imported.jsonl` 行数为准**：sidecar 逐条追加 `videos.jsonl`、Agent 在 RUNNING 阶段已增量导入大部分视频，任务 Done 时若只统计最终轮新增会把累计数算成 0/1（`imported` 与 `first_asset` 都犯过）。统计须在清理临时目录前读 `imported.jsonl`；回归测试 `collector_imported_is_task_total_not_final_round`（`fake_collect_batch.cjs` 分批时序）守护此行为。
- **Whisper ASR 会静默失败**：sidecar 是 `scripts/whisper-server/whisper_transcribe.py`（faster-whisper），`whisper_python` 必须解析为绝对路径（相对路径在应用进程里找不到脚本）；CUDA 库缺失时已做 CPU 回退。改 ASR 相关代码后必须用真实视频验证一次转写，失败不会抛出明显错误。
- **Whisper 模型内置化契约（2026-08 新增）**：模型是程序内置基座，统一存 `storage/models/whisper/<名称>/`（tiny/base/small/medium/large-v3 五档，注册表在 `src/whisper_models.rs`，文件清单已核实 Systran/faster-whisper-* 仓库），不再依赖用户预装的 HF 全局缓存。安装判据 = 清单文件齐全且 `model.bin > 1MB`（完整文件由 `.part` rename 而来，天然原子）。`state.rs::build_pipeline` 把已安装模型解析为**目录绝对路径**传给转写脚本（faster-whisper 原生支持本地目录，Python 脚本不用改）；未安装/自定义值原样透传（faster-whisper 回退自身缓存，向后兼容）。下载走**专用线程 + reqwest（不走 agent runtime）**：`.part` 断点续传（Range 请求，响应 200 非 206 则截断重来）、每 chunk 30s 读超时、进度写 `models/whisper/.progress_<名称>.json`（tmp+rename，前端 `list_whisper_models` 1.5s 轮询；非终态且 mtime>10s 判 `interrupted`）。路由回退：`auto` 模式官方源（huggingface.co）优先，单文件失败或均速 <50KB/s 持续 20s 自动切 hf-mirror.com 续传（不消耗 3 次尝试次数）。全局单下载（`ACTIVE_DOWNLOAD` Mutex），HEAD 求总大小时**必须手读 Content-Length 头**（reqwest 对 HEAD 响应 `content_length()` 恒为 0）。回归测试 `whisper_models_test.rs`（本地 TcpListener mock server）守护。
- **「手工淘汰」是最高优先级，AI 分析不得复活素材**：`update_asset_status` 手工淘汰时写 `eliminate_reason="手工淘汰"`，离开淘汰态清除。重新 `enqueue_analysis`（`commands/analysis.rs`）会先清除该标记——用户主动重分析可推翻手工淘汰；但 vision Agent（`agents/vision.rs`）在分析前与分析后都会复查最新 `eliminate_reason`，一旦是「手工淘汰」就返回 `AgentOutcome::Cancelled`，绝不把素材升回 CANDIDATE。改分析流程时保持「用户否决 > AI 结论」，回归测试 `vision_agent_skips_manually_eliminated_asset` 守护此行为。
- **本机调试脚本（`storage/logs/*.py`，gitignored，仅此机器存在）**：`cdp_helper.py`（经 WebView2 CDP 驱动运行中的应用：`list|eval|click|shot`，需启动 app 前设 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--remote-debugging-port=9222 --remote-allow-origins=*'`）、`status_snapshot.py`（库/任务快照）、`watch_progress.py`（盯采集 progress.json）、`export_ig_cookies.py`（IG 下载 cookie 导出，需先完全关闭 Chrome）。
- **publisher 心跳契约（僵尸恢复）**：sidecar 每 5s 重写 progress 文件（心跳），agent 按文件 mtime 判活——`browser_started`/`uploading`/`waiting_for_file` 超过 45s 未更新视为 sidecar 已死（典型场景：应用重启杀死子进程，任务卡 RUNNING 无法推进），重拉 sidecar（同名会话 `closeSession` 先清旧标签页）最多 2 次，仍无心跳则任务置 CANCELLED（素材保持 SELECTED 可重新发起）。`form_filled` 同样判定但不重拉（重拉会重新上传填表、且用户可能已在页面手动发布）：点勾代点依赖 sidecar 存活，冻结直接置 CANCELLED 并提示核实抖音后台状态（素材同样保持 SELECTED）。回归测试 `publisher_respawns_when_sidecar_heartbeat_stale`、`publisher_cancels_when_form_filled_sidecar_stale` 等守护此行为。
- **publisher 自动封面与定时契约**：填表后 sidecar 先设定时（快）再等封面（慢），两步失败仅 warn 提示人工，不阻断、不盲目操作：② 点「定时发布」label（`data-checked` 判选中态），`fill` 时间输入框（placeholder「日期和时间」）+ Enter 让 semi DatePicker 解析 + 读回 value 校验；① AI 推荐封面——**就绪判据是封面容器内无任何「生成中」可见文本且带图封面项 ≥1**（生成中已有带 blob 图的半成品封面项，不能只看有无项；生成耗时 20s~2min+ 不等）。等待用 Node 侧轮询瞬时 eval（单次 eval 有 **CDP 115s 上限**，页面内长 await 不可行），上限 150s。就绪后 opencli `click '[class*="recommendCover-"]' --nth 0`（CDP 真实鼠标；**类名关键词必须带连字符**——`recommendCover` 子串会误匹配 `recommendCoverContainer`，曾导致点击落在容器上无效），**点击后必弹「是否确认应用此封面？」，须在弹窗容器内按文本点【确定】**（之前漏掉弹窗，一切点击都看似无效；定时作品**编辑页**该弹窗点确定也不生效，验证只能在新发布页/端到端做）。成功校验三信号：主封面 img src 变化 / 第一项出现 `selected-` 类 / 第一项出现 svg 红勾（端到端实测三重命中）；全不命中报 warn，不把「无法确认」当成功。**选中应用后另一个方向（横/竖）的槽位封面会进入二次自动生成**（「设置封面」槽位区出现「生成中」占位块）：`waitCoverSlotsReady` 以「设置封面」标题为锚向上找含「横封面|竖封面」标签的槽位区容器，等其内「生成中」可见文本消失（上限 180s）才写 `form_filled` 弹确认窗——曾在首个封面选中后立即弹窗，用户核对时槽位封面还是半成品；超时仅 warn 不阻断（`slotTitlePattern`/`slotLabelPattern` 在 selectors.json 可配）。定时目标时刻由 agent 端 `pick_schedule_time` 算好经 job.json `scheduleTime` 传入：今天起 14 天内按**三层覆盖**挑日（数据源 `DouyinPostsRepo::list_day_video_minutes` + **并发占位**——其他活跃 publisher 任务 payload 的 `schedule_time` 经 `TasksRepo::list_active_schedule_times` 合并进 day_minutes，排除本任务；任务 finalize 前其定时不在日历里，必须补位后续任务才能顺延；日历口径私密不占位，定时作品按定时发布日占位）——① 最近一个**无公开视频的本地日**，当天 08:00~19:00 随机分钟（先保证每天 1 个）；② 14 天都有视频后，最近一个**恰好 1 个**的日子，按 上午(08:00~12:00)→下午(12:00~19:00)→晚上(19:00~22:00) 顺序取第一个未被占用时段（已有上午→下午、已有下午→上午的互补由此自然得出）；③ 同理给恰好 2 个的日子补第 3 个。时刻须满足抖音「**2 小时后及 14 天内**」：仅今天要求起始分钟 ≥ ceil(now+2h)（跨午夜则今天整日均不可选），目标时段今天窗口不足顺推下一空闲段（上午/下午不足 → 晚上），三段都不足跳过今天；14 天 × 3 全满传 null 提示人工。回归测试 `schedule_*` 与 `publisher_spawn_writes_schedule_time_in_job`（`publisher_test.rs`）守护。另注意：**opencli eval 对字符串返回值输出裸文本（非 JSON）**，读字符串结果必须用 `runOpencliRaw`——既有 topic/declaration 脚本用 `runOpencli`（JSON.parse）读字符串结果会静默落入 catch，其「未自动完成」的 warn 可能是误报（未实测确认，2026-08 探测时发现）。
- **publisher 发布确认与日历登记契约**：表单填完后任务保持 RUNNING（payload `stage=form_ready` + sidecar 消息透传），侧栏「发布中」列表行由转圈变为 ✓/✕（不再弹悬浮窗）。用户在侧栏点 ✓ 授权（直接生效，无二次确认，写入 confirm 标记）后，sidecar `monitorAfterFill` 据此**代点一次发布按钮**（`buildPublishClickScript` 按 selectors.json `publishButton.labels` 候选文本「发布/定时发布」精确匹配可见且未 disabled 的 button，识别不到或多个歧义一律不点并提示人工；`confirmHandled` 保证整个监控期只点一次；代点后 90s 无成功信号提醒检查页面，人工在页面补点仍被检测兜底）；用户也可全程自行在浏览器点【发布】。sidecar 每 3s 轮询「发布已提交」信号（被动检测）：① 主信号——一次 `tab list` 同时判活与取当前 URL（**不能用 `get url`**：无活页会话会触发 daemon 建幽灵页），跳转到 `/creator-micro/content/manage` 即判定（「发布成功」toast 一闪而过，3s 轮询可能错过，URL 跳转是可靠的滞后证据）；② 辅助信号——`successIndicators` 全部候选逐个 find。**发布信息捕获**：填表完成后 sidecar 注入 fetch/XHR 钩子（`injectPublishHook`），凡 `/media/(aweme|item|video)` 族接口（草稿保存与发布提交同族）响应带 aweme_id/item_id 时，把页面实际值（标题/定时时间/封面 canvas 截图）快照进 localStorage（同源持久，跳转后可读；30 分钟有效期防陈旧）；发布判定时读回，封面 dataURL 由 sidecar 落盘 `<covers>/local_<draftId>.jpg`，随 progress `post` 字段上报。agent `finalize_published` 据此**直接 upsert `douyin_posts`，日历即时更新（不再入队整轮 sync）**：定时时间在未来 → `scheduled`（`create_time`=定时时间、`public_time`=0），已过或缺失 → `published`；捕获缺失回退 draft/job.json 值，`aweme_id` 缺失用 `local-draft-<draft_id>` 占位——下次整轮 sync 拉到真实行后按「标题相同+发布日期相同」清理占位（`DouyinPostsRepo::delete_local_placeholders`，仅匹配 local- 前缀；真实行已被 sync 抢先拉取时不覆盖其统计）。**所有结束路径只 `unbind` 释放租约、标签页保留给用户**（published/cancelled 相同）；`browser_closed` 是用户自己关页后的租约清理，非主动关闭。误判边界：form_filled 阶段用户自己手动导航到内容管理页会被判为已发布（可接受：表单已填完跳走是反常操作，progress 消息里写明判定依据 URL）。回归测试 `publisher_confirm_marker_alone_does_not_finalize`、`publisher_finalizes_when_sidecar_detects_success`、`publisher_registers_past_schedule_time_as_published`、`douyin_sync_*placeholder*` 守护。
- **douyin_sync（发布日历同步）契约**：sidecar `scripts/douyin-sync/sync.js` 借 opencli 在创作者中心页面 eval-fetch `janus/douyin/creator/pc/work_list`——**只认 `max_cursor` 游标翻页（`page_size`/`page_num` 被忽略，页大小用 `count`），且必须带 `scene=star_atlas&device_platform=android&aid=1128` 全参数，裸参数响应不含定时发布作品**。响应两个数组：`aweme_list`（定时+已发布合并，**定时作品的 `create_time` 即定时发布时间、`public_time=0`，`status_value=140` 与私密作品共享不能作判据，`create_time > now` 才是**）与 `items`（本页**已发布子集**，按 `create_time` 一对一 join——`items[].id` 是 f64 超 MAX_SAFE_INTEGER 已截断不可作 join 键）；**完播率/2秒跳出率在 `items[].metrics`（`completion_rate`/`bounce_rate_2s`，0-1 小数字符串，×100 入库），定时作品无此数据**；**违规标记在 `items[].review`（健康 `status=2` 且 `details` 空；流量减少 `status=5`，`details[0].text/url` 为审核摘要与申诉链接），违规原因（如「原创性不足」）需逐条调 `GET creator.douyin.com/aweme/v1/review/result/?object_id=<id>&scene=10019`（同源可走页面 eval-fetch；响应 `review_detail.fields` 里 `reason_detail`/`multi_reason_detail` 的 `[0].brief`），已存 brief 的作品经 `job.knownViolations` 跳过补抓；upsert 时 status/text/url 覆盖（违规消解即清空），brief 消解前 COALESCE 保留**。封面是签名 URL（x-expires 约 13 天过期），同步时即下载到 `covers/douyin_<aweme_id>.<ext>`（平铺命名，备份 zip 自动包含），upsert 用 COALESCE 保留旧封面，且每轮用 `datetime('now')` 刷新 `synced_at`（前端「上次同步」按字符串取 max，全表格式必须统一为 SQLite 原生格式，不得改 RFC3339）。心跳/僵尸恢复/取消契约与 collector 相同（60s mtime 判死）。回归测试 `douyin_sync_test.rs` 守护。
- **opencli 后台化契约（2026-08 实测调通）**：后台型 sidecar（collect/douyin-sync/moderate）启动时设两个环境变量（子进程继承）：`OPENCLI_WINDOW=background`（opencli `browser` 命令默认 foreground 会抢焦点；background 下 open/get url/eval 全程不切前台）与 `OPENCLI_BROWSER_IDLE_TIMEOUT=604800`（会话租约保活 7 天，默认仅 10 分钟）。**任务结束用 `parkSession()` 驻留 about:blank，不再 `closeSession()` 关标签页**——opencli 扩展的「容器窗口」模型下，关最后一个标签页会连窗口一起关掉，下次任务被迫 `windows.create` 新建可见窗口闪到前台（用户投诉的根源）；驻留后下次 `open` 复用同一标签页导航（page id 不变，不累积）。会话名因此固定（`dy-sync`/`dy-mod`/`collect-<platform>`，不带 taskId）。容器窗口不存在时 open 仍会新建窗口：sidecar 用 `minimizeNewChromeWindows()` 对比 open 前后 `Get-Process chrome` 的 MainWindowHandle 把新建窗口最小化（已存在的用户窗口绝不动；最小化窗口句柄会掉出枚举，只可能漏抓不可能误伤）。实测：容器存在时同步全程前台纹丝不动；容器缺失时新窗口 `focused:false` 不抢焦点并被立即最小化。已知残留风险：扩展在容器状态错乱时可能「收养」用户窗口并把其首个标签页导航成 about:blank（opencli 上游问题，本应用无法防御，已报 jackwener/OpenCLI#2364）。**publish.js 故意保持 foreground**——用户要看填好的表单并手动点发布。另注意：对无活页的会话发命令（如 `get url`），daemon 会自动创建 about:blank 幽灵页（parkSession 利用它当驻留页）。
- **douyin_moderate（违规处理）契约**：sidecar `scripts/douyin-sync/moderate.js` 两个写接口均为表单 POST（2026-08 从管理页弹窗抓包确认）：① 改权限 `POST /web/api/media/aweme/update/`，body `item_id=<id>&download=1&visibility_type=1&xg_user_id=0&dx_upgraded=0`（visibility_type 1=仅自己可见）；② 定时改立即发布 `POST /web/api/media/update/timer/`，body `item_id=<id>&timing=<now+10s>`（与「修改定时」弹窗的「立即发布」同接口，时间范围支持当前到未来 14 天）。成功判据 `status_code==0`；**权限修改未 ack 成功绝不提升定时作品**。处理成功后 agent 自动入队一轮 douyin_sync 刷新状态。「立即发布」只在两条已授权路径触发：① 用户点击日历浮窗【一键处理/处理所有违规视频】并经 `ask()` 确认弹窗二次确认；② 顶栏【自动巡检】开关开启时的 30 分钟自动轮次（开关即用户预先授权，2026-08-24 用户明确确认包含此步）。回归测试 `douyin_moderate_test.rs` 守护。
- **自动巡检契约（30 分钟同步 + 违规自动处理）**：顶栏刷新按钮左侧开关，状态存 `app_settings` 独立 key `auto_sync_moderate`（**与 AppSettings 完整 JSON 分离**，互不影响；默认关）。`auto_sync.rs` 的 `maybe_enqueue` 由 lib.rs 第三个后台线程每 60s 调用：开关开 + 无活跃 sync/moderate 任务 + 距上次 sync 任务创建 ≥30min（SQL `datetime('now','-1800 seconds')` 比较）→ 入队 `douyin_sync` 且 payload 带 `{"source":"auto"}`。联动在 sync agent Done 分支：**仅 `source=="auto"` 轮次**且开关仍开且 `list_violating()` 非空 → 入队 `douyin_moderate`（`{"mode":"all","source":"auto"}`，即手动【处理所有违规视频】同一逻辑）。**防环**：手动 sync（无 source）永不触发；moderate 完成后自动入队的刷新 sync（`json!({})` 无 source）也不再触发——失败遗留的违规由下一个 30 分钟轮次重试。回归测试 `auto_sync_test.rs`（调度）与 `douyin_sync_test.rs` 三个联动用例守护。
- **托盘常驻契约**：点 X 关窗仅隐藏到系统托盘（`lib.rs` 拦截 `CloseRequested`：`prevent_close` + `hide`），后台 worker（分析/发布/巡检）继续运行；仅托盘右键菜单【退出】（`app.exit(0)`）真正结束进程。托盘左键单击或菜单【显示窗口】恢复主窗口（`show_main_window`：show+unminimize+set_focus）。**单实例**（tauri-plugin-single-instance，必须在 plugin 链最先注册）：二次启动被拦截改为显示已有窗口——双开会争用 SQLite 与 opencli 浏览器会话。托盘图标经 `include_bytes!("../icons/icon.ico")` 嵌入（不依赖打包期图标配置，dev/build 均可用），依赖 tauri features `tray-icon` + `image-ico`。
- **顶栏 Ollama 状态标签**：每 5s 探测 `GET /api/version`（`commands/ollama.rs` 的 `check_ollama`，2s 超时）；未启动时点击弹确认，确认后 `start_ollama` spawn `ollama serve`（`CREATE_NO_WINDOW`），前端轮询直至就绪。改 Ollama 连接/启动相关逻辑时保持此契约。
- **内置 yt-dlp 契约**：yt-dlp 不再是外部依赖（`AppSettings.ytdlp_path` 已移除，旧设置 JSON 残留该键由 serde 自动忽略）。内置 exe 落点 `storage/tools/yt-dlp.exe`（备份 zip 不含，丢了重新下载即可），装配入口 `state.rs::build_runtime` 直接传该绝对路径给 CollectorAgent（sidecar 链路不变）。**获取路径只有两条**：① lib.rs setup 调 `tools::ytdlp::ensure_from_bundle` 从打包资源释放（tauri.conf.json `bundle.resources` 打包 `src-tauri/resources/yt-dlp.exe`，该文件 gitignored、由 `scripts/ensure-ytdlp.mjs` 在 beforeDev/beforeBuild 钩子里幂等下载；**仅缺失时释放，绝不覆盖**——用户更新过的新版不能被内置旧版顶掉）；② 设置页「一键更新/下载安装」（`commands/ytdlp.rs` 三个命令：status/check/update——下载到 `.exe.new`、跑 `--version` 验证能运行、rename 轮换 `exe→.old`+`.new→exe`，Windows 下运行中的 exe 可 rename 不可覆盖，`.old` 删不掉不致命）。版本比较必须数值化（`version_key`），字符串比较会把 `2025.8.1` 误判大于 `2025.08.22`。reqwest 查 GitHub API 必须带 UA 且**不配 no_proxy**（走系统代理，GitHub 直连困难时靠它）。回归测试 `ytdlp_test.rs` 守护。
- **备份导入两段式 + `app.restart()` 文件锁竞争**：导入备份（`backup.rs`）运行中只解压校验到 `storage/.import_staging/`，用户确认后写 `.import_pending` 标记，重启在 `Database::open` 前由 `apply_pending_import` 覆盖替换。注意 `app.restart()` **先起新进程、旧进程后退出**——新进程 rename 时旧进程的 SQLite/视频句柄可能未释放（os error 32/5），所以 rename 一律走 `rename_with_retry`（最多等 5s）；失败保留标记与暂存下次启动重试，回滚把已搬入的新物**搬回暂存**（不是删除）。回归测试 `apply_retries_while_target_file_locked_by_old_process` / `apply_keeps_mark_and_staging_when_target_locked_for_good` 守护此行为。
- **安全红线（验收标准）**：发布按钮的点击只能由用户在应用内对单个任务显式点勾授权触发（sidecar 代点一次；2026-08 用户明确修订，取代原「永不自动点击、发布永远由用户本人在浏览器完成」红线）；不得存在自动/定时/批量/无人值守的发布点击路径；发布完成标记只能由「发布成功」信号（页面跳转/成功提示，被动检测、零点击）产生；自动化识别不到表单元素（含发布按钮）时必须停止并提示，不得盲目操作。

## 提交规范

中文 commit message，格式 `type(范围): 描述`，如 `feat(分析): LLM 逐条判定命中/排除条件并随分析结果入库`。type 用 feat/fix/test/docs/refactor 等。
