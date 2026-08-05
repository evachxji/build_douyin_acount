import { useEffect, useState } from "react";
import { api } from "../../api/client";
import type { AgentTask, PublishDraft } from "../../api/types";
import { useTaskStore } from "../../stores/taskStore";

function DraftSummary({ draft }: { draft?: PublishDraft }) {
  if (!draft) return null;
  return (
    <div className="mt-2 space-y-1 text-xs text-mute">
      <p>
        标题：<span className="text-paper/85">{draft.title ?? "—"}</span>
      </p>
      <p className="line-clamp-2">
        文案：<span className="text-paper/75">{draft.body ?? "—"}</span>
      </p>
      <p>
        话题：
        <span className="text-paper/75">
          {draft.hashtags.map((h) => `#${h}`).join(" ") || "—"}
        </span>
      </p>
    </div>
  );
}

/** 发布任务的「已选择」/「我已发布」操作按钮。 */
function PublisherActions({ task }: { task: AgentTask }) {
  const resume = useTaskStore((s) => s.resume);
  const cancel = useTaskStore((s) => s.cancel);
  const [copied, setCopied] = useState(false);
  const draftId = Number(task.payload.draft_id);
  const stage = task.payload.stage;

  const copyPath = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      const ta = document.createElement("textarea");
      ta.value = text;
      document.body.appendChild(ta);
      ta.select();
      document.execCommand("copy");
      document.body.removeChild(ta);
    }
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  return (
    <div className="mt-2 flex flex-wrap items-center gap-2">
      {stage === "file" ? (
        <>
          <button
            onClick={() => void api.publishFileSelected(draftId)}
            className="rounded border border-accent bg-accent px-3 py-1 text-xs font-medium text-ink hover:brightness-110"
          >
            已选择
          </button>
          <span className="text-[11px] text-mute">
            选好文件后点击，程序将自动填写标题/文案/话题
          </span>
        </>
      ) : stage === "form_ready" ? (
        <>
          <button
            onClick={() => void api.publishConfirmed(draftId)}
            className="rounded border border-accent bg-accent px-3 py-1 text-xs font-medium text-ink hover:brightness-110"
          >
            我已发布
          </button>
          <span className="text-[11px] text-mute">
            确认已在浏览器中点击【发布】后点击
          </span>
        </>
      ) : (
        <button
          onClick={() => void resume(task.id)}
          className="rounded border border-ok/50 px-3 py-1 text-xs text-ok hover:bg-ok/10"
        >
          继续
        </button>
      )}
      <button
        onClick={() => void cancel(task.id)}
        className="rounded border border-bad/50 px-3 py-1 text-xs text-bad hover:bg-bad/10"
      >
        取消任务
      </button>
      {stage === "file" && typeof task.payload.path === "string" && (
        <div className="mt-1 w-full rounded border border-line bg-ink/80 p-2">
          <div className="text-[10px] uppercase tracking-wider text-mute">
            视频路径（可复制后在文件选择弹窗中粘贴）
          </div>
          <div className="mt-1 flex items-center gap-2">
            <code className="min-w-0 flex-1 truncate text-xs text-paper/85">
              {task.payload.path}
            </code>
            <button
              onClick={() => void copyPath(String(task.payload.path))}
              className="shrink-0 rounded border border-line px-2 py-0.5 text-[11px] text-mute hover:border-accent/50 hover:text-paper"
            >
              {copied ? "已复制" : "复制"}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

export function ApprovalDialog() {
  const tasks = useTaskStore((s) => s.tasks);
  const resume = useTaskStore((s) => s.resume);
  const cancel = useTaskStore((s) => s.cancel);
  const [drafts, setDrafts] = useState<PublishDraft[]>([]);
  const waiting = tasks.filter((t) => t.status === "WAITING_FOR_HUMAN");

  useEffect(() => {
    if (waiting.some((t) => t.agent_name === "publisher")) {
      void api.listDrafts().then(setDrafts).catch(() => {});
    }
  }, [waiting.length]); // eslint-disable-line react-hooks/exhaustive-deps

  if (waiting.length === 0) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm">
      <div className="w-[560px] rounded-xl border border-accent/40 bg-panel p-6 shadow-2xl">
        <h2 className="text-base font-bold text-paper">需要人工确认</h2>
        <p className="mt-1 text-xs leading-relaxed text-mute">
          以下任务已执行到人工确认节点。请核对内容后再操作——最终发布类操作不会被自动执行。
        </p>
        <ul className="mt-4 max-h-[60vh] space-y-2 overflow-y-auto">
          {waiting.map((t) => {
            const draft = drafts.find((d) => d.id === t.payload.draft_id);
            const isPub = t.agent_name === "publisher";
            return (
              <li key={t.id} className="rounded-lg border border-line bg-ink/60 p-3">
                <div className="flex items-center gap-2 text-sm">
                  <span className="text-paper/85">
                    #{t.id} · {isPub ? "抖音发布" : t.agent_name} · {t.task_type}
                  </span>
                  <span className="text-mute">
                    {t.human_required ? "（发布需人工确认）" : ""}
                  </span>
                </div>
                {isPub && typeof t.payload.message === "string" && (
                  <p className="mt-2 rounded border border-warn/30 bg-warn/5 p-2 text-xs leading-relaxed text-paper/85">
                    {t.payload.message}
                  </p>
                )}
                {isPub && <DraftSummary draft={draft} />}
                {t.error && <p className="mt-1 text-xs text-bad">{t.error}</p>}
                {isPub ? (
                  <PublisherActions task={t} />
                ) : (
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
                )}
              </li>
            );
          })}
        </ul>
      </div>
    </div>
  );
}
