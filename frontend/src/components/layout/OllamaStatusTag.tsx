import { useCallback, useEffect, useRef, useState } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import { api } from "../../api/client";

type Status = "checking" | "up" | "down" | "starting";

const STYLE: Record<Status, string> = {
  checking: "border-line text-mute",
  up: "border-emerald-500/40 bg-emerald-500/10 text-emerald-400",
  down: "border-bad/40 bg-bad/10 text-bad hover:bg-bad/20",
  starting: "border-amber-500/40 bg-amber-500/10 text-amber-400 animate-pulse",
};

const LABEL: Record<Status, string> = {
  checking: "Ollama 检测中…",
  up: "Ollama 已启动",
  down: "Ollama 未启动",
  starting: "Ollama 启动中…",
};

/** 顶栏 Ollama 状态标签：未启动时点击询问并后台拉起服务。 */
export function OllamaStatusTag() {
  const [status, setStatus] = useState<Status>("checking");
  const startingRef = useRef(false);

  const check = useCallback(async () => {
    if (startingRef.current) return;
    const up = await api.checkOllama().catch(() => false);
    setStatus(up ? "up" : "down");
  }, []);

  useEffect(() => {
    void check();
    const t = setInterval(() => void check(), 5000);
    return () => clearInterval(t);
  }, [check]);

  const handleClick = async () => {
    if (status !== "down") return;
    const ok = await ask("Ollama 未运行，是否启动？\nAI 分析与文案生成都依赖它。", {
      title: "启动 Ollama",
      kind: "warning",
    });
    if (!ok) return;
    startingRef.current = true;
    setStatus("starting");
    try {
      await api.startOllama();
    } catch {
      startingRef.current = false;
      setStatus("down");
      return;
    }
    // 轮询直至服务就绪（最长 30s）。
    for (let i = 0; i < 30; i += 1) {
      await new Promise((r) => setTimeout(r, 1000));
      if (await api.checkOllama().catch(() => false)) {
        startingRef.current = false;
        setStatus("up");
        return;
      }
    }
    startingRef.current = false;
    setStatus("down");
  };

  return (
    <button
      onClick={() => void handleClick()}
      disabled={status !== "down"}
      title={
        status === "down"
          ? "Ollama 未运行，点击启动"
          : status === "starting"
            ? "正在等待 Ollama 启动…"
            : "Ollama 服务正常"
      }
      className={`h-9 rounded border px-3 text-sm transition-colors disabled:cursor-default disabled:opacity-100 ${STYLE[status]}`}
    >
      {LABEL[status]}
    </button>
  );
}
