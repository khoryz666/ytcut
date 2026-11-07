<script lang="ts">
  import { current, move, playAt, pos, queue, remove } from './lib/player';

  const info = $derived(
    !$current ? '' : $queue.length === 1 ? '1 song, on repeat' : `${$queue.length} songs`,
  );
</script>

<article class="stack">
  <div class="row between">
    <h2>Playlist</h2>
    <span class="hint">{info}</span>
  </div>

  {#if !$current}
    <p class="hint">Pick songs from your library. Click one to play it, or ➕ to add it here.</p>
  {:else}
    <ol id="queue">
      {#each $queue as name, i (i)}
        <li class="track" class:current={i === $pos}>
          <span class="index">{i === $pos ? '▶' : i + 1}</span>
          <button class="name" title={i === $pos ? `Now playing: ${name}` : `Play ${name}`}
            onclick={() => playAt(i)}>{name}</button>
          <button class="mini" title="Move up" aria-label="Move up" disabled={i === 0}
            onclick={() => move(i, i - 1)}>↑</button>
          <button class="mini" title="Move down" aria-label="Move down"
            disabled={i === $queue.length - 1} onclick={() => move(i, i + 1)}>↓</button>
          <button class="mini danger" title="Remove from playlist" aria-label="Remove from playlist"
            onclick={() => remove(name)}>✕</button>
        </li>
      {/each}
    </ol>
  {/if}
</article>
