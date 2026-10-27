/** The player dock: audio by default, video on request, loop, repeat one, shuffle, playlist. */
import { $, el, button, toggle } from './dom.js';
import { clock } from './format.js';

/** Times a song is reloaded after an error before giving up on it. */
const MAX_RETRIES = 2;

const audio = $<HTMLAudioElement>('audio'), video = $<HTMLVideoElement>('video');
/** The element playing now: audio, so no video is decoded, unless the video is shown. */
let media: HTMLMediaElement = audio;
const dock = $('#dock'), playButton = $('#play'), seek = $<HTMLInputElement>('#seek');
const queueList = $('#queue'), playlistLink = $('#show-queue');
const videoPanel = $('#video-panel'), videoButton = $('#show-video');

let queue: string[] = [];
let pos = 0, shuffle = false, repeatOne = false, failures = 0, retries = 0, seeking = false;
/** Index of the playlist row being dragged, or -1. */
let dragFrom = -1;
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

/** Play a song now without losing the playlist: jump to it, or insert it after the current one. */
export function playNow(name: string) {
  if (!queue.length) return playQueue([name]);
  const i = queue.indexOf(name);
  if (i >= 0) return play(i);
  queue.splice(pos + 1, 0, name);
  play(pos + 1);
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
    if (i === pos) play(pos, media.currentTime);
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
  retries = 0;
  load(time);
  showNow();
}

/** Start the current song at `time` seconds. */
function load(time: number) {
  media.src = '/media/' + encodeURIComponent(queue[pos]);
  media.currentTime = time;
  media.play().catch(() => {});
}

/** Move playback to the other element at the same spot, keeping play or pause. */
function switchTo(next: HTMLMediaElement) {
  if (next === media) return;
  const from = media, src = from.getAttribute('src'), time = from.currentTime, paused = from.paused;
  media = next;
  from.pause();
  from.removeAttribute('src');
  from.load();
  if (src) {
    next.src = src;
    next.currentTime = time;
    if (!paused) next.play().catch(() => {});
  }
  showNow();
  showPlaying();
}

function step(dir: number) {
  const n = queue.length;
  if (!n) return;
  play(shuffle && n > 1 ? (pos + 1 + Math.floor(Math.random() * (n - 1))) % n : (pos + dir + n) % n);
}

function stop() {
  queue = [];
  media.removeAttribute('src');
  media.load();
  showNow();
}

function move(from: number, to: number) {
  if (!Number.isInteger(from) || from === to) return;
  const playing = queue[pos];
  queue.splice(to, 0, ...queue.splice(from, 1));
  pos = queue.indexOf(playing);
  showNow();
}

/** A playlist row: ⠿ to drag and reorder, click to play, ✕ to remove. */
function queueRow(name: string, i: number) {
  const isCurrent = i === pos, many = queue.length > 1;
  const marker = isCurrent
    ? el('span', { className: 'eq', title: 'Now playing' }, el('i'), el('i'), el('i'))
    : String(i + 1);
  const li = el('li', { className: 'track' + (isCurrent ? ' current' : ''), draggable: many },
    many ? el('span', { className: 'grip', textContent: '⠿', title: 'Drag to reorder' }) : '',
    el('span', { className: 'index' }, marker),
    button(name, () => play(i), { className: 'name', title: isCurrent ? `Now playing: ${name}` : `Play ${name}` }),
    button('✕', () => updateQueue(name), {
      className: 'remove', title: 'Remove from playlist', ariaLabel: 'Remove from playlist',
    }),
  );
  li.ondragstart = (e) => {
    dragFrom = i;
    li.classList.add('dragging');
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = 'move';
      e.dataTransfer.setData('text/plain', name);
    }
  };
  li.ondragend = () => {
    dragFrom = -1;
    clearDropMarks();
  };
  // Show a line above or below this row where the dragged song will land.
  li.ondragover = (e) => {
    if (dragFrom < 0) return;
    e.preventDefault();
    const box = li.getBoundingClientRect(), below = e.clientY > box.top + box.height / 2;
    clearDropMarks();
    li.classList.add(below ? 'drop-after' : 'drop-before');
  };
  li.ondrop = (e) => {
    e.preventDefault();
    const below = li.classList.contains('drop-after');
    clearDropMarks();
    if (dragFrom < 0) return;
    const to = i + (below ? 1 : 0);
    move(dragFrom, dragFrom < to ? to - 1 : to);
  };
  return li;
}

function clearDropMarks() {
  for (const row of queueList.querySelectorAll('.drop-before, .drop-after, .dragging')) {
    row.classList.remove('drop-before', 'drop-after', 'dragging');
  }
}

function showNow() {
  const name = current(), many = queue.length > 1;
  dock.hidden = !name;
  document.body.classList.toggle('docked', !!name);
  // A lone song, or "repeat one", loops natively: no gap between plays.
  media.loop = repeatOne || !many;
  $('#now').textContent = name ?? '';
  $('#now-pos').textContent = !many ? 'On repeat'
    : `${pos + 1} of ${queue.length}${repeatOne ? ' · repeating' : ''}`;
  for (const id of ['#prev', '#next', '#repeat', '#shuffle']) $(id).hidden = !many;
  showPlaylistLink();
  $('#playlist-empty').hidden = !!name;
  queueList.hidden = !name;
  $('#playlist-info').textContent = !name ? '' : !many ? '1 song, on repeat'
    : `${queue.length} songs · drag ⠿ to reorder`;
  queueList.replaceChildren(...queue.map(queueRow));
  listeners.forEach((fn) => fn());
}

/** ☰ leads to the playlist, so it is only needed away from the Playlist page. */
function showPlaylistLink() {
  playlistLink.hidden = !queue.length || location.hash === '#playlist';
}

function togglePlay() {
  if (media.paused) media.play().catch(() => {});
  else media.pause();
}

function showPlaying() {
  const label = media.paused ? 'Play' : 'Pause';
  playButton.textContent = media.paused ? '▶' : '⏸';
  playButton.title = label;
  playButton.ariaLabel = label;
  document.body.classList.toggle('is-playing', !media.paused);
}

/** A song failed to load: retry it a couple of times, then report it and move on. */
function failed() {
  if (!queue.length) return;
  const song = queue[pos], at = media.currentTime;
  console.warn('ytcut: playback error', song, media.error?.code, media.error?.message);
  // Decoders and cold disks sometimes fail on the first try; reload at the same spot.
  if (retries < MAX_RETRIES) {
    retries++;
    setTimeout(() => { if (queue[pos] === song) load(at); }, 400 * retries);
    return;
  }
  $('#now-pos').textContent = "Can't play this file";
  // Skip to the next song, but stop once every song in the loop has failed.
  if (queue.length > 1 && ++failures < queue.length) setTimeout(() => step(1), 1500);
}

/** Listen on both elements, but only react to the one playing now. */
function on(type: keyof HTMLMediaElementEventMap, handler: () => void) {
  for (const element of [audio, video]) {
    element.addEventListener(type, () => { if (element === media) handler(); });
  }
}

on('play', showPlaying);
on('pause', showPlaying);
on('ended', () => step(1));
on('playing', () => { failures = 0; retries = 0; });
on('error', failed);
on('loadedmetadata', () => {
  seek.max = String(media.duration || 0);
  $('#duration').textContent = clock(media.duration || 0);
});
on('timeupdate', () => {
  if (!seeking) seek.value = String(media.currentTime);
  $('#time').textContent = clock(seeking ? +seek.value : media.currentTime);
});
video.onclick = togglePlay;
seek.oninput = () => {
  seeking = true;
  $('#time').textContent = clock(+seek.value);
};
seek.onchange = () => {
  media.currentTime = +seek.value;
  seeking = false;
};

playButton.onclick = togglePlay;
$('#prev').onclick = () => step(-1);
$('#next').onclick = () => step(1);
$('#stop').onclick = stop;
$('#shuffle').onclick = () => { shuffle = toggle($('#shuffle')); };
$('#repeat').onclick = () => { repeatOne = toggle($('#repeat')); showNow(); };
window.addEventListener('hashchange', showPlaylistLink);
videoButton.onclick = () => {
  const show = toggle(videoButton);
  videoPanel.hidden = !show;
  switchTo(show ? video : audio);
  const label = show ? 'Hide video' : 'Show video';
  videoButton.title = label;
  videoButton.ariaLabel = label;
};
