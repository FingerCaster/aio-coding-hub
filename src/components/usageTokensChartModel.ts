export function buildUsageTokensXAxisTicks(labels: string[]) {
  if (labels.length <= 7) return labels;

  const interval = Math.max(1, Math.ceil((labels.length - 1) / 6));
  const ticks = labels.filter((_, i) => i % interval === 0);
  const last = labels[labels.length - 1];

  if (last && ticks[ticks.length - 1] !== last) {
    ticks.push(last);
  }

  return ticks;
}
