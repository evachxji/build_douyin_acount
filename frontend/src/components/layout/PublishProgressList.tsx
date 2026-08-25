import { useEffect, useState } from "react";
import { api } from "../../api/client";
import { useTaskStore } from "../../stores/taskStore";
import type { AgentTask } from "../../api/types";

/** 发布中列表（侧栏下方）：每个活跃发布任务一行，支持多任务并行。
 *  填表完成前转圈（处理中）；表单就绪（stage=form_ready）后变为 ✓/✕——
 *  点 ✓ 授权 sidecar 在页面代点一次发布按钮（直接生效，无二次确认）；
 *  点 ✕ 取消发布。用户自行在浏览器点发布同样会被检测登记。 */
export function PublishProgressList() {
  const tasks = useTaskStore((s) => s.tasks);
  const cancel = useTaskStore((s) => s.cancel);
  // 已点勾授权的任务 id：行内转「发布提交中」防重复点击，任务终态离开列表后清理。
  const [busyIds, setBusyIds] = useState<Set<number>>(new Set());

  const publishing = tasks.filter(
    (t) =>
      t.agent_name === "publisher" &&
      (t.status === "PENDING" || t.status === "RUNNING"),
  );

  useEffect(() => {
    setBusyIds((prev) => {
      if (prev.size === 0) return prev;
      const alive = new Set(
        tasks
          .filter(
            (t) =>
              t.agent_name === "publisher" &&
              (t.status === "PENDING" || t.status === "RUNNING"),
          )
          .map((t) => t.id),
      );
      const next = new Set([...prev].filter((id) => alive.has(id)));
      return next.size === prev.size ? prev : next;
    });
  }, [tasks]);

  if (publishing.length === 0) return null;

  const fmtSchedule = (t: AgentTask): string => {
    if (t.status === "PENDING") return "排队中";
    const s = t.payload.schedule_time;
    if (typeof s === "string" && s) return s.slice(5, 16); // "YYYY-MM-DD HH:mm" → "MM-DD HH:mm"
    if (s === null) return "待人工选时";
    return "计算中…";
  };

  const authorize = async (t: AgentTask) => {
    const draftId = Number(t.payload.draft_id);
    if (!Number.isInteger(draftId)) return;
    setBusyIds((prev) => new Set(prev).add(t.id));
    try {
      await api.publishConfirmed(draftId);
    } catch {
      setBusyIds((prev) => {
        const next = new Set(prev);
        next.delete(t.id);
        return next;
      });
    }
  };

  return (
    <div className="border-t border-line px-3 py-2">
      <div className="mb-1 px-1 text-[11px] font-semibold uppercase tracking-wider text-mute">
        发布中
      </div>
      <ul className="space-y-1">
        {publishing.map((t) => {
          const ready = t.payload.stage === "form_ready";
          const busy = busyIds.has(t.id);
          const title =
            typeof t.payload.title === "string" && t.payload.title
              ? t.payload.title
              : `视频 #${String(t.payload.asset_id ?? "?")}`;
          const message =
            typeof t.payload.message === "string" ? t.payload.message : "";
          return (
            <li key={t.id} className="rounded px-1 py-1 hover:bg-white/5">
              <div className="flex items-center gap-1.5">
                {ready && !busy ? (
                  <>
                    <button
                      onClick={() => void authorize(t)}
                      title="确认发布：页面将自动点击【发布】按钮"
                      className="shrink-0 rounded border border-accent/60 px-1.5 py-0.5 text-[11px] font-medium text-accent transition-colors hover:bg-accent/15"
                    >
                      ✓
                    </button>
                    <button
                      onClick={() => void cancel(t.id)}
                      title="取消发布"
                      className="shrink-0 rounded border border-bad/50 px-1.5 py-0.5 text-[11px] font-medium text-bad transition-colors hover:bg-bad/10"
                    >
                      ✕
                    </button>
                  </>
                ) : (
                  <span
                    className="h-3.5 w-3.5 shrink-0 animate-spin rounded-full border-2 border-mute/40 border-t-accent"
                    title={busy ? "发布提交中…" : "发布准备中…"}
                  />
                )}
                <span className="min-w-0 flex-1 truncate text-xs text-paper/85" title={title}>
                  {title}
                </span>
                <span className="shrink-0 text-[11px] tabular-nums text-mute">
                  {fmtSchedule(t)}
                </span>
              </div>
              {message && (
                <p className="mt-0.5 truncate pl-1 text-[11px] text-mute" title={message}>
                  {message}
                </p>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
