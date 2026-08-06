# 自定义筛选条件（命中条件 / 排除条件）设计规格

- 日期：2026-08-06
- 状态：已获用户设计确认，待实现
- 关联代码：`src-tauri/src/agents/filter.rs`、`src-tauri/src/analysis/llm.rs`、`src-tauri/src/analysis/pipeline.rs`、`src-tauri/src/settings.rs`、`src-tauri/src/state.rs`、`src-tauri/src/domain/analysis.rs`、`src-tauri/src/db/repos/analyses.rs`、`src-tauri/src/db/migrations.rs`、`frontend/src/api/types.ts`、`frontend/src/components/settings/SettingsDrawer.tsx`

## 背景与目标

当前筛选有两类写死的英文规则（大量英文字幕直接淘汰、英文主导对白按词占比阈值淘汰）和质量分规则。**用户决定：移除两条内置英文规则，不再由代码自动过滤英文相关内容；英文相关内容以后由用户自己配置排除条件来筛。质量分规则保留。** 同时增加**可配置的自定义筛选条件**：

- **命中条件**：题材/内容要求列表，任一命中即通过（OR）；列表为空 = 不限制。
- **排除条件**：违禁/不想要内容的列表，任一命中即淘汰；列表为空 = 不限制。

条件由用户在设置界面手工维护，可以是任意自然语言描述（例如「宠物」「长中文对话」「色情」「性暗示」），判定依据可以是画面或音频，由本地 LLM 在分析时逐条判断。

由此，英文过滤从「内置规则」转为「用户可选配置」：用户若希望排除「大量英文字幕」「长英文对话」，把它们加入排除条件即可；不配置则不再有任何英文相关的自动过滤。

## 术语

- **命中条件**（原「正向条件」）：视频内容符合任一条件即视为满足题材要求。
- **排除条件**（原「反向条件」）：视频内容符合任一条件即淘汰。
- **条件判定**：LLM 对单条条件的输出，包含 `name`、`hit`、`basis`（判定依据）。

## 非目标（本版本不做）

- 不对已有分析结果做存量重判；配置条件后仅对之后的分析生效。
- 不做条件组合逻辑（组内 OR、组间 AND 等复杂表达式）。
- 不做采集任务级临时条件（条件只存在全局设置中）。
- 不在资产详情页展示逐条判定结果（数据会入库，UI 展示留待后续）。
- 不移除 LLM 输出与库中的 `has_foreign_language` / `has_large_amount_english_subtitle` 元数据字段（保留数据与列，仅不再参与过滤）。

## 数据模型

### AppSettings（settings.rs）

**移除** `english_ratio_threshold` 字段（它只服务于被删除的英文对白规则；serde 默认忽略未知字段，旧保存的 JSON 里残留该字段不会导致反序列化失败）。

**新增**两个字段，默认空数组，空数组语义 = 不限制：

```rust
#[serde(default)] pub hit_conditions: Vec<String>,
#[serde(default)] pub exclude_conditions: Vec<String>,
```

**必须使用 `#[serde(default)]`**：`app_settings` 表整行是完整 JSON，旧数据缺少这两个字段时，若无 default 会导致整个设置反序列化失败、整体回退默认值（既有已知坑）。前端 `AppSettings` 类型同步增加 `hit_conditions: string[]` 与 `exclude_conditions: string[]`，并移除 `english_ratio_threshold`（设置抽屉中对应的「英文对白阈值」输入框一并删除）。

**另新增** `condition_basis: HashMap<String, ConditionBasis>`（条件名 -> 画面/音频/不限，`#[serde(default)]`）。判定依据由本地 LLM 在分析前**惰性自动分类**并持久化：某条件依据未知时，Vision Agent 先调用一次 `classify_condition_bases` 分类并写入设置，供两阶段分析使用；`update_settings` 保存时保留库中已有分类结果，避免前端陈旧副本清空。旧设置无此字段时默认空（等价于全部「不限」）。

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
- `has_foreign_language` / `has_large_amount_english_subtitle` 两个输出字段保留（作元数据），但过滤层不再消费它们。
- 具体条件清单由代码动态拼入 user 消息末尾（`analysis/pipeline.rs` 或 `analysis/llm.rs`），格式示例：

  ```
  筛选要求（按编号逐条判定；conditions 中每条 index 必须与编号一致）：
  1. 长中文对话（排除）
  2. 长英文对话（排除）
  ...（仅列出本阶段需要判定的条件，编号为完整清单中的绝对编号）
  ```

- **两阶段判定（2026-08-06 增补）**：
  - 完整编号顺序 = 音频类排除条件 -> 其余排除条件 -> 命中条件（绝对编号）。
  - 阶段1（便宜）：抽音频 + Whisper 转写后，用 `judge_conditions` 仅依据标题与转写判定「音频」类排除条件；任一命中即早退返回部分分析（category/quality 为空、未抽帧未走视觉），由 Vision Agent 置为淘汰并记录 reason。
  - 阶段2（贵）：阶段1 全部未命中（或无音频类条件）才抽帧 + 逐帧视觉 + LLM 综合，判定其余排除条件与命中条件，并把阶段1 的未命中判定并入最终 `condition_results`。
  - 音频提取/转写失败或转写为空时跳过阶段1，安全回退阶段2（未判定的排除条件不构成排除）。
- 条件为空时不拼该段，请求与现在完全一致。

### 输出结构

`AnalysisJson` 增加 `conditions` 字段（JSON 数组，按编号判定）：

```json
{
  "category": "...",
  "...其余原有字段...",
  "conditions": [
    { "index": 1, "hit": true, "basis": "画面" },
    { "index": 5, "hit": false, "basis": "音频" }
  ]
}
```

- `parse_analysis_json` 对 `conditions` 缺失、非数组、元素字段缺失时：容错（跳过非法元素），最终允许为空列表；保证旧模型输出仍可解析。
- **编号映射**：`resolve_conditions` 把模型输出的 `index` 按「先命中条件后排除条件」的顺序映射回设置中的条件原文，生成 `ConditionVerdict { name, hit, basis }` 后入库；编号越界、重复编号、空条件名一律跳过（重复取第一条）。过滤层仍按 `name` 精确匹配，但 name 来自设置列表本身，不再依赖模型回显中文。

## 过滤规则与优先级

`FilterRules` **移除** `english_ratio_threshold` 字段与 `english_word_ratio` 函数（后者仅服务于被删除的英文规则），**增加**：

```rust
pub hit_conditions: Vec<String>,
pub exclude_conditions: Vec<String>,
```

`FilterAgent::evaluate` 判定顺序（`state.rs` 从设置注入两个列表）：

1. **排除条件层**：`exclude_conditions` 非空 且 任一条件在 `condition_results` 中 `hit=true` → `SkipDownload`（淘汰）。
2. **命中条件层**：`hit_conditions` 非空 且 全部条件均已判定 且 全部 `hit=false` → `SkipDownload`（淘汰）。
3. **质量分规则**（保留）：`quality_score < quality_min_keep` → `LowPriority`。
4. 冲突时排除优先（例如既是「宠物」又命中「色情」→ 淘汰）。
5. 两个列表均为空，或旧分析结果没有 `condition_results` 数据 → 跳过第 1、2 层。
6. **未判定的命中条件不参与判定**：若某条已配置的命中条件未出现在 `condition_results`（条件在分析之后才新增、或 LLM 漏判），命中条件层整体跳过，避免基于未判定条件误杀；排除条件层只对已判定条目生效，未判定的排除条件不构成排除。

**删除原有两条英文规则**：不再检查 `has_large_amount_english_subtitle`、不再检查 `has_foreign_language` + 词占比。原 `filter_skips_large_english_subtitles`、`filter_skips_english_dominated_speech`、`english_word_ratio_counts_latin_words` 测试删除。

判定结果映射沿用现有 `verdict_status`：`SkipDownload` → `Eliminated`。

为便于排查，`FilterAgent::evaluate` 返回结构化 `FilterDecision { verdict, reason }`（不再只返回 `FilterVerdict`）；`VisionAgent` 成功输出把 `reason` 写入任务日志（如 `excluded_by_condition: 色情` 或 `no_hit_condition`），不改 UI。

## 前端 UI

`SettingsDrawer.tsx` 增加两个分区：「命中条件」「排除条件」。

- 每个分区为若干行，每行一个文本输入框 + 删除按钮。
- 分区底部「添加条件」按钮追加空行。
- 保存时经现有 `update_settings` 全字段提交（自动带上新字段）；「添加」/「删除」仅在点击保存后持久化。
- 空列表展示为「不限制（空列表）」提示文案。
- 输入框 placeholder 示例：命中条件「宠物、生活、风景、手工」；排除条件「长中文对话、色情、性暗示」。

## 错误处理与兼容性

- 设置反序列化：新字段必须有 `#[serde(default)]`，旧设置行缺失字段时其余设置保持不变；删除 `english_ratio_threshold` 对旧 JSON 安全（serde 默认忽略未知字段）。
- LLM 输出 `conditions` 缺失/非法：回退空列表，过滤跳过条件层，不影响分析落库。
- 条件列表含空字符串：保存时前端过滤空行；后端 `evaluate` 忽略空字符串条件。
- 旧分析结果（无新列数据）：`condition_results` 为空，条件层不生效。
- 行为变化说明：删除英文规则后，含英文的视频不再被自动淘汰（除非用户配置了对应排除条件）；已有分析数据中的 `has_foreign_language` / `has_large_amount_english_subtitle` 保留，状态不回溯变更。
- 迁移失败：`Database::open` 现有错误处理已覆盖，不新增特殊路径。

## 测试计划

### Rust 单元测试

- `parse_analysis_json`：
  - 带 `conditions` 的正常解析；
  - 无 `conditions`（旧输出）解析成功且为空列表；
  - `conditions` 为非法 JSON 类型（如字符串）时容错为空列表。
- `FilterAgent::evaluate` 规则矩阵：
- 无任何条件列表 → 仅质量分规则生效（含英文字幕/英文对白的样例应返回 Keep 而非 SkipDownload）；
- 命中条件任一命中 → Keep；
- 命中条件全部未命中 → SkipDownload；
- 排除条件任一命中 → SkipDownload；
- 命中与排除冲突（同一条视频两个都中）→ 排除优先；
- 空字符串条件被忽略；
- 旧分析（无判定数据）不受新条件影响（条件层跳过）；
- 已配置命中条件中有未判定条目（分析早于条件配置）→ 命中条件层跳过、不误杀；
- 已配置排除条件全部未判定 → 不构成排除。
- 设置测试：`settings_test.rs` 移除 `english_ratio_threshold` 断言，roundtrip 增加命中/排除条件；旧 JSON（含已删除字段、缺新字段）仍可加载。

### 前端测试

- `format`/helpers 现有测试保持通过；如提取条件列表编辑辅助函数（如去空行、trim），补对应测试。

### 集成/回归

- `cargo test` 全绿、`cargo clippy --all-targets` 零警告、`pnpm test` 全过、`pnpm build` 成功。
- 手工冒烟（可选）：设置页添加命中/排除条件 → 保存 → 重分析一条资产 → 确认状态按规则变化、任务日志含淘汰原因。

## 实施步骤

1. **领域与数据层**：`domain/analysis.rs` 加 `ConditionVerdict` 与字段；migration 加列 + `Database::open` 幂等补列；`db/repos/analyses.rs` 读写新列；所有 `NewAnalysis` 构造点补 `condition_results`；`db_repos_test.rs` 增加 condition_results 往返测试 → 验证：`cargo test`。
2. **设置模型**：`settings.rs` 删 `english_ratio_threshold`、加两个 `#[serde(default)]` 字段；`settings_test.rs` 同步 → 验证：`cargo test`。
3. **分析管线与 LLM 协议**：`analysis/llm.rs` 的 `AnalysisJson` 加 `conditions`、`parse_analysis_json` 容错解析、`OllamaLlm` 动态拼条件段；`pipeline.rs` 的 `analyze` 增加条件参数并填充 `condition_results`；更新 prompt；更新所有测试桩 → 验证：`cargo test`。
4. **过滤规则与装配**：`filter.rs` 删除两条英文规则与 `english_word_ratio`、`FilterRules` 换新字段、`evaluate` 返回 `FilterDecision`；`vision.rs` 输出 reason；`state.rs` 从设置注入条件列表 → 验证：`cargo test`、`cargo clippy --all-targets`。
5. **前端**：`types.ts` 同步字段；`SettingsDrawer.tsx` 删英文阈值输入、加命中/排除条件列表编辑区 → 验证：`pnpm test`、`pnpm build`。
6. **全量回归**：`cargo test`、`cargo clippy --all-targets`、`pnpm test`、`pnpm build` → 验证：全绿。
7. **收尾**：更新 `docs/02-architecture.md`（筛选规则描述）与相关文档，提交。

## 验证标准

- 所有自动化测试通过，clippy 零警告。
- 空条件列表时行为 = 仅质量分规则；含大段英文字幕/英文主导对白的样例不再被自动淘汰（有对应测试证明）。
- 配置「命中：宠物、生活、风景、手工；排除：长中文对话、色情、性暗示」后，重分析能按规则产出候选/淘汰，且任务日志能指出淘汰条件。
