import { useAssetsStore } from "../../stores/assetsStore";
import { useSelectionStore } from "../../stores/selectionStore";
import { useTaskStore } from "../../stores/taskStore";
import { videoUrl, coverUrl } from "../../api/urls";
import { formatCount, formatDuration, platformLabel } from "../../lib/format";
import { ScoreBadge } from "../ScoreBadge";
import { StatusBadge } from "../StatusBadge";
import { AddMusicDialog } from "./AddMusicDialog";
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
  // 刚点击【准备发布】、任务尚未出现在 2.5s 轮询列表时的本地过渡 loading
  // （按素材 id 绑定：切换到其他视频时，其按钮保持可点）。
  const [publishingId, setPublishingId] = useState<number | null>(null);
  const [transition, setTransition] = useState<{ action: "approve" | "eliminate" } | null>(null);
  const [musicOpen, setMusicOpen] = useState(false);
  const videoRef = useRef<HTMLVideoElement | null>(null);
  // 视频元素「代次」：添加音乐合成完成后 +1 强制 <video> 重挂载——二次合成时输出文件路径不变
  // （固定 <uuid>.music.mp4，file_path 不变 key 就不变），旧元素持有已被 ffmpeg 覆盖的文件，
  // 继续播放会因解码状态损坏卡死页面；重挂载释放旧句柄、从新文件头开始。
  const [videoEpoch, setVideoEpoch] = useState(0);
  // 已出帧的视频路径：切换素材后视频首帧加载期间（约几百毫秒）用封面图盖住黑屏加载态，
  // 本地视频出帧后即刻撤掉封面，切换看起来是瞬时的。
  const [readyPath, setReadyPath] = useState<string | null>(null);

  const asset = assets.find((a) => a.id === focusId) ?? null;

  // 该素材的活跃发布任务：按钮状态由任务数据驱动——切换视频时各自绑定，
  // 任务终态（发布/取消/失败）后自动恢复可点；活跃期禁止重复入队。
  const activePublishTask = asset
    ? tasks.find(
        (t) =>
          t.agent_name === "publisher" &&
          Number(t.payload.asset_id) === asset.id &&
          (t.status === "PENDING" || t.status === "RUNNING"),
      )
    : undefined;
  const publishing = publishingId === asset?.id || activePublishTask != null;
  // 添加音乐入口（音符图标按钮）仅候选/待发布状态显示，固定在删除按钮左侧。
  const showMusicBtn = asset != null && ["CANDIDATE", "SELECTED"].includes(asset.status);

  useEffect(() => {
    if (!asset) {
      setAnalysis(null);
      setCopy(null);
      return;
    }
    setAnalysis(null);
    setCopy(null);
    // 切换素材时关掉可能还开着的添加音乐弹窗（弹窗绑定的是旧素材）。
    setMusicOpen(false);
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
  }, [asset?.file_path, videoEpoch]);

  // 任务出现在轮询列表后，本地过渡 loading 交给任务数据驱动（activePublishTask）。
  useEffect(() => {
    if (publishingId == null) return;
    const appeared = tasks.some(
      (t) =>
        t.agent_name === "publisher" &&
        Number(t.payload.asset_id) === publishingId &&
        (t.status === "PENDING" || t.status === "RUNNING"),
    );
    if (appeared) setPublishingId(null);
  }, [publishingId, tasks]);

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
    if (!asset) return;
    setPublishingId(asset.id);
    try {
      await api.preparePublish(asset.id);
      // 任务入队后稍等后台 worker 拉起 sidecar，再刷新任务数据（不打开底部任务面板）。
      setTimeout(() => void useTaskStore.getState().refresh(), 1200);
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
      setPublishingId(null);
    }
  };

  return (
    <aside className="relative flex min-w-0 flex-1 flex-col bg-panel/60">
      <div className="min-h-0 flex-1 overflow-y-auto">
      <div className="p-4">
        <div className="relative">
          <video
            key={`${asset.file_path}:${videoEpoch}`}
            ref={videoRef}
            // ?v= 破坏浏览器对同路径旧内容的启发式缓存（二次合成同路径覆盖后必须读到新文件）
            src={`${videoUrl(asset.file_path)}?v=${videoEpoch}`}
            controls
            onLoadedData={() => setReadyPath(asset.file_path)}
            className="aspect-video w-full rounded-lg border border-line bg-black"
          />
          {readyPath !== asset.file_path && asset.cover_path && (
            <img
              src={coverUrl(asset.cover_path)}
              alt=""
              className="pointer-events-none absolute inset-0 h-full w-full rounded-lg border border-line bg-black object-contain"
            />
          )}
        </div>
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
        {["NEW", "ANALYZING", "CANDIDATE", "SELECTED"].includes(asset.status) && (
          <button
            onClick={() => void handleTransitionalStatus("ELIMINATED")}
            className="h-9 rounded border border-bad/40 px-3 text-sm text-bad hover:bg-bad/10"
            title="标记为淘汰，不再参与发布；进行中/排队中的分析任务会一并取消"
          >
            淘汰
          </button>
        )}
        {asset.status === "SELECTED" && (
        <button
          onClick={() => void handlePreparePublish()}
          disabled={publishing}
          className="inline-flex h-9 items-center justify-center gap-1.5 rounded border border-accent bg-accent px-3 text-sm font-medium text-ink hover:brightness-110 disabled:cursor-not-allowed disabled:opacity-40"
          title={
            activePublishTask?.payload.stage === "form_ready"
              ? "表单已填好，请到左侧栏「发布中」列表点 ✓ 确认发布"
              : "打开独立浏览器窗口，自动上传视频并填写标题/文案/话题；表单就绪后在左侧栏点 ✓ 确认发布"
          }
        >
          {publishing ? (
            activePublishTask?.payload.stage === "form_ready" ? (
              "待确认发布"
            ) : (
              <>
                <span className="h-3.5 w-3.5 animate-spin rounded-full border-2 border-ink/40 border-t-ink" />
                准备中…
              </>
            )
          ) : (
            "准备发布"
          )}
        </button>
        )}
        {showMusicBtn && (
          <button
            onClick={() => {
              // 先暂停正在播放的视频，避免试听/合成时与视频声音叠在一起。
              videoRef.current?.pause();
              setMusicOpen(true);
            }}
            disabled={publishing}
            className={`ml-auto flex h-9 w-9 items-center justify-center rounded border text-sm disabled:opacity-40 ${
              asset.music_info
                ? "border-accent bg-accent/15 text-accent"
                : "border-line text-mute hover:border-accent/50 hover:text-paper"
            }`}
            title={
              publishing
                ? "发布任务进行中，暂不能替换视频文件"
                : asset.music_info
                  ? `已添加：${asset.music_info.music_name}，再次合成将从原始视频重来`
                  : "添加音乐：从音乐库选一首合成进视频（可试听、可关闭原声）"
            }
          >
            ♪
          </button>
        )}
        <button
          onClick={() => void handleDelete()}
          className={`${showMusicBtn ? "" : "ml-auto "}h-9 rounded border border-bad/40 px-3 text-sm text-bad hover:bg-bad/10`}
        >
          删除
        </button>
      </div>
      {transitionOverlay}
      {musicOpen && (
        <AddMusicDialog
          asset={asset}
          onClose={() => setMusicOpen(false)}
          onApplied={() => {
            setMusicOpen(false);
            // 强制 <video> 重挂载：二次合成是同路径覆盖，旧元素持有的文件已失效；
            // 随后刷新列表让 file_path/music_info 进组件（首次合成时 file_path 本身也会变）。
            setVideoEpoch((e) => e + 1);
            void useAssetsStore.getState().load();
          }}
        />
      )}
    </aside>
  );
}
