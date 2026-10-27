/** Entry point: tabs, the server keep-alive, and the first render. */
import { $ } from './dom.js';
import { loadFiles } from './library.js';
import { pollJobs } from './download.js';

function showTab() {
  const tab = location.hash === '#playlist' ? 'playlist' : 'download';
  for (const id of ['download', 'playlist']) $('#' + id).hidden = id !== tab;
  for (const a of document.querySelectorAll<HTMLAnchorElement>('.tabs a')) {
    a.toggleAttribute('aria-current', a.hash === '#' + tab);
  }
  if (tab === 'playlist') loadFiles();
}

window.onhashchange = showTab;
// Tells the server this page is open, so it can stop once every window is closed.
new EventSource('/alive');
showTab();
pollJobs();
