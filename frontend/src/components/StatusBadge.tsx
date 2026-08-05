import { statusMeta } from "../lib/format";

const TONE_CLASS: Record<string, string> = {
  neutral: "bg-black/80 text-mute border-white/25",
  running: "bg-black/80 text-warn border-warn/50 animate-pulse",
  ready: "bg-black/80 text-ok border-ok/50",
  selected: "bg-black/80 text-accent border-accent/60",
  published: "bg-black/80 text-violet-300 border-violet-400/60",
  warn: "bg-black/80 text-warn border-warn/50",
  danger: "bg-black/80 text-bad border-bad/60",
};

export function StatusBadge({ status }: { status: string }) {
  const meta = statusMeta(status);
  return (
    <span
      className={`inline-flex items-center gap-1 rounded border px-1.5 py-0.5 text-[11px] leading-none ${TONE_CLASS[meta.tone] ?? TONE_CLASS.neutral}`}
    >
      <span className="h-1.5 w-1.5 rounded-full bg-current" />
      {meta.label}
    </span>
  );
}
