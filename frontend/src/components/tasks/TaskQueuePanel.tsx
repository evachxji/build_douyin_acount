import { useTaskStore } from "../../stores/taskStore";
import { StatusBadge } from "../StatusBadge";
import { agentLabel, taskTypeLabel } from "../../lib/format";

export function TaskQueuePanel() {
  const tasks = useTaskStore((s) => s.tasks);
  const error = useTaskStore((s) => s.error);
  const setOpen = useTaskStore((s) => s.setOpen);
  const resume = useTaskStore((s) => s.resume);
  const cancel = useTaskStore((s) => s.cancel);

  return (
    <section className="h-56 shrink-0 border-t border-line bg-panel">
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
      <ul className="h-[calc(100%-2.25rem)] overflow-y-auto p-2">
        {tasks.length === 0 && (
          <li className="px-2 py-6 text-center text-xs text-mute">暂无任务</li>
        )}
        {tasks.slice(0, 30).map((t) => (
          <li
            key={t.id}
            className="flex items-center gap-3 rounded px-2 py-1.5 text-xs hover:bg-white/5"
          >
            <StatusBadge status={t.status} />
            <span className="w-16 shrink-0 text-mute">#{t.id}</span>
            <span className="w-14 shrink-0 text-paper/85">{agentLabel(t.agent_name)}</span>
            <span className="w-24 shrink-0 text-paper/85">{taskTypeLabel(t.task_type)}</span>
            <span className="shrink-0 text-mute">
              素材 {String(t.payload.asset_id ?? "—")}
            </span>
            <span className="shrink-0 tabular-nums text-mute">
              {t.attempts}/{t.max_attempts}
            </span>
            {t.error && (
              <span className="min-w-0 flex-1 truncate text-bad" title={t.error}>
                {t.error}
              </span>
            )}
            {!t.error && <span className="min-w-0 flex-1" />}
            {t.status === "WAITING_FOR_HUMAN" && (
              <span className="flex shrink-0 gap-1.5">
                <button
                  onClick={() => void resume(t.id)}
                  className="rounded border border-ok/40 px-2 py-0.5 text-ok hover:bg-ok/10"
                >
                  放行
                </button>
                <button
                  onClick={() => void cancel(t.id)}
                  className="rounded border border-bad/40 px-2 py-0.5 text-bad hover:bg-bad/10"
                >
                  取消
                </button>
              </span>
            )}
          </li>
        ))}
      </ul>
    </section>
  );
}
