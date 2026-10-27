/** The player dock: audio by default, video on request, loop, repeat one, shuffle, playlist. */
import { $, el, button, toggle } from './dom.js';
import { clock } from './format.js';

const video = $<HTMLVideoElement>('video');
const dock = $('#dock'), playButton = $('#play'), seek = $<HTMLInputElement>('#seek');
const queueList = $('#queue'), queueButton = $('#show-queue');
const videoPanel = $('#video-panel'), videoButton = $('#show-video');

let queue: string[] = [];
let pos = 0, shuffle = false, repeatOne = false, failures = 0, seeking = false;
const listeners: (() => void)[] = [];

/** Call fn whenever the playlist or the current song changes. */
export const onChange = (fn: () => void) => { listeners.push(fn); };
export const current = (): string | undefined => queue[pos];
export const isQueued = (name: string) => queue.includes(name);

/** Replace the playlist and start from its first song. */
export function playQueue(names: string[]) {
  queue = names;
  play(0);
}

export function addToQueue(name: string) {
  if (!queue.length) return playQueue([name]);
  queue.push(name);
  showNow();
}

/** Follow a renamed (newName given) or deleted file in the playlist. */
export function updateQueue(name: string, newName?: string) {
  const i = queue.indexOf(name);
  if (i < 0) return;
  if (newName) {
    queue[i] = newName;
    if (i === pos) play(pos, video.currentTime);
    else showNow();
    return;
  }
  queue.splice(i, 1);
  if (!queue.length) return stop();
  if (i < pos) pos--;
  if (i === pos) play(pos % queue.length);
  else showNow();
}

function play(i: number, time = 0) {
  pos = i;
  video.src = '/media/' + encodeURIComponent(queue[pos]);
  video.currentTime = time;
  video.play().catch(() => {});
  showNow();
}

function step(dir: number) {
  const n = queue.length;
  if (!n) return;
  play(shuffle && n > 1 ? (pos + 1 + Math.floor(Math.random() * (n - 1))) % n : (pos + dir + n) % n);
}

function stop() {
  queue = [];
  video.removeAttribute('src');
  video.load();
  showNow();
}

function move(from: number, to: number) {
  if (!Number.isInteger(from) || from === to) return;
  const playing = queue[pos];
  queue.splice(to, 0, ...queue.splice(from, 1));
  pos = queue.indexOf(playing);
  showNow();
}

/** A playlist row: click to play, drag to reorder, ✕ to remove. */
function queueRow(name: string, i: number) {
  const li = el('li', { className: 'song' + (i === pos ? ' playing' : ''), draggable: true },
    el('span', { className: 'size', textContent: String(i + 1) }),
    button(name, () => play(i), { className: 'name', title: 'Play now' }),
    button('✕', () => updateQueue(name), { title: 'Remove from playlist', ariaLabel: 'Remove from playlist' }),
  );
  li.ondragstart = (e) => e.dataTransfer?.setData('text/plain', String(i));
  li.ondragover = (e) => e.preventDefault();
  li.ondrop = (e) => {
    e.preventDefault();
    move(parseInt(e.dataTransfer?.getData('text/plain') ?? '', 10), i);
  };
  return li;
}

function showNow() {
  const name = current(), many = queue.length > 1;
  dock.hidden = !name;
  document.body.classList.toggle('docked', !!name);
  // A lone song, or "repeat one", loops natively: no gap between plays.
  video.loop = repeatOne || !many;
  $('#now').textContent = name ?? '';
  $('#now-pos').textContent = !many ? 'On repeat'
    : `${pos + 1} of ${queue.length}${repeatOne ? ' · repeating' : ''}`;
  for (const id of ['#prev', '#next', '#repeat', '#shuffle', '#show-queue']) $(id).hidden = !many;
  queueList.hidden = !many || queueButton.getAttribute('aria-pressed') !== 'true';
  queueList.replaceChildren(...queue.map(queueRow));
  listeners.forEach((fn) => fn());
}

function togglePlay() {
  if (video.paused) video.play().catch(() => {});
  else video.pause();
}

function showPlaying() {
  const label = video.paused ? 'Play' : 'Pause';
  playButton.textContent = video.paused ? '▶' : '⏸';
  playButton.title = label;
  playButton.ariaLabel = label;
}

video.onplay = showPlaying;
video.onpause = showPlaying;
video.onclick = togglePlay;
video.onended = () => step(1);
video.onplaying = () => { failures = 0; };
video.onerror = () => {
  if (!queue.length) return;
  $('#now-pos').textContent = "Can't play this file";
  // Skip to the next song, but stop once every song in the loop has failed.
  if (queue.length > 1 && ++failures < queue.length) setTimeout(() => step(1), 1500);
};
video.onloadedmetadata = () => {
  seek.max = String(video.duration || 0);
  $('#duration').textContent = clock(video.duration || 0);
};
video.ontimeupdate = () => {
  if (!seeking) seek.value = String(video.currentTime);
  $('#time').textContent = clock(seeking ? +seek.value : video.currentTime);
};
seek.oninput = () => {
  seeking = true;
  $('#time').textContent = clock(+seek.value);
};
seek.onchange = () => {
  video.currentTime = +seek.value;
  seeking = false;
};

playButton.onclick = togglePlay;
$('#prev').onclick = () => step(-1);
$('#next').onclick = () => step(1);
$('#stop').onclick = stop;
$('#shuffle').onclick = () => { shuffle = toggle($('#shuffle')); };
$('#repeat').onclick = () => { repeatOne = toggle($('#repeat')); showNow(); };
queueButton.onclick = () => { toggle(queueButton); showNow(); };
videoButton.onclick = () => {
  videoPanel.hidden = !toggle(videoButton);
  const label = videoPanel.hidden ? 'Show video' : 'Hide video';
  videoButton.title = label;
  videoButton.ariaLabel = label;
};
