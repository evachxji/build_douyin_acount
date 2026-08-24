import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { ask } from "@tauri-apps/plugin-dialog";
import { api } from "../../api/client";
import { coverUrl } from "../../api/urls";
import type { DouyinPost } from "../../api/types";
import { useDouyinPostsStore } from "../../stores/douyinPostsStore";
import { useTaskStore } from "../../stores/taskStore";
import { formatCount, formatSyncAgo } from "../../lib/format";
import {
  dayMarks,
  formatHM,
  formatRate,
  groupPostsByDay,
  hasViolation,
  isPrivate,
  isScheduled,
  isTodayEpoch,
  monthCells,
  postTime,
  postUrl,
  shiftMonth,
  todayKey,
  violationLabel,
  visibilityLabel,
} from "../../lib/calendar";

const WEEKDAYS = ["一", "二", "三", "四", "五", "六", "日"];
const PANEL_WIDTH = 700;

function dayTitle(key: string): string {
  const [y, m, d] = key.split("-").map(Number);
  const date = new Date(y, m - 1, d);
  const wd = "日一二三四五六"[date.getDay()];
  return `${m}月${d}日 周${wd}`;
}

function PostRow({ post, moderateActive }: { post: DouyinPost; moderateActive: boolean }) {
  const scheduled = isScheduled(post);
  const violating = hasViolation(post);
  const cover = coverUrl(post.cover_path);
  const setTaskOpen = useTaskStore((s) => s.setOpen);

  // 一键处理：违规作品设私密；今天发布的还会把最晚定时作品改为立即发布（先确认再执行）。
  const handleModerate = async () => {
    const publishedToday = isTodayEpoch(post.create_time);
    const ok = await ask(
      `将把「${(post.title ?? "").slice(0, 30) || post.aweme_id}」设为仅自己可见。` +
        (publishedToday
          ? "\n该作品发布于今天：权限修改成功后，最晚的一条定时作品将改为立即发布。"
          : "") +
        "\n\n确定继续吗？",
      { title: "一键处理违规作品", kind: "warning" },
    );
    if (!ok) return;
    try {
      await api.moderateDouyinViolation(post.aweme_id);
      setTaskOpen(true);
      setTimeout(() => void useTaskStore.getState().refresh(), 1200);
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <li
      className={`relative flex gap-2.5 rounded-md px-2 py-2 ${
        violating
          ? "border-l-2 border-bad bg-bad/10"
          : scheduled
            ? "border-l-2 border-warn bg-warn/5"
            : "hover:bg-white/5"
      }`}
    >
      {cover ? (
        <img
          src={cover}
          loading="lazy"
          className="h-14 w-10 shrink-0 rounded object-cover"
          alt=""
        />
      ) : (
        <div className="flex h-14 w-10 shrink-0 items-center justify-center rounded bg-panel2 text-mute">
          ▶
        </div>
      )}
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline gap-2">
          <span
            className={`shrink-0 text-xs font-semibold tabular-nums ${
              scheduled ? "text-warn" : "text-accent"
            }`}
          >
            {scheduled ? `定时 ${formatHM(postTime(post))}` : formatHM(postTime(post))}
          </span>
          <span
            className={`shrink-0 rounded px-1 py-px text-[10px] leading-4 ${
              isPrivate(post) ? "bg-warn/15 text-warn" : "bg-ok/15 text-ok"
            }`}
          >
            {visibilityLabel(post)}
          </span>
          <button
            onClick={() => void api.openUrl(postUrl(post)).catch((e) => alert(String(e)))}
            className="min-w-0 truncate text-left text-xs text-paper hover:text-accent hover:underline"
            title={`${post.title ?? ""}\n${scheduled ? "打开编辑页" : "打开作品详情页"}`}
          >
            {post.title || "（无标题）"}
          </button>
        </div>
        {violating && (
          <div
            className="mt-1 flex items-center gap-1 text-[11px] font-semibold text-bad"
            title={post.violation_text ?? "作品违规"}
          >
            <span>⚠</span>
            <span className="truncate">{violationLabel(post)}</span>
          </div>
        )}
        {!scheduled && (
          <>
            <div className="mt-1 text-[11px] tabular-nums text-mute">
              ▶ {formatCount(post.play_count)} · ♥ {formatCount(post.digg_count)} · 评{" "}
              {formatCount(post.comment_count)}
            </div>
            <div className="mt-0.5 text-[11px] tabular-nums text-mute/80">
              完播 {formatRate(post.completion_rate)} · 2秒跳出 {formatRate(post.bounce_rate_2s)}
            </div>
          </>
        )}
      </div>
      {violating && !scheduled && !isPrivate(post) && (
        <button
          onClick={() => void handleModerate()}
          disabled={moderateActive}
          className="absolute bottom-2 right-2 rounded border border-bad/60 px-1.5 py-0.5 text-[11px] text-bad transition-colors hover:bg-bad/15 disabled:cursor-not-allowed disabled:opacity-40"
          title="设为仅自己可见；若今天发布，最晚的定时作品将改为立即发布"
        >
          一键处理
        </button>
      )}
    </li>
  );
}

/** 发布日历弹层：左侧月历（滚轮/按钮切月），右侧联动显示悬停日期的作品列表。 */
export function CalendarPopover({
  anchorRef,
  onClose,
}: {
  anchorRef: React.RefObject<HTMLButtonElement | null>;
  onClose: () => void;
}) {
  const posts = useDouyinPostsStore((s) => s.posts);
  const loaded = useDouyinPostsStore((s) => s.loaded);
  const load = useDouyinPostsStore((s) => s.load);
  const tasks = useTaskStore((s) => s.tasks);
  const setTaskOpen = useTaskStore((s) => s.setOpen);

  const panelRef = useRef<HTMLDivElement | null>(null);
  const now = new Date();
  const [view, setView] = useState({ year: now.getFullYear(), month: now.getMonth() });
  // 悬停日期始终跟踪；点击日期后锁定，右侧列表不再随悬停变动（再点解锁/点别的改锁）。
  const [hoverDay, setHoverDay] = useState(todayKey());
  const [lockedDay, setLockedDay] = useState<string | null>(null);
  const activeDay = lockedDay ?? hoverDay;
  const [syncing, setSyncing] = useState(false);
  const wheelAccRef = useRef(0);
  const wheelLockRef = useRef(0);

  useEffect(() => {
    if (!loaded) void load();
  }, [loaded, load]);

  // 初始位置：锚点按钮正下方、左对齐，防右侧越界。
  const [pos] = useState(() => {
    const rect = anchorRef.current?.getBoundingClientRect();
    const left = Math.min(rect?.left ?? 80, window.innerWidth - PANEL_WIDTH - 8);
    return { left: Math.max(8, left), top: (rect?.bottom ?? 56) + 6 };
  });

  // 打开后按实际尺寸修正位置，避免超出窗口边缘。
  useLayoutEffect(() => {
    if (!panelRef.current) return;
    const rect = panelRef.current.getBoundingClientRect();
    if (rect.bottom > window.innerHeight) {
      panelRef.current.style.top = `${Math.max(8, window.innerHeight - rect.height - 8)}px`;
    }
    if (rect.right > window.innerWidth) {
      panelRef.current.style.left = `${Math.max(8, window.innerWidth - rect.width - 8)}px`;
    }
  }, []);

  // 点击外部 / Escape 关闭。
  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (panelRef.current?.contains(e.target as Node)) return;
      if (anchorRef.current?.contains(e.target as Node)) return;
      onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [anchorRef, onClose]);

  const grouped = useMemo(() => groupPostsByDay(posts), [posts]);
  const cells = useMemo(() => monthCells(view.year, view.month), [view]);
  const today = todayKey();
  const activePosts = grouped.get(activeDay) ?? [];

  const syncTaskActive = tasks.some(
    (t) => t.agent_name === "douyin_sync" && ["PENDING", "RUNNING"].includes(t.status),
  );
  const moderateActive = tasks.some(
    (t) => t.agent_name === "douyin_moderate" && ["PENDING", "RUNNING"].includes(t.status),
  );
  const violationCount = useMemo(
    () => posts.filter((p) => hasViolation(p) && !isPrivate(p)).length,
    [posts],
  );
  const lastSynced = posts.reduce<string | null>(
    (acc, p) => (acc == null || p.synced_at > acc ? p.synced_at : acc),
    null,
  );

  const moveMonth = (delta: number) => setView((v) => shiftMonth(v.year, v.month, delta));

  // 滚轮切月：累加 deltaY 过阈值才翻页，带 200ms 锁避免触控板连跳。
  const onWheel = (e: React.WheelEvent) => {
    wheelAccRef.current += e.deltaY;
    const nowMs = Date.now();
    if (nowMs - wheelLockRef.current < 200) return;
    if (Math.abs(wheelAccRef.current) < 40) return;
    moveMonth(wheelAccRef.current > 0 ? 1 : -1);
    wheelAccRef.current = 0;
    wheelLockRef.current = nowMs;
  };

  const goToday = () => {
    const d = new Date();
    setView({ year: d.getFullYear(), month: d.getMonth() });
    setHoverDay(todayKey());
    setLockedDay(null);
  };

  const handleSync = async () => {
    setSyncing(true);
    try {
      await api.startDouyinSync();
      setTaskOpen(true);
      setTimeout(() => void useTaskStore.getState().refresh(), 1200);
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
    } finally {
      setSyncing(false);
    }
  };

  // 处理所有违规作品：全部设私密；今天发布的各触发一条最晚定时作品立即发布。
  const handleModerateAll = async () => {
    const violating = posts.filter((p) => hasViolation(p) && !isPrivate(p));
    const todayCount = violating.filter((p) => isTodayEpoch(p.create_time)).length;
    const scheduledCount = posts.filter((p) => isScheduled(p)).length;
    const promoteCount = Math.min(todayCount, scheduledCount);
    const ok = await ask(
      `共 ${violating.length} 条违规作品，将全部设为仅自己可见。` +
        (promoteCount > 0
          ? `\n其中 ${todayCount} 条发布于今天：将依次把最晚的 ${promoteCount} 条定时作品改为立即发布。`
          : "\n没有发布于今天的违规作品，不会改动定时作品。") +
        "\n\n确定继续吗？",
      { title: "处理所有违规作品", kind: "warning" },
    );
    if (!ok) return;
    try {
      await api.moderateAllViolations();
      setTaskOpen(true);
      setTimeout(() => void useTaskStore.getState().refresh(), 1200);
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
    }
  };

  return createPortal(
    <div
      ref={panelRef}
      style={{ left: pos.left, top: pos.top, width: PANEL_WIDTH }}
      className="fixed z-50 flex flex-col overflow-hidden rounded-xl border border-line bg-panel shadow-2xl"
    >
      <div className="flex min-h-0">
        {/* 左侧：月历 */}
        <div className="w-72 shrink-0 select-none p-3" onWheel={onWheel}>
          <div className="mb-2 flex items-center gap-1">
            <button
              onClick={() => moveMonth(-1)}
              className="h-6 w-6 rounded text-mute hover:bg-white/10 hover:text-paper"
              title="上一月"
            >
              ‹
            </button>
            <span className="flex-1 text-center text-sm font-semibold text-paper">
              {view.year}年{view.month + 1}月
            </span>
            <button
              onClick={() => moveMonth(1)}
              className="h-6 w-6 rounded text-mute hover:bg-white/10 hover:text-paper"
              title="下一月"
            >
              ›
            </button>
            <button
              onClick={goToday}
              className="ml-1 rounded border border-line px-1.5 py-0.5 text-[11px] text-mute hover:border-accent/40 hover:text-paper"
            >
              今天
            </button>
          </div>
          <div className="mb-1 grid grid-cols-7 text-center text-[11px] text-mute">
            {WEEKDAYS.map((w) => (
              <span key={w}>{w}</span>
            ))}
          </div>
          <div className="grid grid-cols-7 gap-y-0.5">
            {cells.map((c) => {
              const marks = dayMarks(grouped.get(c.key));
              const isToday = c.key === today;
              const isActive = c.key === activeDay;
              const isLocked = c.key === lockedDay;
              return (
                <button
                  key={c.key}
                  onMouseEnter={() => setHoverDay(c.key)}
                  onClick={() =>
                    setLockedDay((prev) => (prev === c.key ? null : c.key))
                  }
                  title={isLocked ? "已锁定，再次点击解锁" : "点击锁定该日期"}
                  className={`flex h-10 flex-col items-center justify-center rounded text-xs transition-colors ${
                    c.inMonth ? "" : "opacity-35"
                  } ${
                    isLocked
                      ? "bg-accent/20 ring-1 ring-accent"
                      : isActive
                        ? "bg-white/10"
                        : "hover:bg-white/5"
                  } ${isToday ? "font-bold text-accent" : "text-paper/85"}`}
                >
                  <span>{c.day}</span>
                  <span className="mt-0.5 flex h-1.5 gap-0.5">
                    {marks.published && <span className="h-1.5 w-1.5 rounded-full bg-info" />}
                    {marks.scheduled && <span className="h-1.5 w-1.5 rounded-full bg-warn" />}
                    {marks.violating && <span className="h-1.5 w-1.5 rounded-full bg-bad" />}
                  </span>
                </button>
              );
            })}
          </div>
          <div className="mt-2 flex items-center gap-3 border-t border-line/60 pt-2 text-[11px] text-mute">
            <span className="flex items-center gap-1">
              <span className="h-1.5 w-1.5 rounded-full bg-info" /> 已发布
            </span>
            <span className="flex items-center gap-1">
              <span className="h-1.5 w-1.5 rounded-full bg-warn" /> 定时
            </span>
            <span className="flex items-center gap-1">
              <span className="h-1.5 w-1.5 rounded-full bg-bad" /> 违规
            </span>
            <span className="ml-auto text-mute/60">滚轮切换月份</span>
          </div>
        </div>

        {/* 右侧：悬停日期的作品列表 */}
        <div className="flex min-w-0 flex-1 flex-col border-l border-line">
          <div className="flex h-11 shrink-0 items-baseline gap-2 border-b border-line px-3 pt-2.5">
            <span className="text-sm font-semibold text-paper">{dayTitle(activeDay)}</span>
            <span className="text-[11px] text-mute">
              {activePosts.length > 0 ? `${activePosts.length} 条` : ""}
            </span>
            {lockedDay && (
              <span className="ml-auto text-[11px] text-accent" title="已锁定：再次点击该日期解锁">
                🔒 已锁定
              </span>
            )}
          </div>
          <ul className="min-h-0 flex-1 overflow-y-auto p-2" style={{ maxHeight: 320 }}>
            {activePosts.length === 0 && (
              <li className="px-2 py-8 text-center text-xs text-mute">
                {loaded && posts.length === 0
                  ? "还没有作品数据，点击下方按钮从抖音创作者中心同步"
                  : "这一天没有发布视频"}
              </li>
            )}
            {activePosts.map((p) => (
              <PostRow key={p.aweme_id} post={p} moderateActive={moderateActive} />
            ))}
          </ul>
          <div className="flex h-12 shrink-0 items-center gap-2 border-t border-line px-3">
            <button
              onClick={() => void handleSync()}
              disabled={syncing || syncTaskActive}
              className="h-8 rounded border px-3 text-xs transition-colors disabled:cursor-not-allowed disabled:opacity-40 enabled:border-accent enabled:bg-accent enabled:text-ink enabled:hover:brightness-110"
              title="从抖音创作者中心同步作品列表与数据（需 Chrome 已登录）"
            >
              {syncTaskActive || syncing ? "同步中…" : "同步抖音数据"}
            </button>
            <button
              onClick={() => void handleModerateAll()}
              disabled={violationCount === 0 || moderateActive}
              className="h-8 rounded border px-3 text-xs transition-colors disabled:cursor-not-allowed disabled:opacity-40 enabled:border-bad enabled:text-bad enabled:hover:bg-bad/10"
              title={
                violationCount === 0
                  ? "当前没有违规作品"
                  : `将 ${violationCount} 条违规作品全部设为仅自己可见；今天发布的会触发最晚定时作品立即发布`
              }
            >
              {moderateActive ? "处理中…" : `处理所有违规视频${violationCount > 0 ? ` (${violationCount})` : ""}`}
            </button>
            <span className="ml-auto text-[11px] text-mute">
              {lastSynced ? `上次同步 ${formatSyncAgo(lastSynced)}` : "尚未同步过"}
            </span>
          </div>
        </div>
      </div>
    </div>,
    document.body,
  );
}
