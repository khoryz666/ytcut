<script lang="ts">
  import { clock } from './lib/format';

  let {
    duration,
    start = $bindable(0),
    end = $bindable(0),
  }: { duration: number | null; start?: number; end?: number } = $props();

  const max = $derived(Math.floor(duration ?? 0));
  const visible = $derived(max >= 2);
  const label = $derived(
    start === 0 && end === max
      ? `Whole video · ${clock(max)}`
      : `${clock(start)} – ${clock(end)} · ${clock(end - start)}`,
  );
  const a = $derived(max ? start / max : 0);
  const b = $derived(max ? end / max : 1);

  // Start with the whole video each time a new link is looked up.
  $effect(() => {
    const limit = Math.floor(duration ?? 0);
    start = 0;
    end = limit;
  });

  /** Keep the handles from crossing. */
  function move(moved: 'start' | 'end') {
    if (start < end) return;
    if (moved === 'start') start = Math.max(0, end - 1);
    else end = Math.min(max, start + 1);
  }
</script>

{#if visible}
  <div class="cut">
    <div class="row between">
      <span class="label">Part to download</span>
      <span class="hint">{label}</span>
    </div>
    <div class="range" style={`--a:${a};--b:${b}`}>
      <div class="fill"></div>
      <input name="start" type="range" min="0" max={max} step="1" bind:value={start}
        oninput={() => move('start')} aria-label="Start">
      <input name="end" type="range" min="0" max={max} step="1" bind:value={end}
        oninput={() => move('end')} aria-label="End">
    </div>
  </div>
{/if}
