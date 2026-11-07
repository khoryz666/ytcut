/** Library listing and download jobs, refreshed while the page is open. */
import { writable } from 'svelte/store';
import { api, type Job, type VideoFile } from './api';

export const files = writable<VideoFile[]>([]);
export const filesError = writable('');
export const jobs = writable<Job[]>([]);

let finished = 0;

export async function loadFiles() {
  try {
    files.set(await api.files());
    filesError.set('');
  } catch (e) {
    filesError.set(e instanceof Error ? e.message : String(e));
  }
}

async function pollJobs() {
  void api.alive(); // Tell the server this page is still open.
  try {
    const next = await api.jobs();
    jobs.set(next);
    const now = next.filter((job) => job.status !== 'running').length;
    if (now !== finished) await loadFiles(); // A download finished.
    finished = now;
  } catch {
    // Keep the last list; the next tick tries again.
  }
}

export function startPolling() {
  void pollJobs();
  const timer = setInterval(pollJobs, 1000);
  const bye = () => void api.bye();
  addEventListener('pagehide', bye);
  return () => {
    clearInterval(timer);
    removeEventListener('pagehide', bye);
  };
}
