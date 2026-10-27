/** The two-handle slider that picks which part of the video to download. */
import { $ } from './dom.js';
import { clock } from './format.js';

const box = $('#cut'), track = $('.range'), label = $('#cut-label');
const start = $<HTMLInputElement>('#cut [name=start]'), end = $<HTMLInputElement>('#cut [name=end]');

/** Show the slider for a video of this many seconds, or hide it when unknown. */
export function showCut(duration: number | null) {
  const max = Math.floor(duration ?? 0);
  box.hidden = max < 2;
  for (const input of [start, end]) input.max = String(max);
  start.value = '0';
  end.value = String(max);
  draw();
}

/** Drop untouched ends from the form data: no cut also skips re-encoding. */
export function trimCut(data: FormData) {
  if (box.hidden || +start.value === 0) data.delete('start');
  if (box.hidden || end.value === end.max) data.delete('end');
}

/** Keep start before end, then update the label and the filled part of the track. */
function draw(moved?: HTMLInputElement) {
  const max = +end.max;
  if (+start.value >= +end.value) {
    if (moved === start) start.value = String(+end.value - 1);
    else end.value = String(+start.value + 1);
  }
  const a = +start.value, b = +end.value;
  label.textContent = a === 0 && b === max
    ? `Whole video · ${clock(max)}`
    : `${clock(a)} – ${clock(b)} · ${clock(b - a)}`;
  track.style.setProperty('--a', String(max && a / max));
  track.style.setProperty('--b', String(max && b / max));
  // Both handles can meet; keep the one that can still move on top.
  start.style.zIndex = a > max / 2 ? '1' : '';
}

start.oninput = () => draw(start);
end.oninput = () => draw(end);
