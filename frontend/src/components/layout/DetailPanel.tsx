import { useAssetsStore } from "../../stores/assetsStore";
import { useSelectionStore } from "../../stores/selectionStore";
import { useTaskStore } from "../../stores/taskStore";
import { videoUrl } from "../../api/urls";
import { formatCount, formatDuration, platformLabel } from "../../lib/format";
import { ScoreBadge } from "../ScoreBadge";
import { StatusBadge } from "../StatusBadge";
import { useEffect, useState } from "react";
import { api } from "../../api/client";
import type { AnalysisResult } from "../../api/types";

export function DetailPanel() {
  const focusId = useSelectionStore((s) => s.focusId);
  const assets = useAssetsStore((s) => s.assets);
  const toggleFavorite = useAssetsStore((s) => s.toggleFavorite);
  const removeAsset = useAssetsStore((s) => s.removeAsset);
  const updateStatus = useAssetsStore((s) => s.updateStatus);
  const analyzeSelected = useAssetsStore((s) => s.analyzeSelected);
  const setTaskOpen = useTaskStore((s) => s.setOpen);
  const [analysis, setAnalysis] = useState<AnalysisResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [publishing, setPublishing] = useState(false);

  const asset = assets.find((a) => a.id === focusId) ?? null;

  useEffect(() => {
    if (!asset) {
      setAnalysis(null);
      return;
    }
    setAnalysis(null);
    void api.getAnalysis(asset.id).then(setAnalysis).catch(() => setAnalysis(null));
  }, [asset?.id, asset]);

  if (!asset) {
    return (
      <aside className="flex w-[380px] shrink-0 flex-col items-center justify-center border-l border-line bg-panel/60 text-sm text-mute">
        点击左侧素材查看详情
      </aside>
    );
  }

  const nextStatus =
    asset.status === "READY"
      ? "SELECTED"
      : asset.status === "SELECTED"
        ? "PUBLISHED"
        : asset.status === "PUBLISHED"
          ? "READY"
          : asset.status === "LOW_PRIORITY"
            ? "READY"
            : null;

  const statusActionLabel =
    asset.status === "READY"
      ? "加入待发布"
      : asset.status === "SELECTED"
        ? "标记已发布"
        : asset.status === "PUBLISHED"
          ? "撤销发布"
          : asset.status === "LOW_PRIORITY"
            ? "恢复可发布"
            : null;

  const handleDelete = async () => {
    if (confirm(`确定删除视频 #${asset.id}？本地文件与封面将一并删除。`)) {
      await removeAsset(asset.id);
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
      const res = await api.preparePublish(asset.id);
      setTaskOpen(true);
      // 任务入队后稍等后台 worker 拉起 sidecar，再刷新任务面板。
      setTimeout(() => void useTaskStore.getState().refresh(), 1200);
      return res;
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
      return null;
    } finally {
      setPublishing(false);
    }
  };

  return (
    <aside className="flex w-[380px] shrink-0 flex-col overflow-y-auto border-l border-line bg-panel/60">
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
        </div>
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
            <div className="rounded border border-accent/25 bg-accent/5 p-3">
              <div className="text-[10px] font-semibold uppercase tracking-wider text-accent/80">
                推荐文案
              </div>
              <p className="mt-1 text-[13px] leading-relaxed text-paper/85">
                {analysis.recommended_caption || "—"}
              </p>
            </div>
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
                ⚠ 判定为英文内容（对白占比高 / 大量英文字幕），已按规则标记低优先级或跳过。
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

      <div className="mt-auto flex flex-wrap gap-2 border-t border-line p-4">
        <button
          onClick={() => void toggleFavorite(asset.id)}
          className={`h-9 rounded border px-3 text-sm transition-colors ${
            asset.is_favorite
              ? "border-accent bg-accent/15 text-accent"
              : "border-line text-mute hover:border-accent/40 hover:text-paper"
          }`}
        >
          {asset.is_favorite ? "取消收藏" : "收藏"}
        </button>
        <button
          onClick={() => void handleAnalyze()}
          disabled={busy}
          className="h-9 rounded border border-line px-3 text-sm text-paper hover:border-accent/50 hover:bg-accent/10 disabled:opacity-40"
        >
          {busy ? "排队中…" : "AI 分析"}
        </button>
        {statusActionLabel && nextStatus && (
          <button
            onClick={() => void updateStatus(asset.id, nextStatus)}
            className="h-9 rounded border border-accent bg-accent/10 px-3 text-sm text-accent hover:bg-accent/20"
          >
            {statusActionLabel}
          </button>
        )}
        {asset.status === "SELECTED" && (
          <button
            onClick={() => void handlePreparePublish()}
            disabled={publishing}
            className="h-9 rounded border border-accent bg-accent px-3 text-sm font-medium text-ink hover:brightness-110 disabled:opacity-40"
            title="打开独立浏览器窗口，自动上传视频并填写标题/文案/话题（最终发布由你本人点击）"
          >
            {publishing ? "准备中…" : "准备发布"}
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
