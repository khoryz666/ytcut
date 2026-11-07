/** Download progress, polled while the page is open. */
import { writable } from 'svelte/store';
import { api, errorMessage, type Job } from './api';
import { loadFiles } from './files';

export const jobs = writable<Job[]>([]);
export const jobsError = writable('');

let running = 0;

export async function pollJobs() {
  try {
    const next = await api.jobs();
    jobs.set(next);
    jobsError.set('');
    const now = next.filter((job) => job.status === 'running').length;
    // A download finished: show its new file in the library.
    if (now < running) await loadFiles();
    running = now;
  } catch (e) {
    jobsError.set(errorMessage(e));
  }
}

export function startPolling() {
  void pollJobs();
  const timer = setInterval(pollJobs, 1000);
  return () => clearInterval(timer);
}
