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
- [ ] 采集进度可视化（目标 100 条，TikTok/IG 各自进度；随采集模块实现）

验证：

- [x] 使用种子数据（10 条：IG 7 / TT 3）浏览操作：分类/状态/平台/收藏/搜索筛选均正确
- [x] 详情面板视频可播放（asset protocol 加载，readyState=4）；收藏持久化到库
- [x] `pnpm test`（5 个）+ `cargo test`（49 个）+ `cargo clippy --all-targets` 零告警
- [x] `pnpm tauri dev` 冒烟：窗口正常打开，三栏渲染无加载错误（CDP 校验 + 截图）
- [ ] `cargo tauri build` 发布包构建（随第四阶段一并执行）

## 第四阶段：抖音发布辅助

- [ ] Playwright sidecar 独立窗口（持久化 Profile）
- [ ] creator.douyin.com 页面对象：上传、填标题/文案/标签
- [ ] Publisher Agent 流程：`prepare_publish` → 填表 → WAITING_FOR_HUMAN
- [ ] GUI 发布流程面板与人工确认

验证（联调）：

- 独立窗口可登录（人工）
- 表单自动填写正确且停在发布按钮前
- 代码审计：无发布按钮自动点击

## 第五阶段（建议）：推荐算法与端到端联调

- [ ] 推荐打分（加权 + 多样性 + LLM 风格加成）
- [ ] 每轮 100 条采集全流程实测（30/70 比例、百万赞过滤）
- [ ] 完整人机协作闭环：采集 → 分析 → 推荐 → 勾选 → 文案 → 发布

验证：真实跑一轮，输出每标签推荐清单供用户挑选。

## 验收红线（贯穿所有阶段）

1. 绝不自动点击抖音最终发布按钮。
2. 采集/发布的浏览器与用户当前浏览器完全隔离。
3. 点赞 > 100 万视频不采集。
4. 每轮固定 100 条、TikTok 30% / Instagram 70%。
