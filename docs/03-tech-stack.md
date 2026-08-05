# 技术选型

## 1. 桌面端与前端

| 组件 | 选型 | 理由 |
| --- | --- | --- |
| 桌面框架 | **Tauri 2 + Rust** | 安装包小、内存占用低、Rust 后端可安全处理本地文件/数据库/子进程；满足用户指定 |
| 前端框架 | **React 18 + TypeScript** | 生态成熟，类型安全，适合三栏数据密集型 UI |
| 样式 | **TailwindCSS** | 快速构建瀑布流等自定义布局，避免组件库的模板感 |
| 构建 | **Vite** | Tauri 官方推荐，HMR 快 |
| 状态管理 | **Zustand** | 轻量，适合资产列表/筛选/选择集状态 |
| 视频播放 | HTML5 `<video>` + Tauri `asset:` 协议 | 本地文件直接播放，无需转码 |

## 2. 后端与存储

| 组件 | 选型 | 理由 |
| --- | --- | --- |
| Rust 异步运行时 | **tokio** | Tauri 生态标准 |
| 数据库 | **SQLite + rusqlite** | 单文件、零运维、支持 WAL 与迁移；完全满足本地单机场景 |
| 迁移 | **rusqlite_migration**（或自写 migrate） | 版本化建表，避免 schema 漂移 |
| 序列化 | **serde / serde_json** | 标准 |
| 日志 | **tracing + tracing-appender** | 结构化日志，任务与文件双写 |

## 3. AI 服务

| 能力 | 选型 | 说明 |
| --- | --- | --- |
| LLM（综合判断/文案） | **Ollama**（如 `qwen2.5:7b`） | 本地 HTTP API，`/api/chat` + JSON 输出约束 |
| Vision（抽帧理解/OCR 估算） | **Ollama**（如 `qwen2.5-vl:7b`、`minicpm-v`） | 本地多模态，直接分析关键帧 |
| ASR（音频转写/语言识别） | **faster-whisper**（small/base） | 速度快、显存友好；若用户环境合适也可用 whisper.cpp |
| 模型管理 | Ollama 模型清单 + 配置文件 | `resources/models.toml` 集中声明，GUI 显示可用状态 |

> 备选：llama.cpp server 提供 OpenAI 兼容 API。Rust 侧统一封装 `AiClient`，底层实现（Ollama / llama.cpp）可切换，切换只影响配置。

## 4. 视频处理

| 能力 | 选型 | 说明 |
| --- | --- | --- |
| 抽帧 / 音频提取 | **FFmpeg**（子进程） | 按时长均匀抽 3~6 帧；提取 16kHz mono WAV |
| 视频下载 | **yt-dlp**（子进程） | 成熟支持 TikTok/IG 链接解析与 CDN 下载；抽象为 `Downloader` trait，可替换为自解析直链 |
| 封面生成 | FFmpeg 首帧 + 可选居中裁剪 | GUI 缩略图 |

## 5. 浏览器自动化

| 组件 | 选型 | 说明 |
| --- | --- | --- |
| 采集与发布 | **Playwright**（Node sidecar） | Rust 通过 JSON-RPC 调用；支持持久化 Profile、网络响应拦截、独立窗口 |
| 独立窗口 | Playwright `launchPersistentContext(userDataDir)` | 满足"不碰用户浏览器"的硬性要求 |

> 备选：Playwright 官方 Rust crate 仍不成熟，Node sidecar 是当前最稳的集成方式。sidecar 由 Tauri 管理生命周期，未安装时自动提示安装。

## 6. 环境依赖清单

| 依赖 | 用途 | 安装方式 |
| --- | --- | --- |
| Rust stable | 后端编译 | rustup |
| Node.js 18+ / pnpm | 前端 + Playwright sidecar | 官方安装包 |
| FFmpeg | 抽帧/音频/封面 | winget/choco |
| yt-dlp | 视频下载 | pip / winget |
| Ollama | 本地 LLM/Vision | 官方安装包 |
| faster-whisper | ASR | Python venv（sidecar） |

## 7. 关键取舍记录（ADR 摘要）

| 决策 | 选择 | 原因 | 代价 |
| --- | --- | --- | --- |
| 元数据采集方式 | 网络响应拦截优先，DOM 兜底 | 响应 JSON 字段稳定，DOM 易变 | 需要维护拦截 URL 规则 |
| 推荐算法 | 确定性加权 + LLM 风格加成 | 可复现、可解释 | 需要调权重 |
| 发布自动化范围 | 填表即停，不点发布 | 安全红线 | 用户需多一步手工点击 |
| 下载器 | yt-dlp 优先 | 平台反爬迭代快，yt-dlp 社区同步 | 依赖外部工具版本 |
