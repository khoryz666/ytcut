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
  /** "exists": the file was already there, so nothing was downloaded. */
  status: 'running' | 'done' | 'exists' | 'error';
  progress: number;
  /** The saved file name, or the error. */
  message: string;
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, init);
  if (!response.ok) throw new Error(await response.text());
  const type = response.headers.get('content-type') ?? '';
  return (type.includes('application/json') ? response.json() : response.text()) as Promise<T>;
}

const form = (fields: Record<string, string>) => new URLSearchParams(fields);
const post = (fields: Record<string, string>): RequestInit => ({
  method: 'POST',
  body: form(fields),
});

export const api = {
  files: () => request<VideoFile[]>('/files'),
  jobs: () => request<Job[]>('/jobs'),
  info: (url: string) => request<VideoInfo>('/info?' + new URLSearchParams({ url })),
  download: (fields: Record<string, string>) => request<string>('/download', post(fields)),
  rename: (name: string, to: string) => request<string>('/rename', post({ name, to })),
  delete: (name: string) => request<string>('/delete', post({ name })),
  /** Tell the server this page is still open. */
  alive: () => fetch('/alive', { method: 'POST' }).catch(() => {}),
};

export const errorMessage = (e: unknown) => (e instanceof Error ? e.message : String(e));
