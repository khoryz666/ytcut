/** Talking to the ytcut server, and the shapes of its answers. */

export interface VideoFile {
  name: string;
  size: number;
}

export interface VideoInfo {
  title: string;
  duration: number | null;
  thumbnail: string | null;
}

export interface Job {
  /** The chosen file name, or the link when the video title is used. */
  label: string;
  status: 'running' | 'done' | 'error';
  progress: number;
  /** The saved file name, or the error. */
  message: string;
}

type Fields = FormData | Record<string, string>;

/** GET when no fields are given, POST them otherwise. Throws the server's message on error. */
export async function request<T>(path: string, fields?: Fields): Promise<T> {
  const init = fields && { method: 'POST', body: new URLSearchParams(pairs(fields)) };
  const res = await fetch(path, init);
  if (!res.ok) throw new Error(await res.text());
  const isJson = res.headers.get('content-type')?.startsWith('application/json');
  return (isJson ? res.json() : res.text()) as Promise<T>;
}

export const errorMessage = (e: unknown) => (e instanceof Error ? e.message : String(e));

function pairs(fields: Fields): string[][] {
  if (fields instanceof FormData) return [...fields].map(([k, v]) => [k, String(v)]);
  return Object.entries(fields);
}
