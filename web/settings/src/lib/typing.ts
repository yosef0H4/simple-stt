/** Slider position 100 is Instant; finite speeds occupy positions 0 through 99. */
export function positionToWpm(position: number): number {
  const value = Math.max(0, Math.min(99, Math.round(position)));
  return value <= 50
    ? 50 + value * 8
    : 450 + Math.round((value - 50) * 1200 / 49);
}

export function wpmToPosition(wpm: number): number {
  const value = Math.max(50, Math.min(1650, wpm));
  return value <= 450
    ? Math.round((value - 50) / 8)
    : 50 + Math.round((value - 450) * 49 / 1200);
}
