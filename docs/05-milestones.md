# 分阶段实施计划

每个阶段以"编译运行 → 测试 → 输出进度 → 等待指令"收尾。

## 第一阶段：需求分析与架构设计（当前）

- [x] 需求分析、系统架构、技术选型、目录结构文档
- [ ] 创建 GitHub 同名仓库 `build_douyin_acount` 并推送
- [ ] 初始化 Tauri 2 + React 脚手架（空壳可编译运行）

验证：

- 文档与需求逐条可追溯
- `cargo tauri dev` 能启动空窗口

## 第二阶段：数据库 + 视频管理 + AI 分析 Pipeline

### 2.1 数据库与领域模型

- [ ] migrations：video_assets / analysis_results / account_styles / publish_drafts / agent_tasks / task_logs / recommendations
- [ ] Repository 层与单元测试（CRUD、状态流转约束）

验证：`cargo test` 全绿；SQLite 文件在 `storage/db/app.db` 正确生成。

### 2.2 视频管理服务

- [ ] 文件存储（videos / covers / frames / audio）路径管理
- [ ] 资产导入（手动拖入本地视频）与删除（含磁盘文件清理）
- [ ] 收藏、标记发布、状态流转命令

验证：CLI 测试脚本 + Rust 集成测试覆盖导入/删除/状态流转。

### 2.3 AI 分析管线

- [ ] FFmpeg 抽帧与音频提取（含超时与错误处理）
- [ ] Whisper sidecar（ASR + 语言识别）
- [ ] Ollama Vision 帧分析（结构化 JSON）
- [ ] LLM 综合与 JSON 校验（重试机制）
- [ ] Vision Agent 任务编排 + 断点续跑
- [ ] Filter Agent（英文判定阈值化）

验证：

- 用 2~3 条真实本地视频跑通全管线
- 单条分析失败不阻塞队列；重启后任务恢复
- 输出 JSON 字段与设计一致

## 第三阶段：GUI + 视频浏览

- [ ] Tauri commands 全量暴露（assets / collection / analysis / drafts / tasks / settings）
- [ ] 三栏布局：分类树 / 瀑布流 / 详情
- [ ] 视频播放、多选、收藏、删除、状态徽章
- [ ] 任务队列面板 + 人工审批对话框
- [ ] 采集进度可视化（目标 100 条，TikTok/IG 各自进度）

验证：

- 使用第二阶段入库的样本数据浏览操作
- 播放本地视频正常；筛选/排序正确
- `pnpm build` + `cargo tauri build` 无错误

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
