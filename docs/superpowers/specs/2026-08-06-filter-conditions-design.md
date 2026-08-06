# 自定义筛选条件（命中条件 / 排除条件）设计规格

- 日期：2026-08-06
- 状态：已获用户设计确认，待实现
- 关联代码：`src-tauri/src/agents/filter.rs`、`src-tauri/src/analysis/llm.rs`、`src-tauri/src/analysis/pipeline.rs`、`src-tauri/src/settings.rs`、`src-tauri/src/state.rs`、`src-tauri/src/domain/analysis.rs`、`src-tauri/src/db/repos/analyses.rs`、`src-tauri/src/db/migrations.rs`、`frontend/src/api/types.ts`、`frontend/src/components/settings/SettingsDrawer.tsx`

## 背景与目标

当前筛选只有三类写死规则：大量英文字幕直接淘汰、英文主导对白（词占比阈值）淘汰、质量分低于阈值淘汰。用户希望在 AI 分析阶段增加**可配置的自定义筛选条件**：

- **命中条件**：题材/内容要求列表，任一命中即通过（OR）；列表为空 = 不限制。
- **排除条件**：违禁/不想要内容的列表，任一命中即淘汰；列表为空 = 不限制。

条件由用户在设置界面手工维护，可以是任意自然语言描述（例如「宠物」「长中文对话」「色情」「性暗示」），判定依据可以是画面或音频，由本地 LLM 在分析时逐条判断。

## 术语

- **命中条件**（原「正向条件」）：视频内容符合任一条件即视为满足题材要求。
- **排除条件**（原「反向条件」）：视频内容符合任一条件即淘汰。
- **条件判定**：LLM 对单条条件的输出，包含 `name`、`hit`、`basis`（判定依据）。

## 非目标（本版本不做）

- 不对已有分析结果做存量重判；配置条件后仅对之后的分析生效。
- 不做条件组合逻辑（组内 OR、组间 AND 等复杂表达式）。
- 不做采集任务级临时条件（条件只存在全局设置中）。
- 不在资产详情页展示逐条判定结果（数据会入库，UI 展示留待后续）。

## 数据模型

### AppSettings（settings.rs）

新增两个字段，默认空数组，空数组语义 = 不限制：

```rust
#[serde(default)] pub hit_conditions: Vec<String>,
#[serde(default)] pub exclude_conditions: Vec<String>,
```

**必须使用 `#[serde(default)]`**：`app_settings` 表整行是完整 JSON，旧数据缺少这两个字段时，若无 default 会导致整个设置反序列化失败、整体回退默认值（既有已知坑）。前端 `AppSettings` 类型同步增加 `hit_conditions: string[]` 与 `exclude_conditions: string[]`。

### analysis_results 表（migrations.rs）

新增一列，存储 LLM 对该资产的逐条条件判定：

```sql
ALTER TABLE analysis_results ADD COLUMN condition_results_json TEXT NOT NULL DEFAULT '[]';
```

迁移说明：`Database::open`（db/mod.rs）每次启动都会 `execute_batch(MIGRATIONS)`，其中全部是 `CREATE TABLE IF NOT EXISTS`，天然幂等。`ALTER TABLE ... ADD COLUMN` 不能直接塞进该批处理（重复执行会报 duplicate column）。采用以下幂等方式：

- `MIGRATIONS` 的 `CREATE TABLE analysis_results` 定义中直接包含 `condition_results_json TEXT NOT NULL DEFAULT '[]'`（覆盖全新库）；
- `Database::open` 在 `execute_batch` 之后增加一步 `ensure_analysis_conditions_column(&conn)`：用 `PRAGMA table_info(analysis_results)` 检查列是否存在，不存在则执行 `ALTER TABLE analysis_results ADD COLUMN condition_results_json TEXT NOT NULL DEFAULT '[]'`（覆盖旧库，且每次启动安全）。

### 领域结构（domain/analysis.rs）

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConditionVerdict {
    pub name: String,   // 条件原文，与设置中的字符串一致
    pub hit: bool,      // 是否命中
    pub basis: String,  // 判定依据：画面 | 音频 | 画面+音频
}
```

`AnalysisResult` 与 `NewAnalysis` 各增加 `condition_results: Vec<ConditionVerdict>`。`AnalysesRepo::row_to_analysis` 从新列解析 JSON（缺失/非法回退 `[]`），`upsert` 写入序列化 JSON。

## LLM 判定协议

### Prompt

- 静态 prompt `resources/prompts/vision-synthesize.md` 保持现有通用说明，增加一段「筛选条件判定」说明：如果请求中提供了筛选条件，必须逐条判定并输出；依据来源包括关键帧（画面）与音频转写（音频）；无条件的请求不出现该段。
- 具体条件清单由代码动态拼入 user 消息末尾（`analysis/pipeline.rs` 或 `analysis/llm.rs`），格式示例：

  ```
  筛选要求（请逐条判定）：
  命中条件（至少命中其一）：宠物、生活、风景、手工
  排除条件（任一命中即排除）：长中文对话、长英文对话、大量英文字幕、色情、性暗示
  ```

- 条件为空时不拼该段，请求与现在完全一致。

### 输出结构

`AnalysisJson` 增加 `conditions` 字段（JSON 数组）：

```json
{
  "category": "...",
  "...其余原有字段...",
  "conditions": [
    { "name": "宠物", "hit": true, "basis": "画面" },
    { "name": "长中文对话", "hit": false, "basis": "音频" }
  ]
}
```

- `parse_analysis_json` 对 `conditions` 缺失、非数组、元素字段缺失时：容错（跳过非法元素），最终允许为空列表；保证旧模型输出仍可解析。
- LLM 返回的 `name` 可能不完全等于设置中的字符串（如多空格、错别字）；过滤层按**精确匹配设置列表中的原文**判断，LLM 输出中无法匹配的条目忽略。

## 过滤规则与优先级

`FilterRules` 增加：

```rust
pub hit_conditions: Vec<String>,
pub exclude_conditions: Vec<String>,
```

`FilterAgent::evaluate` 判定顺序（`state.rs` 从设置注入两个列表）：

1. **排除条件层**：`exclude_conditions` 非空 且 任一条件在 `condition_results` 中 `hit=true` → `SkipDownload`（淘汰）。
2. **命中条件层**：`hit_conditions` 非空 且 全部条件 `hit=false` → `SkipDownload`（淘汰）。
3. **原有规则层**：大段英文字幕、英文主导对白、质量分低于阈值，行为不变。
4. 冲突时排除优先（例如既是「宠物」又命中「色情」→ 淘汰）。
5. 两个列表均为空，或旧分析结果没有 `condition_results` 数据 → 跳过第 1、2 层，行为与现状完全一致。

判定结果映射沿用现有 `verdict_status`：`SkipDownload` → `Eliminated`。

为便于排查，`VisionAgent` 成功输出增加 `reason` 字段（可选字符串），记录导致淘汰的条件名（如 `excluded_by_condition: 色情` 或 `no_hit_condition`），仅写入任务日志，不改 UI。

## 前端 UI

`SettingsDrawer.tsx` 增加两个分区：「命中条件」「排除条件」。

- 每个分区为若干行，每行一个文本输入框 + 删除按钮。
- 分区底部「添加条件」按钮追加空行。
- 保存时经现有 `update_settings` 全字段提交（自动带上新字段）；「添加」/「删除」仅在点击保存后持久化。
- 空列表展示为「不限制（空列表）」提示文案。
- 输入框 placeholder 示例：命中条件「宠物、生活、风景、手工」；排除条件「长中文对话、色情、性暗示」。

## 错误处理与兼容性

- 设置反序列化：新字段必须有 `#[serde(default)]`，旧设置行缺失字段时其余设置保持不变。
- LLM 输出 `conditions` 缺失/非法：回退空列表，过滤跳过条件层，不影响分析落库。
- 条件列表含空字符串：保存时前端过滤空行；后端 `evaluate` 忽略空字符串条件。
- 旧分析结果（无新列数据）：`condition_results` 为空，条件层不生效。
- 迁移失败：`Database::open` 现有错误处理已覆盖，不新增特殊路径。

## 测试计划

### Rust 单元测试

- `parse_analysis_json`：
  - 带 `conditions` 的正常解析；
  - 无 `conditions`（旧输出）解析成功且为空列表；
  - `conditions` 为非法 JSON 类型（如字符串）时容错为空列表。
- `FilterAgent::evaluate` 规则矩阵：
  - 无任何条件列表 → 行为与旧规则一致；
  - 命中条件任一命中 → Keep；
  - 命中条件全部未命中 → SkipDownload；
  - 排除条件任一命中 → SkipDownload；
  - 命中与排除冲突（同一条视频两个都中）→ 排除优先；
  - 空字符串条件被忽略；
  - 旧分析（无判定数据）不受新条件影响。

### 前端测试

- `format`/helpers 现有测试保持通过；如提取条件列表编辑辅助函数（如去空行、trim），补对应测试。

### 集成/回归

- `cargo test` 全绿、`cargo clippy --all-targets` 零警告、`pnpm test` 全过、`pnpm build` 成功。
- 手工冒烟（可选）：设置页添加命中/排除条件 → 保存 → 重分析一条资产 → 确认状态按规则变化、任务日志含淘汰原因。

## 实施步骤

1. **先写失败测试**：`filter` 规则矩阵 + `parse_analysis_json` 兼容解析 → 验证：新测试红（旧代码不通过）。
2. **数据层**：migration 加列；`domain/analysis.rs`、`db/repos/analyses.rs` 扩展结构 → 验证：`cargo test`。
3. **分析管线**：`analysis/llm.rs` 解析 conditions；`pipeline.rs` 动态拼条件段 → 验证：`cargo test`。
4. **过滤与设置**：`filter.rs` 新规则；`settings.rs` 新字段；`state.rs` 注入；`VisionAgent` 输出 reason → 验证：`cargo test`。
5. **前端**：`types.ts` 加字段；`SettingsDrawer.tsx` 条件列表编辑区 → 验证：`pnpm test`、`pnpm build`。
6. **全量回归**：`cargo test`、`cargo clippy --all-targets`、`pnpm test`、`pnpm build` → 验证：全绿。
7. **收尾**：更新 `docs/02-architecture.md`（如架构图涉及筛选）与相关文档，提交。

## 验证标准

- 所有自动化测试通过，clippy 零警告。
- 空条件列表时行为与现状逐条等价（旧测试全部保持绿色即证明）。
- 配置「命中：宠物、生活、风景、手工；排除：长中文对话、色情、性暗示」后，重分析能按规则产出候选/淘汰，且任务日志能指出淘汰条件。
