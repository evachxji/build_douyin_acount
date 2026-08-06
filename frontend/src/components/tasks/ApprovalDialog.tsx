import { useEffect, useState } from "react";
import { api } from "../../api/client";
import type { AgentTask } from "../../api/types";
import { useTaskStore } from "../../stores/taskStore";
import { agentLabel, taskTypeLabel } from "../../lib/format";

/** 复制路径到系统剪贴板（后端 arboard，可靠）。 */
function CopyPath({ path }: { path: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      onClick={() =>
        void api
          .copyText(path)
          .then(() => {
            setCopied(true);
            setTimeout(() => setCopied(false), 1500);
          })
          .catch(() => {})
      }
      className="shrink-0 rounded border border-line px-2 py-0.5 text-[11px] text-mute hover:border-accent/50 hover:text-paper"
    >
      {copied ? "已复制" : "复制"}
    </button>
  );
}

/** 选文件阶段信息弹窗：任务 RUNNING 且 payload.stage=file 时展示（不阻塞流程）。 */
function FileStagePanel({ task }: { task: AgentTask }) {
  const cancel = useTaskStore((s) => s.cancel);
  const path = String(task.payload.path ?? "");
  const title = String(task.payload.title ?? "");

  // 弹窗出现时自动复制路径，用户直接粘贴。
  useEffect(() => {
    if (path) void api.copyText(path).catch(() => {});
  }, [path]);

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/70 backdrop-blur-sm">
      <div className="w-[520px] rounded-xl border border-accent/40 bg-panel p-6 shadow-2xl">
        <h2 className="text-base font-bold text-paper">请选择视频文件</h2>
        <p className="mt-1 text-xs leading-relaxed text-mute">
          在打开的抖音页面点击「上传视频」，在文件选择弹窗中粘贴路径（已自动复制）并选择文件。
          选择后程序将自动填写标题/文案/话题，无需再点确认。
        </p>
        <div className="mt-3 space-y-2 text-sm">
          <div className="flex items-center gap-2">
            <span className="w-14 shrink-0 text-xs text-mute">视频标题</span>
            <span className="min-w-0 flex-1 truncate text-paper/85">{title || "—"}</span>
          </div>
          <div className="flex items-center gap-2">
            <span className="w-14 shrink-0 text-xs text-mute">视频路径</span>
            <code className="min-w-0 flex-1 truncate text-xs text-paper/85">{path}</code>
            {path && <CopyPath path={path} />}
          </div>
        </div>
        <div className="mt-4 flex justify-end">
          <button
            onClick={() => void cancel(task.id)}
            className="rounded border border-bad/50 px-3 py-1 text-xs text-bad hover:bg-bad/10"
          >
            取消任务
          </button>
        </div>
      </div>
    </div>
  );
}

export function ApprovalDialog() {
  const tasks = useTaskStore((s) => s.tasks);
  const resume = useTaskStore((s) => s.resume);
  const cancel = useTaskStore((s) => s.cancel);
  // 正在提交确认的弹窗任务 id：处理期间禁用按钮并显示 loading，避免重复点击。
  const [busyId, setBusyId] = useState<number | null>(null);
  const waiting = tasks.filter((t) => t.status === "WAITING_FOR_HUMAN");

  // 选文件阶段：任务 RUNNING 且 stage=file（自动填表前的信息提示）。
  const fileStage = tasks.find(
    (t) =>
      t.agent_name === "publisher" &&
      t.payload.stage === "file" &&
      (t.status === "RUNNING" || t.status === "PENDING"),
  );
  if (fileStage) return <FileStagePanel task={fileStage} />;

  if (waiting.length === 0) return null;

  const confirmTask = async (t: AgentTask) => {
    if (busyId !== null) return;
    setBusyId(t.id);
    try {
      if (t.agent_name === "publisher") {
        await api.publishConfirmed(Number(t.payload.draft_id));
      } else {
        await resume(t.id);
      }
    } finally {
      setBusyId(null);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm">
      <div className="w-[520px] rounded-xl border border-accent/40 bg-panel p-6 shadow-2xl">
        <h2 className="text-base font-bold text-paper">需要人工确认</h2>
        <p className="mt-1 text-xs leading-relaxed text-mute">
          请核对内容后再操作——最终发布类操作不会被自动执行。
        </p>
        <ul className="mt-4 max-h-[60vh] space-y-2 overflow-y-auto">
          {waiting.map((t) => {
            const isPub = t.agent_name === "publisher";
            return (
              <li key={t.id} className="rounded-lg border border-line bg-ink/60 p-3">
                <div className="flex items-center gap-2 text-sm">
                  <span className="text-paper/85">
                    #{t.id} · {agentLabel(t.agent_name)} · {taskTypeLabel(t.task_type)}
                  </span>
                </div>
                {isPub && typeof t.payload.message === "string" && (
                  <p className="mt-2 rounded border border-warn/30 bg-warn/5 p-2 text-xs leading-relaxed text-paper/85">
                    {t.payload.message}
                  </p>
                )}
                {t.error && <p className="mt-1 text-xs text-bad">{t.error}</p>}
                <div className="mt-2 flex gap-2">
                  <button
                    onClick={() => void confirmTask(t)}
                    disabled={busyId === t.id}
                    className="inline-flex items-center gap-1.5 rounded border border-accent bg-accent px-3 py-1 text-xs font-medium text-ink hover:brightness-110 disabled:cursor-not-allowed disabled:opacity-60"
                  >
                    {busyId === t.id ? (
                      <>
                        <span className="h-3 w-3 animate-spin rounded-full border-2 border-ink/40 border-t-ink" />
                        处理中…
                      </>
                    ) : (
                      <>{isPub ? "我已发布" : "确认，继续"}</>
                    )}
                  </button>
                  <button
                    onClick={() => void cancel(t.id)}
                    className="rounded border border-bad/50 px-3 py-1 text-xs text-bad hover:bg-bad/10"
                  >
                    取消任务
                  </button>
                </div>
              </li>
            );
          })}
        </ul>
      </div>
    </div>
  );
}
