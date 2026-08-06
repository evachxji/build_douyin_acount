# 分阶段实施计划

每个阶段以"编译运行 → 测试 → 输出进度 → 等待指令"收尾。

## 第一阶段：需求分析与架构设计（完成）

- [x] 需求分析、系统架构、技术选型、目录结构文档
- [x] 创建 GitHub 同名私有仓库 `build_douyin_acount` 并推送（<github-account>/build_douyin_acount）
- [x] 初始化 Tauri 2 + React 脚手架（空壳可编译运行）

验证：

- 文档与需求逐条可追溯
- `cargo tauri dev` 能启动空窗口

## 第二阶段：数据库 + 视频管理 + AI 分析 Pipeline

### 2.1 数据库与领域模型

- [x] migrations：video_assets / analysis_results / account_styles / publish_drafts / agent_tasks / task_logs / recommendations
- [x] Repository 层与单元测试（CRUD、状态流转约束）

验证：`cargo test` 全绿；SQLite 文件在 `storage/db/app.db` 正确生成。

### 2.2 视频管理服务

- [x] 文件存储（videos / covers / frames / audio）路径管理
- [x] 资产导入（手动拖入本地视频）与删除（含磁盘文件清理）
- [x] 收藏、标记发布、状态流转命令

验证：CLI 测试脚本 + Rust 集成测试覆盖导入/删除/状态流转。

### 2.3 AI 分析管线

- [x] FFmpeg 抽帧与音频提取（含超时与错误处理）
- [x] Whisper sidecar（ASR + 语言识别）
- [x] Ollama Vision 帧分析（结构化 JSON）
- [x] LLM 综合与 JSON 校验
- [x] Vision Agent 任务编排 + 队列断点续跑
- [x] Filter Agent（英文判定阈值化）

验证：

- 用 2~3 条真实本地视频跑通全管线
- 单条分析失败不阻塞队列；重启后任务恢复
- 输出 JSON 字段与设计一致

> 备注：真实模型端到端联调（Ollama 拉取 Vision/LLM 模型）待模型就绪后执行；管线编排已由桩组件集成测试覆盖。

### 2.4 脚手架（第一阶段遗留项，随第二阶段完成）

- [x] Tauri 2 + React 18 + Tailwind 脚手架可编译运行
- [x] 根级 `package.json` 统一 `pnpm tauri dev/build` 入口
- [x] 冒烟验证：vite 启动 → Rust 编译 → 窗口正常打开

## 第三阶段：GUI + 视频浏览

- [x] Tauri commands 全量暴露（assets / analysis / tasks / settings；collection / drafts 随第四、五阶段实现）
- [x] 三栏布局：分类树 / 瀑布流 / 详情
- [x] 视频播放、多选、收藏、删除、状态徽章
- [x] 任务队列面板 + 人工审批对话框
- [ ] 采集进度可视化（目标条数可配置，默认 100 条，TikTok/IG 各自进度；随采集模块实现）

验证：

- [x] 使用种子数据（10 条：IG 7 / TT 3）浏览操作：分类/状态/平台/收藏/搜索筛选均正确
- [x] 详情面板视频可播放（asset protocol 加载，readyState=4）；收藏持久化到库
- [x] `pnpm test`（5 个）+ `cargo test`（49 个）+ `cargo clippy --all-targets` 零告警
- [x] `pnpm tauri dev` 冒烟：窗口正常打开，三栏渲染无加载错误（CDP 校验 + 截图）
- [ ] `cargo tauri build` 发布包构建（随第四阶段一并执行）

## 第四阶段：抖音发布辅助

- [x] 浏览器编排：opencli 在用户已登录的 Chrome 中新开上传页标签（登录态复用，无需重新登录）
- [x] 上传弹窗由用户手动操作：程序展示视频路径（一键复制）+【已选择】按钮
- [x] Publisher Agent 流程：`prepare_publish` → 人工选文件（自动检测页面变化）→ 自动填标题/文案/话题 → WAITING_FOR_HUMAN → 人工确认发布
- [x] GUI 发布流程面板与人工确认（选文件信息弹窗精简为标题/路径/复制；发布确认弹窗）

验证（联调）：

- [x] opencli 会话登录态确认（whoami：<douyin-account>）+ 上传页打开正常
- [x] GUI 全流程冒烟（假 sidecar）：准备发布 → 选文件信息弹窗 → 自动填表 → 我已发布 → 资产 PUBLISHED
- [x] 真实侧边车验证到「选文件」阶段；完整真实上传/填表/发布由用户手动选择文件后联调
- [x] 代码审计：sidecar 与 Agent 无任何点击发布按钮的代码路径；发布完成仅由「发布成功」提示或用户确认产生

## 第五阶段（建议）：推荐算法与端到端联调

- [x] 配额配置（每轮条数 / TT 比例 / IG 比例 / 点赞上限，默认 20 条 / 30% / 70% / 100 万）
- [x] 采集器（TikTok explore 推荐流 + IG Reels 播放器，yt-dlp 下载，点赞过滤，自动入库+入队分析）
- [x] 推荐打分（质量加权 + 点赞对数归一 + 作者多样性，写入 recommendations 表）
- [x] Content Agent（按账号风格生成标题/正文/话题；prepare_publish 优先使用）
- [x] 每轮全量实测（20 条/轮，TT 6 / IG 14）
- [x] 完整人机协作闭环：采集 → 分析 → 推荐 → 勾选 → 文案 → 发布

验证（2026-08-06 完成一轮）：

- 采集：TT 6 + IG 14 共 20 条全部入库；IG 侧因 Chrome app-bound 加密无法导出 cookie，
  改用「播放器采码 → 单 reels 页 info API 取 CDN 直链 → 直接下载」方案（实测 ~1.3MB/s）
- 分析：20/20 完成，候选 15 / 淘汰 5；修复 vision 重试时 `ANALYZING -> ANALYZING` 非法迁移
- 推荐：推荐清单输出 12 条（按分类质量+点赞排序），自动生成文案任务并全部成功
- 发布：asset 5 经人工勾选「通过」→ 准备发布 → 用户本人核对后在浏览器点击发布 → 标记 PUBLISHED；
  全程代码无自动点击发布按钮路径，发布完成仅由用户确认触发
- 「我已发布」确认按钮增加 loading 动画与禁用态，避免重复点击

## 验收红线（贯穿所有阶段）

1. 绝不自动点击抖音最终发布按钮。
2. 采集/发布的浏览器与用户当前浏览器完全隔离。
3. 点赞超上限的视频不采集（上限可配置，默认 >100 万跳过）。
4. 采集配额可配置：默认每轮 100 条、TikTok 30% / Instagram 70%。
