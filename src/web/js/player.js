/** The video player and its temporary playlist: loop, repeat one, shuffle, reorder. */
import { $, el, button, toggle } from './dom.js';

const video = $('video');
let queue = [], pos = 0, shuffle = false, repeatOne = false, failures = 0;
const listeners = [];

/** Call fn whenever the playlist or the current song changes. */
export const onChange = (fn) => listeners.push(fn);
export const current = () => queue[pos];
export const isQueued = (name) => queue.includes(name);

/** Replace the playlist and start from its first song. */
export function playQueue(names) {
  queue = names;
  play(0);
}

export function addToQueue(name) {
  if (!queue.length) return playQueue([name]);
  queue.push(name);
  showNow();
}

/** Follow a renamed (newName given) or deleted file in the playlist. */
export function updateQueue(name, newName) {
  const i = queue.indexOf(name);
  if (i < 0) return;
  if (newName) {
    queue[i] = newName;
    i === pos ? play(pos, video.currentTime) : showNow();
    return;
  }
  queue.splice(i, 1);
  if (!queue.length) return stop();
  if (i < pos) pos--;
  i === pos ? play(pos % queue.length) : showNow();
}

function play(i, time = 0) {
  pos = i;
  $('#player').hidden = false;
  video.src = '/media/' + encodeURIComponent(queue[pos]);
  video.currentTime = time;
  video.play().catch(() => {});
  showNow();
}

function step(dir) {
  const n = queue.length;
  if (!n) return;
  play(shuffle && n > 1 ? (pos + 1 + Math.floor(Math.random() * (n - 1))) % n : (pos + dir + n) % n);
}

function stop() {
  queue = [];
  video.removeAttribute('src');
  video.load();
  $('#player').hidden = true;
  showNow();
}

function move(from, to) {
  if (!Number.isInteger(from) || from === to) return;
  const playing = queue[pos];
  queue.splice(to, 0, ...queue.splice(from, 1));
  pos = queue.indexOf(playing);
  showNow();
}

/** A playlist row: click to play, drag to reorder, ✕ to remove. */
function queueRow(name, i) {
  const li = el('li', { className: 'song' + (i === pos ? ' playing' : ''), draggable: true },
    el('span', { className: 'size', textContent: i + 1 }),
    button(name, () => play(i), { className: 'name', title: 'Play now' }),
    button('✕', () => updateQueue(name), { title: 'Remove from playlist', ariaLabel: 'Remove from playlist' }),
  );
  li.ondragstart = (e) => e.dataTransfer.setData('text/plain', i);
  li.ondragover = (e) => e.preventDefault();
  li.ondrop = (e) => {
    e.preventDefault();
    move(parseInt(e.dataTransfer.getData('text/plain'), 10), i);
  };
  return li;
}

function showNow() {
  const name = queue[pos], many = queue.length > 1;
  // A lone song, or "repeat one", loops natively: no gap between plays.
  video.loop = repeatOne || !many;
  $('#now').textContent = !name ? ''
    : !many ? `${name} · on repeat`
    : `${name} · ${pos + 1} of ${queue.length} · ${repeatOne ? 'repeating this one' : 'looping'}`;
  $('#now-nav').textContent = name ? `♪ ${name}` : '';
  for (const id of ['#prev', '#next', '#repeat', '#shuffle', '#queue']) $(id).hidden = !many;
  $('#queue').replaceChildren(...queue.map(queueRow));
  listeners.forEach((fn) => fn());
}

video.onended = () => step(1);
video.onplaying = () => { failures = 0; };
video.onerror = () => {
  if (!queue.length) return;
  $('#now').textContent = `Can't play ${queue[pos]}.`;
  // Skip to the next song, but stop once every song in the loop has failed.
  if (queue.length > 1 && ++failures < queue.length) setTimeout(() => step(1), 1500);
};
$('#prev').onclick = () => step(-1);
$('#next').onclick = () => step(1);
$('#stop').onclick = stop;
$('#shuffle').onclick = (e) => { shuffle = toggle(e.currentTarget); };
$('#repeat').onclick = (e) => { repeatOne = toggle(e.currentTarget); showNow(); };
