import { useTaskStore } from "../../stores/taskStore";
import { StatusBadge } from "../StatusBadge";
import { agentLabel, formatDateTime, taskTypeLabel } from "../../lib/format";

const TERMINAL_STATUSES = ["SUCCEEDED", "FAILED", "CANCELLED"];

export function TaskQueuePanel({ height }: { height: number }) {
  const tasks = useTaskStore((s) => s.tasks);
  const error = useTaskStore((s) => s.error);
  const setOpen = useTaskStore((s) => s.setOpen);

  return (
    <section
      style={{ height }}
      className="flex shrink-0 flex-col border-t border-line bg-panel"
    >
      <header className="flex h-9 items-center gap-3 border-b border-line px-4">
        <span className="text-xs font-semibold uppercase tracking-wider text-mute">
          Agent 任务队列
        </span>
        <span className="text-[11px] text-mute">
          {tasks.filter((t) => t.status === "PENDING" || t.status === "RUNNING").length} 进行中
        </span>
        {error && <span className="text-[11px] text-bad">刷新失败：{error}</span>}
        <button
          onClick={() => setOpen(false)}
          className="ml-auto text-sm text-mute hover:text-paper"
          title="收起"
        >
          ▾
        </button>
      </header>
      <div className="flex items-center gap-3 border-b border-line px-2 py-1.5 text-[11px] text-mute">
        <span className="w-20 shrink-0">状态</span>
        <span className="w-16 shrink-0">#ID</span>
        <span className="w-14 shrink-0">Agent</span>
        <span className="w-24 shrink-0">任务类型</span>
        <span className="w-16 shrink-0">素材</span>
        <span className="w-10 shrink-0 tabular-nums">尝试</span>
        <span className="w-20 shrink-0">创建时间</span>
        <span className="w-20 shrink-0">结束时间</span>
        <span className="min-w-0 flex-1">错误</span>
      </div>
      <ul className="min-h-0 flex-1 overflow-y-auto p-2">
        {tasks.length === 0 && (
          <li className="px-2 py-6 text-center text-xs text-mute">暂无任务</li>
        )}
        {tasks.slice(0, 30).map((t) => (
          <li
            key={t.id}
            className="flex items-center gap-3 rounded px-2 py-1.5 text-xs hover:bg-white/5"
          >
            <span className="w-20 shrink-0">
              <StatusBadge status={t.status} />
            </span>
            <span className="w-16 shrink-0 text-mute">#{t.id}</span>
            <span className="w-14 shrink-0 text-paper/85">{agentLabel(t.agent_name)}</span>
            <span className="w-24 shrink-0 text-paper/85">{taskTypeLabel(t.task_type)}</span>
            <span
              className="w-16 shrink-0 truncate text-mute"
              title={String(t.payload.asset_id ?? "")}
            >
              素材 {String(t.payload.asset_id ?? "—")}
            </span>
            <span className="w-10 shrink-0 tabular-nums text-mute">
              {t.attempts}/{t.max_attempts}
            </span>
            <span className="w-20 shrink-0 tabular-nums text-mute">
              {formatDateTime(t.created_at)}
            </span>
            <span className="w-20 shrink-0 tabular-nums text-mute">
              {TERMINAL_STATUSES.includes(t.status)
                ? formatDateTime(t.updated_at)
                : "—"}
            </span>
            {t.error && (
              <span className="min-w-0 flex-1 truncate text-bad" title={t.error}>
                {t.error}
              </span>
            )}
            {!t.error && <span className="min-w-0 flex-1" />}
          </li>
        ))}
      </ul>
    </section>
  );
}
