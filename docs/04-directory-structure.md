# 目录结构

```text
build_douyin_acount/
├── README.md
├── docs/
│   ├── 01-requirements-analysis.md
│   ├── 02-architecture.md
│   ├── 03-tech-stack.md
│   ├── 04-directory-structure.md        ← 本文件
│   └── 05-milestones.md
│
├── frontend/                              # React 18 + TS + Tailwind
│   ├── package.json
│   ├── vite.config.ts
│   ├── tailwind.config.ts
│   ├── index.html
│   └── src/
│       ├── main.tsx
│       ├── App.tsx
│       ├── api/                           # Tauri invoke / event 封装（类型安全）
│       │   ├── client.ts
│       │   ├── assets.ts
│       │   ├── tasks.ts
│       │   └── publish.ts
│       ├── stores/                        # Zustand
│       │   ├── assetsStore.ts
│       │   ├── filterStore.ts
│       │   ├── selectionStore.ts
│       │   └── taskStore.ts
│       ├── components/
│       │   ├── layout/
│       │   │   ├── Sidebar.tsx            # 分类树
│       │   │   ├── Waterfall.tsx          # 瀑布流
│       │   │   ├── DetailPanel.tsx        # 详情
│       │   │   └── TopBar.tsx             # 采集入口 / 设置 / 状态
│       │   ├── assets/
│       │   │   ├── VideoCard.tsx
│       │   │   ├── ScoreBadge.tsx
│       │   │   └── StatusBadge.tsx
│       │   ├── tasks/
│       │   │   ├── TaskQueuePanel.tsx
│       │   │   └── ApprovalDialog.tsx     # 人工确认节点
│       │   └── publish/
│       │       ├── DraftEditor.tsx
│       │       └── PublishFlowPanel.tsx
│       └── styles/
│
├── src-tauri/                             # Rust 后端
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── capabilities/
│   └── src/
│       ├── main.rs
│       ├── lib.rs
│       ├── commands/                      # IPC 层
│       │   ├── mod.rs
│       │   ├── assets.rs                  # 列表/详情/收藏/删除/状态流转
│       │   ├── collection.rs              # 启动采集/进度
│       │   ├── analysis.rs                # 单条重分析/批量分析
│       │   ├── drafts.rs                  # 文案生成/保存
│       │   ├── recommendations.rs         # 推荐查询
│       │   ├── publish.rs                 # 准备发布/标记已发布
│       │   └── settings.rs                # 模型/风格/比例配置
│       ├── state/                         # AppState（DB、Registry、Runtime 句柄）
│       ├── domain/                        # 领域模型与状态机
│       │   ├── mod.rs
│       │   ├── asset.rs
│       │   ├── analysis.rs
│       │   ├── draft.rs
│       │   ├── task.rs
│       │   └── error.rs
│       ├── db/
│       │   ├── mod.rs
│       │   ├── migrations.rs
│       │   └── repos/                     # 各表 Repository
│       ├── agents/
│       │   ├── mod.rs
│       │   ├── runtime.rs                 # 队列/状态机/重试/日志/审批
│       │   ├── collector.rs
│       │   ├── vision.rs
│       │   ├── filter.rs
│       │   ├── content.rs
│       │   └── publisher.rs
│       ├── collector/                     # 采集模块（独立、插件化）
│       │   ├── mod.rs                     # PlatformAdapter trait + Registry
│       │   ├── tiktok.rs
│       │   ├── instagram.rs
│       │   └── downloader.rs              # yt-dlp 封装
│       ├── analysis/                      # AI 分析管线
│       │   ├── mod.rs
│       │   ├── frames.rs                  # FFmpeg 抽帧
│       │   ├── audio.rs                   # 音频提取
│       │   ├── asr.rs                     # Whisper 调用
│       │   ├── vision.rs                  # Ollama Vision
│       │   ├── llm.rs                     # Ollama LLM + JSON 约束
│       │   └── pipeline.rs                # 编排：帧→Vision→ASR→LLM
│       ├── publish/
│       │   ├── mod.rs
│       │   ├── bridge.rs                  # Playwright sidecar JSON-RPC 客户端
│       │   └── douyin_page.rs             # creator.douyin.com 页面对象
│       ├── media/                         # 文件存储管理
│       ├── recommend/                     # 推荐排序
│       ├── ai/                            # AiClient 抽象（Ollama/llama.cpp 可切换）
│       └── util/                          # 重试/超时/路径工具
│
├── scripts/
│   ├── playwright-bridge/                 # Node sidecar
│   │   ├── package.json
│   │   ├── index.js                       # JSON-RPC 服务
│   │   └── src/
│   │       ├── window.js                  # 独立窗口（持久化 Profile）
│   │       ├── intercept.js               # 网络响应拦截
│   │       └── douyin.js                  # 抖音页面操作（不含发布点击）
│   ├── whisper-server/                    # faster-whisper Python sidecar
│   └── dev.ps1                            # 一键开发环境启动
│
├── resources/                             # 运行时资源（打包进应用）
│   ├── prompts/
│   │   ├── vision-frame.md                # 单帧分析提示词
│   │   ├── vision-synthesize.md           # 帧+ASR 综合提示词
│   │   ├── filter.md                      # 英文/质量判定提示词
│   │   └── content.md                     # 文案生成（按风格）
│   ├── selectors/
│   │   ├── douyin.json
│   │   ├── tiktok.json
│   │   └── instagram.json
│   ├── filter-rules.toml                  # 过滤阈值
│   └── models.toml                        # 模型清单与默认配置
│
├── storage/                               # 本地数据（gitignored，运行时生成）
│   ├── db/
│   │   └── app.db
│   ├── videos/
│   ├── covers/
│   ├── frames/
│   ├── audio/
│   ├── logs/
│   └── browser-profiles/
│       ├── collector/
│       └── publisher/
│
└── .gitignore
```

## 模块职责边界

| 模块 | 职责 | 不负责 |
| --- | --- | --- |
| `agents/runtime` | 任务生命周期 | 业务算法 |
| `agents/collector` | 采集编排（比例、配额、去重） | 平台抓取细节（由 adapter 负责） |
| `collector/` | 平台抓取与下载 | 分析 |
| `analysis/` | 抽帧、ASR、Vision、LLM 综合 | 采集与存储策略 |
| `recommend/` | 加权排序与推荐理由 | 文案生成 |
| `publish/` | 浏览器桥接与抖音页面操作 | 发布决策（由人） |
| `commands/` | IPC 参数校验与结果返回 | 业务实现 |
