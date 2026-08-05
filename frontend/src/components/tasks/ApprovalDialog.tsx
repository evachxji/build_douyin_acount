import { useTaskStore } from "../../stores/taskStore";

export function ApprovalDialog() {
  const tasks = useTaskStore((s) => s.tasks);
  const resume = useTaskStore((s) => s.resume);
  const cancel = useTaskStore((s) => s.cancel);
  const waiting = tasks.filter((t) => t.status === "WAITING_FOR_HUMAN");
  if (waiting.length === 0) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm">
      <div className="w-[520px] rounded-xl border border-accent/40 bg-panel p-6 shadow-2xl">
        <h2 className="text-base font-bold text-paper">需要人工确认</h2>
        <p className="mt-1 text-xs leading-relaxed text-mute">
          以下任务已执行到人工确认节点。请核对内容后再操作——最终发布类操作不会被自动执行。
        </p>
        <ul className="mt-4 space-y-2">
          {waiting.map((t) => (
            <li key={t.id} className="rounded-lg border border-line bg-ink/60 p-3">
              <div className="flex items-center gap-2 text-sm">
                <span className="text-paper/85">
                  #{t.id} · {t.agent_name} · {t.task_type}
                </span>
                <span className="text-mute">
                  {t.human_required ? "（发布需人工确认）" : ""}
                </span>
              </div>
              {t.error && <p className="mt-1 text-xs text-bad">{t.error}</p>}
              <div className="mt-2 flex gap-2">
                <button
                  onClick={() => void resume(t.id)}
                  className="rounded border border-ok/50 px-3 py-1 text-xs text-ok hover:bg-ok/10"
                >
                  确认，继续
                </button>
                <button
                  onClick={() => void cancel(t.id)}
                  className="rounded border border-bad/50 px-3 py-1 text-xs text-bad hover:bg-bad/10"
                >
                  取消任务
                </button>
              </div>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
