/** The download page: link lookup with preview, file name, part to download, and progress. */
import { $, el, errorNodes } from './dom.js';
import { clock } from './format.js';
import { request, errorMessage, type Job, type VideoInfo } from './api.js';
import { showCut, trimCut } from './cut.js';
import { loadFiles } from './library.js';

const form = $<HTMLFormElement>('#dl-form');
const input = (name: string) => form.elements.namedItem(name) as HTMLInputElement;
const url = input('url'), fileName = input('name');
const info = $('#info'), thumb = $<HTMLImageElement>('#thumb'), formError = $('#dl-error');
let lookupTimer: number | undefined, lookupUrl = '', jobsTimer: number | undefined, running = 0;

url.oninput = () => {
  clearTimeout(lookupTimer);
  lookupTimer = setTimeout(lookup, 400);
};

/** Show the video behind the link, or why it can't be found. */
async function lookup() {
  const link = (lookupUrl = url.value.trim());
  showPreview(null);
  info.textContent = link && url.validity.valid ? 'Looking up video…' : '';
  if (!info.textContent) return;
  try {
    const video = await request<VideoInfo>('/info?' + new URLSearchParams({ url: link }));
    if (link !== lookupUrl) return;
    info.textContent = '';
    showPreview(video);
  } catch (e) {
    if (link === lookupUrl) info.replaceChildren(...errorNodes(errorMessage(e)));
  }
}

function showPreview(video: VideoInfo | null) {
  $('#preview').hidden = !video;
  fileName.placeholder = video?.title ?? 'Same as the video title';
  showCut(video?.duration ?? null);
  $('#title').textContent = video?.title ?? '';
  $('#length').textContent = video?.duration ? clock(video.duration) : '';
  thumb.hidden = !video?.thumbnail;
  if (video?.thumbnail) thumb.src = video.thumbnail;
  else thumb.removeAttribute('src');
}

form.onsubmit = async (e) => {
  e.preventDefault();
  const data = new FormData(form);
  trimCut(data);
  try {
    await request<string>('/download', data);
    form.reset();
    lookup();
    formError.textContent = '';
    pollJobs();
  } catch (err) {
    formError.textContent = errorMessage(err);
  }
};

function jobRow(job: Job) {
  const state = job.status === 'running' ? el('progress', { max: 100, value: job.progress })
    : job.status === 'done' ? el('span', { className: 'hint', textContent: 'Saved ✓' })
    : job.status === 'exists' ? el('span', { className: 'hint', textContent: 'Already in your library' })
    : el('span', { className: 'error' }, ...errorNodes(job.message || 'Download failed'));
  const name = (job.status !== 'error' && job.message) || job.label;
  return el('li', {}, el('span', { textContent: name }), state);
}

/** Refresh the download list every second while something downloads. */
export async function pollJobs() {
  clearTimeout(jobsTimer);
  let jobs: Job[];
  try {
    jobs = await request<Job[]>('/jobs');
  } catch {
    return;
  }
  $('#jobs').hidden = !jobs.length;
  $('#jobs').replaceChildren(...jobs.map(jobRow));
  const now = jobs.filter((j) => j.status === 'running').length;
  if (now < running) loadFiles();
  running = now;
  if (now) jobsTimer = setTimeout(pollJobs, 1000);
}
