# build_douyin_acount

PC 端 AI 短视频运营助手 —— 从 TikTok / Instagram Reels 采集优质短视频，使用本地 AI 完成理解、分类、筛选与文案生成，辅助运营者发布到抖音。

> 这是一个**人机协作工具**，不是无人值守机器人。涉及账号登录、最终发布等高风险操作均保留人工确认环节。

## 当前进度

- [x] 第一阶段：需求分析与系统架构设计（2026-08-05）
- [x] 第二阶段：数据库 + 视频管理 + AI 分析 Pipeline（2026-08-05，41 个测试全绿）
- [ ] 第三阶段：桌面 GUI + 视频浏览
- [ ] 第四阶段：抖音发布辅助

## 文档索引

| 文档 | 内容 |
| --- | --- |
| [需求分析](docs/01-requirements-analysis.md) | 功能需求、补充约束、非功能需求、风险 |
| [系统架构](docs/02-architecture.md) | 分层架构、Agent 设计、数据模型、核心流程 |
| [技术选型](docs/03-tech-stack.md) | 技术栈与选型理由 |
| [目录结构](docs/04-directory-structure.md) | 仓库结构与模块职责 |
| [阶段计划](docs/05-milestones.md) | 分阶段实施计划与验收标准 |

## 技术栈概览

- 桌面端：Tauri 2 + Rust
- 前端：React 18 + TypeScript + TailwindCSS + Vite
- 数据库：SQLite
- 本地 AI：Ollama（Vision / LLM）+ Whisper（ASR）
- 视频处理：FFmpeg
- 浏览器自动化：Playwright（独立窗口，独立登录态）
- 下载：yt-dlp

## 快速开始（开发环境）

前置依赖：

- Rust stable + Node.js 18+ + pnpm
- [Ollama](https://ollama.com)（本地 LLM / Vision 模型）
- FFmpeg
- yt-dlp

```powershell
# 1. 启动本地 AI 服务
ollama serve
ollama pull qwen2.5-vl:7b

# 2. 安装依赖并启动前端
cd frontend
pnpm install
pnpm dev

# 3. 启动 Tauri 开发模式（另开终端）
cd src-tauri
cargo tauri dev
```

## 安全原则

1. 采集与发布使用**独立的浏览器窗口与独立的浏览器 Profile**，不触碰用户正在使用的浏览器。
2. 发布流程自动化到"表单填写完成"即停止，**最终发布按钮只由用户本人点击**。
3. 账号 Cookie / 登录态仅保存在本地独立 Profile 中。
4. 点赞数超过 100 万的视频不采集。
