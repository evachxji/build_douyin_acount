import { useAssetsStore } from "../../stores/assetsStore";
import { useSelectionStore } from "../../stores/selectionStore";
import { useTaskStore } from "../../stores/taskStore";
import { videoUrl } from "../../api/urls";
import { formatCount, formatDuration, platformLabel } from "../../lib/format";
import { ScoreBadge } from "../ScoreBadge";
import { StatusBadge } from "../StatusBadge";
import { useEffect, useState } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import { api } from "../../api/client";
import type { AnalysisResult, ContentCopy } from "../../api/types";

export function DetailPanel({ width }: { width: number }) {
  const focusId = useSelectionStore((s) => s.focusId);
  const assets = useAssetsStore((s) => s.assets);
  const removeAsset = useAssetsStore((s) => s.removeAsset);
  const updateStatus = useAssetsStore((s) => s.updateStatus);
  const analyzeSelected = useAssetsStore((s) => s.analyzeSelected);
  const setTaskOpen = useTaskStore((s) => s.setOpen);
  const tasks = useTaskStore((s) => s.tasks);
  const [analysis, setAnalysis] = useState<AnalysisResult | null>(null);
  const [copy, setCopy] = useState<ContentCopy | null>(null);
  const [busy, setBusy] = useState(false);
  const [copying, setCopying] = useState(false);
  const [publishing, setPublishing] = useState(false);

  const asset = assets.find((a) => a.id === focusId) ?? null;

  useEffect(() => {
    if (!asset) {
      setAnalysis(null);
      setCopy(null);
      return;
    }
    setAnalysis(null);
    setCopy(null);
    void api.getAnalysis(asset.id).then(setAnalysis).catch(() => setAnalysis(null));
    void api.getCopy(asset.id).then(setCopy).catch(() => setCopy(null));
  }, [asset?.id, asset]);

  // 准备发布后保持 loading，直到「请选择视频文件」弹窗出现（publisher 任务 stage=file）。
  // 60 秒超时兜底，避免任务异常时按钮一直转。
  useEffect(() => {
    if (!publishing) return;
    const fileStage = tasks.some(
      (t) =>
        t.agent_name === "publisher" &&
        t.payload.stage === "file" &&
        (t.status === "RUNNING" || t.status === "PENDING"),
    );
    if (fileStage) {
      setPublishing(false);
      return;
    }
    const timer = setTimeout(() => setPublishing(false), 60_000);
    return () => clearTimeout(timer);
  }, [publishing, tasks]);

  const handleGenerateCopy = async () => {
    if (!asset) return;
    setCopying(true);
    try {
      await api.generateCopy(asset.id);
      // 文案由后台 LLM 生成，轮询等待结果（最长 60 秒）。
      for (let i = 0; i < 20; i += 1) {
        await new Promise((r) => setTimeout(r, 3000));
        const c = await api.getCopy(asset.id).catch(() => null);
        if (c) {
          setCopy(c);
          break;
        }
      }
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
    } finally {
      setCopying(false);
    }
  };

  if (!asset) {
    return (
      <aside
        style={{ width }}
        className="flex shrink-0 flex-col items-center justify-center border-l border-line bg-panel/60 text-sm text-mute"
      >
        点击左侧素材查看详情
      </aside>
    );
  }

  const nextStatus =
    asset.status === "CANDIDATE"
      ? "SELECTED"
      : asset.status === "SELECTED"
        ? "PUBLISHED"
        : asset.status === "PUBLISHED"
          ? "CANDIDATE"
          : asset.status === "ELIMINATED"
            ? "CANDIDATE"
            : null;

  const statusActionLabel =
    asset.status === "CANDIDATE"
      ? "通过"
      : asset.status === "SELECTED"
        ? "标记已发布"
        : asset.status === "PUBLISHED"
          ? "撤销发布"
          : asset.status === "ELIMINATED"
            ? "恢复候选"
            : null;

  const handleStatusAction = async () => {
    if (!nextStatus) return;
    await updateStatus(asset.id, nextStatus);
    // 用户「通过」候选素材 -> 待发布，并自动生成 AI 文案。
    if (asset.status === "CANDIDATE" && nextStatus === "SELECTED") {
      void api.generateCopy(asset.id).catch(() => {});
    }
  };

  const handleDelete = async () => {
    const ok = await ask(`确定删除视频 #${asset.id}？本地文件与封面将一并删除。`, {
      title: "删除确认",
      kind: "warning",
    });
    if (!ok) return;
    try {
      await removeAsset(asset.id);
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
    }
  };

  const handleReveal = async () => {
    try {
      await api.revealInFolder(asset.file_path);
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
    }
  };

  const handleAnalyze = async () => {
    setBusy(true);
    try {
      await analyzeSelected([asset.id]);
      setTaskOpen(true);
    } finally {
      setBusy(false);
    }
  };

  const handlePreparePublish = async () => {
    setPublishing(true);
    try {
      await api.preparePublish(asset.id);
      setTaskOpen(true);
      // 任务入队后稍等后台 worker 拉起 sidecar，再刷新任务面板。
      setTimeout(() => void useTaskStore.getState().refresh(), 1200);
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
      setPublishing(false);
    }
  };

  return (
    <aside
      style={{ width }}
      className="flex shrink-0 flex-col border-l border-line bg-panel/60"
    >
      <div className="min-h-0 flex-1 overflow-y-auto">
      <div className="p-4">
        <video
          key={asset.file_path}
          src={videoUrl(asset.file_path)}
          controls
          className="aspect-video w-full rounded-lg border border-line bg-black"
        />
      </div>

      <div className="px-4">
        <div className="flex items-start justify-between gap-2">
          <h2 className="text-sm font-semibold leading-snug text-paper">
            {asset.title ?? `视频 #${asset.id}`}
          </h2>
          <StatusBadge status={asset.status} />
        </div>
        <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-mute">
          <span>{platformLabel(asset.source_platform)}</span>
          <span>{asset.author ?? "未知作者"}</span>
          <span>{formatDuration(asset.duration_ms)}</span>
          <span className="tabular-nums">
            ♥ {formatCount(asset.likes)} · {formatCount(asset.comments)} 评 ·{" "}
            {formatCount(asset.plays)} 播
          </span>
          <button
            onClick={() => void handleReveal()}
            className="ml-auto rounded border border-line px-1.5 py-0.5 text-mute transition-colors hover:border-accent/50 hover:text-paper"
            title="在资源管理器中查看视频位置"
            aria-label="在资源管理器中查看视频位置"
          >
            📁
          </button>
        </div>
        {asset.status === "ELIMINATED" && (
          <p className="mt-2 rounded border border-bad/30 bg-bad/10 px-2 py-1 text-xs text-bad">
            淘汰原因：{asset.eliminate_reason ?? "未记录"}
          </p>
        )}
      </div>

      <div className="mt-4 border-t border-line px-4 py-4">
        <div className="flex items-center gap-2">
          <span className="text-[11px] font-semibold uppercase tracking-wider text-mute">
            AI 分析
          </span>
          <ScoreBadge score={asset.quality_score} />
        </div>
        {analysis ? (
          <div className="mt-3 space-y-3 text-sm">
            <div className="flex flex-wrap gap-1.5">
              {[analysis.category, ...analysis.tags].filter(Boolean).map((t) => (
                <span key={t} className="rounded bg-white/5 px-2 py-0.5 text-xs text-paper/85">
                  {t}
                </span>
              ))}
            </div>
            {analysis.summary && (
              <p className="text-[13px] leading-relaxed text-paper/75">{analysis.summary}</p>
            )}
            {/* 已生成风格化 AI 文案时不再重复展示分析推荐文案 */}
            {!copy && (
              <div className="rounded border border-accent/25 bg-accent/5 p-3">
                <div className="text-[10px] font-semibold uppercase tracking-wider text-accent/80">
                  推荐文案
                </div>
                <p className="mt-1 text-[13px] leading-relaxed text-paper/85">
                  {analysis.recommended_caption || "—"}
                </p>
              </div>
            )}
            {analysis.asr_text && (
              <div>
                <div className="text-[10px] font-semibold uppercase tracking-wider text-mute">
                  语音转写（{analysis.asr_language ?? "unknown"}）
                </div>
                <p className="mt-1 max-h-24 overflow-y-auto text-xs leading-relaxed text-mute">
                  {analysis.asr_text}
                </p>
              </div>
            )}
            {(analysis.has_foreign_language || analysis.has_large_amount_english_subtitle) && (
              <p className="text-xs text-bad">
                ⚠ 检测到外语内容（对白语言：
                {analysis.has_foreign_language ? (analysis.asr_language || "外语") : "未知"}
                {analysis.has_large_amount_english_subtitle ? " / 大量外文字幕" : ""}
                ）。是否淘汰取决于设置的排除条件与质量分，非外语即淘汰。
              </p>
            )}
          </div>
        ) : (
          <p className="mt-2 text-xs text-mute">
            {asset.status === "ANALYZING"
              ? "正在分析中，完成后自动刷新…"
              : "尚未分析，点击「AI 分析」排队处理。"}
          </p>
        )}
      </div>

      <div className="mt-4 border-t border-line px-4 py-4">
        <div className="flex items-center justify-between">
          <span className="text-[11px] font-semibold uppercase tracking-wider text-mute">
            AI 文案
          </span>
          <button
            onClick={() => void handleGenerateCopy()}
            disabled={copying || !analysis}
            className="rounded border border-line px-2 py-0.5 text-[11px] text-mute hover:border-accent/50 hover:text-paper disabled:opacity-40"
          >
            {copying ? "生成中…" : "生成文案"}
          </button>
        </div>
        {copy ? (
          <div className="mt-2 space-y-2 text-sm">
            <p className="text-[13px] font-medium text-paper/90">{copy.title}</p>
            {copy.body && <p className="text-[13px] leading-relaxed text-paper/75">{copy.body}</p>}
            {copy.hashtags.length > 0 && (
              <p className="text-xs text-accent">
                {copy.hashtags.map((h) => `#${h}`).join(" ")}
              </p>
            )}
          </div>
        ) : (
          <p className="mt-2 text-xs text-mute">
            按设置中的账号风格生成抖音标题/正文/话题。
          </p>
        )}
      </div>

      </div>
      <div className="flex shrink-0 flex-wrap gap-2 border-t border-line p-4">
        {asset.status !== "CANDIDATE" && (
          <button
            onClick={() => void handleAnalyze()}
            disabled={busy}
            className="h-9 rounded border border-line px-3 text-sm text-paper hover:border-accent/50 hover:bg-accent/10 disabled:opacity-40"
          >
            {busy ? "排队中…" : "AI 分析"}
          </button>
        )}
        {statusActionLabel && nextStatus && (
          <button
            onClick={() => void handleStatusAction()}
            className="h-9 rounded border border-accent bg-accent/10 px-3 text-sm text-accent hover:bg-accent/20"
            title={asset.status === "CANDIDATE" ? "标记为待发布并自动生成 AI 文案" : undefined}
          >
            {statusActionLabel}
          </button>
        )}
        {asset.status === "CANDIDATE" && (
          <button
            onClick={() => void updateStatus(asset.id, "ELIMINATED")}
            className="h-9 rounded border border-bad/40 px-3 text-sm text-bad hover:bg-bad/10"
            title="标记为淘汰，不再参与发布"
          >
            淘汰
          </button>
        )}
        {asset.status === "SELECTED" && (
        <button
          onClick={() => void handlePreparePublish()}
          disabled={publishing}
          className="inline-flex h-9 items-center justify-center gap-1.5 rounded border border-accent bg-accent px-3 text-sm font-medium text-ink hover:brightness-110 disabled:cursor-not-allowed disabled:opacity-40"
          title="打开独立浏览器窗口，自动上传视频并填写标题/文案/话题（最终发布由你本人点击）"
        >
          {publishing ? (
            <>
              <span className="h-3.5 w-3.5 animate-spin rounded-full border-2 border-ink/40 border-t-ink" />
              准备中…
            </>
          ) : (
            "准备发布"
          )}
        </button>
        )}
        <button
          onClick={() => void handleDelete()}
          className="ml-auto h-9 rounded border border-bad/40 px-3 text-sm text-bad hover:bg-bad/10"
        >
          删除
        </button>
      </div>
    </aside>
  );
}
