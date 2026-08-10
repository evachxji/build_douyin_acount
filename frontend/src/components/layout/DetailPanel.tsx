import { useAssetsStore } from "../../stores/assetsStore";
import { useSelectionStore } from "../../stores/selectionStore";
import { useTaskStore } from "../../stores/taskStore";
import { videoUrl } from "../../api/urls";
import { formatCount, formatDuration, platformLabel } from "../../lib/format";
import { ScoreBadge } from "../ScoreBadge";
import { StatusBadge } from "../StatusBadge";
import { useEffect, useRef, useState } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import { api } from "../../api/client";
import type { AnalysisResult, ContentCopy } from "../../api/types";

export function DetailPanel() {
  const focusId = useSelectionStore((s) => s.focusId);
  const focus = useSelectionStore((s) => s.focus);
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
  // 「准备中」兜底的固定 deadline：useEffect 会随 2.5s 轮询重跑，用 setTimeout(60s)
  // 计时会被不断重置永不触发（曾导致取消后按钮永远卡在「准备中」）。
  const publishDeadlineRef = useRef(0);
  // 本次点击创建的任务 id：复位只认它，否则列表里上一次运行的终态任务会让
  // loading 在点击后立刻被复位（表现为按钮毫无变化）。
  const publishTaskIdRef = useRef<number | null>(null);
  const [transition, setTransition] = useState<{ action: "approve" | "eliminate" } | null>(null);
  const videoRef = useRef<HTMLVideoElement | null>(null);

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

  // 原生 <video controls> 的键盘快捷键只在视频元素聚焦时生效；点击全屏按钮后焦点留在按钮上，
  // 按空格会触发按钮（keyup 激活，keydown 的 preventDefault 挡不住）退出全屏、方向键也失效。
  // 进入全屏时把焦点移回视频元素（等价于手动点一下画面），原生快捷键即恢复；
  // 仅当视频未聚焦（原生未接管）时，keydown 兜底处理空格/方向键，避免与原生双重触发。
  useEffect(() => {
    const el = videoRef.current;
    if (!el) return;
    const onFsChange = () => {
      if (document.fullscreenElement === el) el.focus();
    };
    const onKey = (e: KeyboardEvent) => {
      if (document.fullscreenElement !== el) return;
      // 视频自身聚焦时原生快捷键在 capture 阶段已接管，此处若再处理会双重触发（如空格一下暂停又立刻播放）。
      if (document.activeElement === el) return;
      const tag = (e.target as HTMLElement | null)?.tagName;
      if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;
      if (e.code === "Space") {
        e.preventDefault();
        if (el.paused) void el.play().catch(() => {});
        else el.pause();
      } else if (e.code === "ArrowLeft") {
        e.preventDefault();
        el.currentTime = Math.max(0, (el.currentTime || 0) - 5);
      } else if (e.code === "ArrowRight") {
        e.preventDefault();
        const next = (el.currentTime || 0) + 5;
        el.currentTime = Number.isFinite(el.duration) ? Math.min(el.duration, next) : next;
      }
    };
    el.addEventListener("fullscreenchange", onFsChange);
    window.addEventListener("keydown", onKey);
    return () => {
      el.removeEventListener("fullscreenchange", onFsChange);
      window.removeEventListener("keydown", onKey);
    };
  }, [asset?.file_path]);

  // 准备发布后保持 loading，直到本次点击创建的任务进入可交互阶段
  // （stage=file 兜底弹窗 / WAITING_FOR_HUMAN 待确认）或终态（含取消）；60s 固定 deadline 兜底。
  useEffect(() => {
    if (!publishing || !asset) return;
    const taskId = publishTaskIdRef.current;
    const t = taskId == null ? undefined : tasks.find((t) => t.id === taskId);
    if (
      t &&
      (t.payload.stage === "file" ||
        t.status === "WAITING_FOR_HUMAN" ||
        ["SUCCEEDED", "FAILED", "CANCELLED"].includes(t.status))
    ) {
      setPublishing(false);
      return;
    }
    const remain = publishDeadlineRef.current - Date.now();
    if (remain <= 0) {
      setPublishing(false);
      return;
    }
    const timer = setTimeout(() => setPublishing(false), remain);
    return () => clearTimeout(timer);
  }, [publishing, tasks, asset]);

  const handleGenerateCopy = async () => {
    if (!asset) return;
    const assetId = asset.id;
    setCopying(true);
    try {
      const taskId = await api.generateCopy(assetId);
      // 文案由后台 content agent 生成：轮询「任务状态」直到终态再取文案。
      // 不能查文案是否存在——content_copies 按 asset upsert，旧文案 id/created_at 不变，
      // 曾导致一点击就查到旧文案、按钮提前复位（新文案几秒后才静默换上）。
      for (let i = 0; i < 40; i += 1) {
        await new Promise((r) => setTimeout(r, 1500));
        const list = await api.listTasks().catch(() => []);
        const t = list.find((t) => t.id === taskId);
        if (!t) continue;
        if (t.status === "SUCCEEDED") {
          const c = await api.getCopy(assetId).catch(() => null);
          // 用户中途切走时不把旧素材的文案写进新详情页。
          if (c && useSelectionStore.getState().focusId === assetId) setCopy(c);
          break;
        }
        if (t.status === "FAILED" || t.status === "CANCELLED") {
          if (t.error) alert(t.error);
          break;
        }
      }
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
    } finally {
      setCopying(false);
    }
  };

  // 过渡蒙层独立于素材存在：通过/淘汰后若当前素材被筛选列表移除，空态分支也要继续显示动画。
  const transitionOverlay = transition ? (
    <div className="detail-transition-overlay" data-action={transition.action}>
      <div className="detail-transition-badge">
        {transition.action === "approve" ? "✓" : "✕"}
      </div>
    </div>
  ) : null;

  if (!asset) {
    return (
      <aside className="relative flex min-w-0 flex-1 flex-col items-center justify-center bg-panel/60 text-sm text-mute">
        点击右侧素材查看详情
        {transitionOverlay}
      </aside>
    );
  }

  const nextStatus =
    asset.status === "CANDIDATE"
      ? "SELECTED"
      : asset.status === "SELECTED"
        ? "PUBLISHED"
        : asset.status === "PUBLISHED"
          ? "SELECTED"
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
  };

  // 候选素材「通过/淘汰」：先播过渡动画，动画结束后自动切换到下一个候选视频。
  // 期间用户若手动切换了素材，则不再自动跳转。
  const handleTransitionalStatus = async (to: string) => {
    const id = asset.id;
    setTransition(
      to === "SELECTED" ? { action: "approve" } : to === "ELIMINATED" ? { action: "eliminate" } : null,
    );
    try {
      await updateStatus(id, to);
      // 用户「通过」候选素材 -> 待发布，并自动生成 AI 文案。
      if (to === "SELECTED") void api.generateCopy(id).catch(() => {});
    } catch (e) {
      setTransition(null);
      throw e;
    }
    window.setTimeout(() => {
      setTransition(null);
      if (useSelectionStore.getState().focusId !== id) return;
      const all = useAssetsStore.getState().assets;
      const idx = all.findIndex((a) => a.id === id);
      const next =
        all.slice(idx + 1).find((a) => a.status === "CANDIDATE") ??
        all.find((a) => a.status === "CANDIDATE");
      if (next) focus(next.id);
    }, 700);
  };

  const onStatusAction = () => {
    // 候选素材的「通过」走动画过渡 + 自动跳转；其余状态流转保持原行为。
    if (asset.status === "CANDIDATE" && nextStatus === "SELECTED") {
      void handleTransitionalStatus("SELECTED");
    } else {
      void handleStatusAction();
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
    publishDeadlineRef.current = Date.now() + 60_000;
    publishTaskIdRef.current = null;
    try {
      const res = await api.preparePublish(asset.id);
      publishTaskIdRef.current = res.task_id;
      // 任务入队后稍等后台 worker 拉起 sidecar，再刷新任务数据（不打开底部任务面板）。
      setTimeout(() => void useTaskStore.getState().refresh(), 1200);
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
      setPublishing(false);
    }
  };

  return (
    <aside className="relative flex min-w-0 flex-1 flex-col bg-panel/60">
      <div className="min-h-0 flex-1 overflow-y-auto">
      <div className="p-4">
        <video
          key={asset.file_path}
          ref={videoRef}
          src={videoUrl(asset.file_path)}
          controls
          className="aspect-video w-full rounded-lg border border-line bg-black"
        />
      </div>

      <div className="px-4">
        <div className="flex items-start justify-between gap-2">
          <h2
            className="min-w-0 flex-1 truncate text-sm font-semibold leading-snug text-paper"
            title={asset.title ?? `视频 #${asset.id}`}
          >
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
            onClick={onStatusAction}
            className="h-9 rounded border border-accent bg-accent/10 px-3 text-sm text-accent hover:bg-accent/20"
            title={asset.status === "CANDIDATE" ? "标记为待发布并自动生成 AI 文案" : undefined}
          >
            {statusActionLabel}
          </button>
        )}
        {(asset.status === "CANDIDATE" || asset.status === "SELECTED") && (
          <button
            onClick={() => void handleTransitionalStatus("ELIMINATED")}
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
      {transitionOverlay}
    </aside>
  );
}
