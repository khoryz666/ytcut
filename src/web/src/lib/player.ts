/**
 * The player: one queue, one audio element, and an optional video element.
 * Components render the stores; this module owns playback and its recovery.
 */
import { derived, get, writable } from 'svelte/store';

/** Times a song is reloaded after an error before giving up on it. */
const MAX_RETRIES = 2;

export const queue = writable<string[]>([]);
export const pos = writable(0);
export const playing = writable(false);
export const repeatOne = writable(false);
export const shuffle = writable(false);
export const showVideo = writable(false);
export const time = writable(0);
export const duration = writable(0);
export const message = writable('');

export const current = derived([queue, pos], ([$queue, $pos]) => $queue[$pos]);
export const queued = derived(queue, ($queue) => new Set($queue));

let audio: HTMLAudioElement | undefined;
let video: HTMLVideoElement | undefined;
let media: HTMLMediaElement | undefined;
let retries = 0;
let failures = 0;

/** Connect the media elements once the player component is mounted. */
export function attach(a: HTMLAudioElement, v: HTMLVideoElement) {
  audio = a;
  video = v;
  media = get(showVideo) ? v : a;
  const listeners: Array<[HTMLMediaElement, string, EventListener]> = [];
  const listen = (element: HTMLMediaElement, type: string, handle: () => void) => {
    const listener = () => {
      if (element === media) handle();
    };
    element.addEventListener(type, listener);
    listeners.push([element, type, listener]);
  };
  for (const element of [a, v]) {
    listen(element, 'play', () => playing.set(true));
    listen(element, 'pause', () => playing.set(false));
    listen(element, 'ended', next);
    listen(element, 'playing', () => {
      failures = 0;
      retries = 0;
      message.set('');
    });
    listen(element, 'error', failed);
    listen(element, 'loadedmetadata', () => duration.set(media?.duration || 0));
    listen(element, 'timeupdate', () => time.set(media?.currentTime || 0));
  }
  return () => {
    for (const [element, type, listener] of listeners) {
      element.removeEventListener(type, listener);
    }
    media?.pause();
    media = undefined;
  };
}

/** Replace the playlist and start from its first song. */
export function playQueue(names: string[]) {
  if (!names.length) return stop();
  queue.set([...names]);
  playAt(0);
}

/** Play a song now without losing the playlist: jump to it, or insert it after the current one. */
export function playNow(name: string) {
  const names = get(queue);
  if (!names.length) return playQueue([name]);
  const i = names.indexOf(name);
  if (i >= 0) return playAt(i);
  const at = get(pos) + 1;
  names.splice(at, 0, name);
  queue.set(names);
  playAt(at);
}

export function addToQueue(name: string) {
  const names = get(queue);
  if (!names.length) return playQueue([name]);
  queue.set([...names, name]);
}

/** Follow a renamed (newName given) or deleted file in the playlist. */
export function updateQueue(name: string, newName?: string) {
  const names = get(queue);
  const i = names.indexOf(name);
  if (i < 0) return;
  if (!newName) return remove(name);
  names[i] = newName;
  queue.set(names);
  if (i === get(pos)) load(get(time));
}

export function remove(name: string) {
  const names = get(queue);
  const i = names.indexOf(name);
  if (i < 0) return;
  if (names.length === 1) return stop();
  const at = get(pos);
  names.splice(i, 1);
  queue.set(names);
  if (i < at) pos.set(at - 1);
  else if (i === at) playAt(at % names.length);
}

/** Swap a song with its neighbour. */
export function move(i: number, to: number) {
  const names = get(queue);
  if (to < 0 || to >= names.length || i === to) return;
  const playing = names[get(pos)];
  [names[i], names[to]] = [names[to], names[i]];
  queue.set(names);
  pos.set(names.indexOf(playing));
}

export function next() {
  const names = get(queue);
  if (!names.length) return;
  const at = get(pos);
  const target = get(shuffle) && names.length > 1
    ? (at + 1 + Math.floor(Math.random() * (names.length - 1))) % names.length
    : (at + 1) % names.length;
  playAt(target);
}

export function prev() {
  const names = get(queue);
  if (!names.length) return;
  playAt((get(pos) - 1 + names.length) % names.length);
}

export function toggle() {
  if (!media) return;
  if (media.paused) media.play().catch(() => {});
  else media.pause();
}

export function seekTo(seconds: number) {
  if (!media) return;
  media.currentTime = seconds;
  time.set(seconds);
}

export function stop() {
  queue.set([]);
  pos.set(0);
  playing.set(false);
  time.set(0);
  duration.set(0);
  message.set('');
  if (media) {
    media.removeAttribute('src');
    media.load();
  }
}

/** Show the video panel, moving playback between audio and video. */
export function setVideo(show: boolean) {
  showVideo.set(show);
  const from = media;
  const to = show ? video : audio;
  if (!from || !to || from === to) return;
  const src = from.getAttribute('src');
  const at = from.currentTime;
  const paused = from.paused;
  media = to;
  from.pause();
  from.removeAttribute('src');
  from.load();
  if (src) {
    to.src = src;
    to.currentTime = at;
    if (!paused) to.play().catch(() => {});
  }
  time.set(at);
}

/** Jump to a row in the playlist. */
export function playAt(i: number, at = 0) {
  pos.set(i);
  retries = 0;
  load(at);
}

/** Start the current song at `at` seconds. */
function load(at: number) {
  const name = get(current);
  if (!media || !name) return;
  media.src = '/media/' + encodeURIComponent(name);
  media.currentTime = at;
  media.play().catch(() => {});
}

/** A song failed to load: retry it a couple of times, then report it and move on. */
function failed() {
  const name = get(current);
  if (!name || !media) return;
  console.warn('ytcut: playback error', name, media.error?.code, media.error?.message);
  if (retries < MAX_RETRIES) {
    retries++;
    const at = media.currentTime;
    setTimeout(() => {
      if (get(current) === name) load(at);
    }, 400 * retries);
    return;
  }
  message.set("Can't play this file");
  // Skip to the next song, but stop once every song in the loop has failed.
  if (get(queue).length > 1 && ++failures < get(queue).length) {
    setTimeout(next, 1500);
  }
}
