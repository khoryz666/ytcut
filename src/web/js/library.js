/** The library list: search, select, play, add to the playlist, and rename or delete via ⋮. */
import { $, el, button } from './dom.js';
import { megabytes } from './format.js';
import { request } from './api.js';
import * as player from './player.js';

let files = [], selected = new Set();

export async function loadFiles() {
  try {
    files = await request('/files');
  } catch (e) {
    $('#lib-error').textContent = e.message;
  }
  const names = new Set(files.map((f) => f.name));
  selected = new Set([...selected].filter((n) => names.has(n)));
  render();
}

function visibleFiles() {
  const q = $('#search').value.trim().toLowerCase();
  return files.filter((f) => f.name.toLowerCase().includes(q));
}

function render() {
  const list = visibleFiles();
  $('#songs').replaceChildren(...list.map(songRow));
  $('#empty').textContent = !files.length ? 'No videos yet. Download one first.'
    : !list.length ? 'No videos match your search.' : '';
  $('#all').checked = list.length > 0 && list.every((f) => selected.has(f.name));
  $('#all').parentElement.hidden = !list.length;
  $('#play-selected').disabled = !selected.size;
  $('#play-selected').textContent = `▶ Play selected (${selected.size})`;
}

function songRow(f) {
  const box = el('input', { type: 'checkbox', checked: selected.has(f.name), title: 'Select' });
  box.onchange = () => {
    box.checked ? selected.add(f.name) : selected.delete(f.name);
    render();
  };
  return el('li', { className: 'song' + (player.current() === f.name ? ' playing' : '') },
    box,
    button(f.name, () => player.playQueue([f.name]), { className: 'name', title: 'Play on repeat' }),
    el('span', { className: 'size', textContent: megabytes(f.size) }),
    player.isQueued(f.name) ? ''
      : button('➕', () => player.addToQueue(f.name), { title: 'Add to playlist', ariaLabel: 'Add to playlist' }),
    menu(f.name),
  );
}

/** The ⋮ menu with Rename and Delete. */
function menu(name) {
  const details = el('details', { className: 'menu' },
    el('summary', { textContent: '⋮', title: 'More', ariaLabel: 'More options' }));
  const item = (text, action, props) => button(text, () => { details.open = false; action(name); }, props);
  details.append(el('div', { className: 'menu-items' },
    item('Rename', renameSong),
    item('Delete', deleteSong, { className: 'danger' })));
  return details;
}

/** Run a library action, show its error if any, then refresh the list. */
async function act(fn) {
  try {
    await fn();
    $('#lib-error').textContent = '';
  } catch (e) {
    $('#lib-error').textContent = e.message;
  }
  await loadFiles();
}

function renameSong(name) {
  const base = name.replace(/\.[^.]+$/, '');
  const to = prompt('New name:', base);
  if (!to || to.trim() === base) return;
  act(async () => {
    const newName = await request('/rename', { name, to });
    if (selected.delete(name)) selected.add(newName);
    player.updateQueue(name, newName);
  });
}

function deleteSong(name) {
  if (!confirm(`Delete "${name}"?\nThis cannot be undone.`)) return;
  act(async () => {
    await request('/delete', { name });
    selected.delete(name);
    player.updateQueue(name);
  });
}

$('#search').oninput = render;
$('#all').onchange = (e) => {
  for (const f of visibleFiles()) e.target.checked ? selected.add(f.name) : selected.delete(f.name);
  render();
};
$('#play-selected').onclick = () => player.playQueue(files.map((f) => f.name).filter((n) => selected.has(n)));
player.onChange(render);
// Clicking anywhere else closes an open ⋮ menu.
document.addEventListener('click', (e) => {
  for (const open of document.querySelectorAll('details.menu[open]')) {
    if (!open.contains(e.target)) open.open = false;
  }
});
