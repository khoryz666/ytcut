/** Text formatting for times and sizes. */

const pad = (n) => String(n).padStart(2, '0');

/** Seconds as "m:ss" or "h:mm:ss". */
export function clock(seconds) {
  const s = Math.round(seconds);
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60);
  return h ? `${h}:${pad(m)}:${pad(s % 60)}` : `${m}:${pad(s % 60)}`;
}

export const megabytes = (bytes) => `${(bytes / 1048576).toFixed(1)} MB`;
