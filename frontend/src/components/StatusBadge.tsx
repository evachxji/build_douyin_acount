import { statusMeta } from "../lib/format";

const TONE_CLASS: Record<string, string> = {
  neutral: "bg-white/5 text-mute border-white/10",
  running: "bg-amber-500/10 text-warn border-warn/30 animate-pulse",
  ready: "bg-emerald-500/10 text-ok border-ok/30",
  selected: "bg-amber-500/15 text-accent border-accent/40",
  published: "bg-violet-500/10 text-violet-300 border-violet-400/30",
  warn: "bg-orange-500/10 text-warn border-warn/30",
  danger: "bg-red-500/10 text-bad border-bad/40",
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
