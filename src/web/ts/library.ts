/**
 * The library list: search, select, play, add to the playlist, and rename or delete via ⋮.
 * While the playlist has songs, the list folds away behind a tray toggle.
 */
import { $, el, button } from './dom.js';
import { megabytes } from './format.js';
import { request, errorMessage, type VideoFile } from './api.js';
import * as player from './player.js';

const search = $<HTMLInputElement>('#search'), all = $<HTMLInputElement>('#all');
const playSelected = $<HTMLButtonElement>('#play-selected'), error = $('#lib-error');
const tray = $('#library-toggle'), body = $('#library-body');
/** Room the ⋮ menu needs below its button, in pixels. */
const MENU_HEIGHT = 110;
let files: VideoFile[] = [];
let selected = new Set<string>();
/** The library list is open even though the playlist has songs. */
let trayOpen = false, hadPlaylist = false;

export async function loadFiles() {
  try {
    files = await request<VideoFile[]>('/files');
  } catch (e) {
    error.textContent = errorMessage(e);
  }
  const names = new Set(files.map((f) => f.name));
  selected = new Set([...selected].filter((n) => names.has(n)));
  render();
}

function visibleFiles() {
  const q = search.value.trim().toLowerCase();
  return files.filter((f) => f.name.toLowerCase().includes(q));
}

function render() {
  // Fold the list away when a playlist starts; the user opens it when they want more songs.
  const hasPlaylist = !!player.current();
  if (hasPlaylist && !hadPlaylist) trayOpen = false;
  hadPlaylist = hasPlaylist;
  tray.hidden = !hasPlaylist;
  tray.setAttribute('aria-expanded', String(trayOpen));
  body.hidden = hasPlaylist && !trayOpen;
  $('#library-count').textContent = `${files.length} songs`;

  const list = visibleFiles();
  $('#songs').replaceChildren(...list.map(songRow));
  $('#empty').textContent = !files.length ? 'No songs yet. Download one first.'
    : !list.length ? 'No songs match your search.' : '';
  all.checked = list.length > 0 && list.every((f) => selected.has(f.name));
  $('.select-all').hidden = !list.length;
  playSelected.disabled = !selected.size;
  playSelected.textContent = `▶ Play selected (${selected.size})`;
}

function songRow(f: VideoFile) {
  const box = el('input', { type: 'checkbox', checked: selected.has(f.name), title: 'Select' });
  box.onchange = () => {
    if (box.checked) selected.add(f.name);
    else selected.delete(f.name);
    render();
  };
  const add = player.isQueued(f.name) ? ''
    : button('➕', () => player.addToQueue(f.name), { title: 'Add to playlist', ariaLabel: 'Add to playlist' });
  return el('li', { className: 'song' + (player.current() === f.name ? ' playing' : '') },
    box,
    button(f.name, () => player.playNow(f.name), { className: 'name', title: 'Play now' }),
    el('span', { className: 'size', textContent: megabytes(f.size) }),
    add,
    menu(f.name),
  );
}

/** The ⋮ menu with Rename and Delete. */
function menu(name: string) {
  const details = el('details', { className: 'menu' },
    el('summary', { textContent: '⋮', title: 'More', ariaLabel: 'More options' }));
  const item = (text: string, action: (name: string) => void, props?: Partial<HTMLButtonElement>) =>
    button(text, () => { details.open = false; action(name); }, props);
  details.append(el('div', { className: 'menu-items' },
    item('Rename', renameSong),
    item('Delete', deleteSong, { className: 'danger' })));
  // Open upward when the menu would end up under the player dock or off screen.
  details.ontoggle = () => {
    if (!details.open) return;
    const dock = $('#dock');
    const floor = dock.hidden ? innerHeight : dock.getBoundingClientRect().top;
    details.classList.toggle('up', floor - details.getBoundingClientRect().bottom < MENU_HEIGHT);
  };
  return details;
}

/** Run a library action, show its error if any, then refresh the list. */
async function act(action: () => Promise<void>) {
  try {
    await action();
    error.textContent = '';
  } catch (e) {
    error.textContent = errorMessage(e);
  }
  await loadFiles();
}

function renameSong(name: string) {
  const base = name.replace(/\.[^.]+$/, '');
  const to = prompt('New name:', base);
  if (!to || to.trim() === base) return;
  act(async () => {
    const newName = await request<string>('/rename', { name, to });
    if (selected.delete(name)) selected.add(newName);
    player.updateQueue(name, newName);
  });
}

function deleteSong(name: string) {
  if (!confirm(`Delete "${name}"?\nThis cannot be undone.`)) return;
  act(async () => {
    await request<string>('/delete', { name });
    selected.delete(name);
    player.updateQueue(name);
  });
}

tray.onclick = () => {
  trayOpen = !trayOpen;
  render();
};
search.oninput = render;
all.onchange = () => {
  for (const f of visibleFiles()) {
    if (all.checked) selected.add(f.name);
    else selected.delete(f.name);
  }
  render();
};
playSelected.onclick = () => player.playQueue(files.map((f) => f.name).filter((n) => selected.has(n)));
player.onChange(render);
// Clicking anywhere else closes an open ⋮ menu.
document.addEventListener('click', (e) => {
  for (const open of document.querySelectorAll<HTMLDetailsElement>('details.menu[open]')) {
    if (!open.contains(e.target as Node)) open.open = false;
  }
});
