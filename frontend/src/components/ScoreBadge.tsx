export function ScoreBadge({ score }: { score: number | null }) {
  if (score === null || score === undefined) return null;
  const tone =
    score >= 80
      ? "text-ok border-ok/40 bg-emerald-500/10"
      : score >= 60
        ? "text-accent border-accent/40 bg-amber-500/10"
        : "text-mute border-white/10 bg-white/5";
  return (
    <span
      className={`inline-flex items-center rounded border px-1.5 py-0.5 text-[11px] font-semibold tabular-nums ${tone}`}
      title={`AI 质量评分 ${score}/100`}
    >
      {score}
    </span>
  );
}
