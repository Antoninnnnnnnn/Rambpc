/** "9 min 05 s", "45 s", "0 s". */
export function formatRemaining(ms: number): string {
  const total = Math.max(0, Math.ceil(ms / 1000));
  const minutes = Math.floor(total / 60);
  const seconds = total % 60;
  if (minutes === 0) return `${seconds} s`;
  return `${minutes} min ${String(seconds).padStart(2, "0")} s`;
}

/** "0:42", "4:30". */
export function formatClock(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const minutes = Math.floor(total / 60);
  const seconds = total % 60;
  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}

/** Spoken/ISO-ish duration for <time dateTime>: "PT9M5S". */
export function isoDuration(ms: number): string {
  const total = Math.max(0, Math.ceil(ms / 1000));
  return `PT${Math.floor(total / 60)}M${total % 60}S`;
}

/** Five minutes per take (docs/windows/03); warn during the last 30 seconds. */
export const TAKE_LIMIT_MS = 5 * 60 * 1000;
export const TAKE_LIMIT_WARNING_MS = TAKE_LIMIT_MS - 30 * 1000;
