/** The download page: link lookup, file name, part to download, and download progress. */
import { $, el, errorNodes } from './dom.js';
import { request } from './api.js';
import { showCut, trimCut } from './cut.js';
import { loadFiles } from './library.js';

const form = $('#dl-form'), field = form.elements;
let lookupTimer, lookupUrl, jobsTimer, running = 0;

field.url.oninput = () => {
  clearTimeout(lookupTimer);
  lookupTimer = setTimeout(lookup, 400);
};

/** Show the video's title and length for the link, or why it can't be found. */
async function lookup() {
  const url = lookupUrl = field.url.value.trim();
  field.name.placeholder = 'Same as the video title';
  showCut(null);
  $('#info').textContent = url && field.url.validity.valid ? 'Looking up video…' : '';
  if (!$('#info').textContent) return;
  try {
    const info = await request('/info?' + new URLSearchParams({ url }));
    if (url !== lookupUrl) return;
    $('#info').textContent = info.title;
    field.name.placeholder = info.title;
    showCut(info.duration);
  } catch (e) {
    if (url === lookupUrl) $('#info').replaceChildren(...errorNodes(e.message));
  }
}

form.onsubmit = async (e) => {
  e.preventDefault();
  const data = new FormData(form);
  trimCut(data);
  try {
    await request('/download', data);
    form.reset();
    lookup();
    $('#dl-error').textContent = '';
    pollJobs();
  } catch (err) {
    $('#dl-error').textContent = err.message;
  }
};

function jobRow(j) {
  const state = j.status === 'running' ? el('progress', { max: 100, value: j.progress })
    : j.status === 'done' ? el('span', { className: 'hint', textContent: 'Saved ✓' })
    : el('span', { className: 'error' }, ...errorNodes(j.message || 'Download failed'));
  return el('li', {}, el('span', { textContent: (j.status === 'done' && j.message) || j.label }), state);
}

/** Refresh the download list every second while something downloads. */
export async function pollJobs() {
  clearTimeout(jobsTimer);
  const jobs = await request('/jobs');
  $('#jobs').hidden = !jobs.length;
  $('#jobs').replaceChildren(...jobs.map(jobRow));
  const now = jobs.filter((j) => j.status === 'running').length;
  if (now < running) loadFiles();
  running = now;
  if (now) jobsTimer = setTimeout(pollJobs, 1000);
}
