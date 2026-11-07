<script lang="ts">
  import { clock } from './lib/format';

  let {
    duration,
    start = $bindable(0),
    end = $bindable(0),
  }: { duration: number | null; start?: number; end?: number } = $props();

  const max = $derived(Math.floor(duration ?? 0));

  // Start with the whole video each time a new link is looked up.
  $effect(() => {
    end = Math.floor(duration ?? 0);
    start = 0;
  });
</script>

{#if max >= 2}
  <div class="stack">
    <div class="row between">
      <strong>Part to download</strong>
      <span class="hint">
        {start === 0 && end === max
          ? `Whole video · ${clock(max)}`
          : `${clock(start)} – ${clock(end)} · ${clock(end - start)}`}
      </span>
    </div>
    <label>Start
      <input name="start" type="range" min="0" max={end - 1} step="1" bind:value={start}>
    </label>
    <label>End
      <input name="end" type="range" min={start + 1} max={max} step="1" bind:value={end}>
    </label>
  </div>
{/if}
