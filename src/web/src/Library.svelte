<script lang="ts">
  import { onMount } from 'svelte';
  import { api, errorMessage } from './lib/api';
  import ErrorText from './ErrorText.svelte';
  import { files, filesError, loadFiles } from './lib/library';
  import { megabytes } from './lib/format';
  import {
    addToQueue,
    current,
    playNow,
    playQueue,
    queued,
    updateQueue,
  } from './lib/player';

  let search = $state('');
  let error = $state('');

  const visible = $derived(
    $files.filter((file) => file.name.toLowerCase().includes(search.trim().toLowerCase())),
  );

  onMount(() => void loadFiles());

  function playAll() {
    playQueue(visible.map((file) => file.name));
  }

  /** Run a library action, show its error if any, then refresh the list. */
  async function act(action: () => Promise<void>) {
    try {
      await action();
      error = '';
    } catch (e) {
      error = errorMessage(e);
    }
    await loadFiles();
  }

  function renameSong(name: string) {
    const base = name.replace(/\.[^.]+$/, '');
    const to = prompt('New name:', base);
    if (!to || to.trim() === base) return;
    void act(async () => {
      updateQueue(name, await api.rename(name, to));
    });
  }

  function deleteSong(name: string) {
    if (!confirm(`Delete "${name}"?\nThis cannot be undone.`)) return;
    void act(async () => {
      await api.delete(name);
      updateQueue(name);
    });
  }
</script>

<article class="stack">
  <div class="row between">
    <h2>Library</h2>
    {#if visible.length}
      <button class="mini" onclick={playAll}>
        ▶ Play {search.trim() ? 'results' : 'all'}
      </button>
    {/if}
  </div>

  <input id="search" type="search" placeholder="Search your songs" aria-label="Search"
    bind:value={search}>

  <ul id="songs">
    {#each visible as file (file.name)}
      <li class="song" class:playing={$current === file.name}>
        <button class="name" title={`Play ${file.name}`}
          onclick={() => playNow(file.name)}>{file.name}</button>
        <span class="size">{megabytes(file.size)}</span>
        {#if !$queued.has(file.name)}
          <button class="mini" title="Add to playlist" aria-label="Add to playlist"
            onclick={() => addToQueue(file.name)}>➕</button>
        {/if}
        <button class="mini" title="Rename" aria-label="Rename"
          onclick={() => renameSong(file.name)}>✎</button>
        <button class="mini danger" title="Delete" aria-label="Delete"
          onclick={() => deleteSong(file.name)}>🗑</button>
      </li>
    {/each}
  </ul>

  {#if !$files.length}
    <p class="hint">No songs yet. Add one above.</p>
  {:else if !visible.length}
    <p class="hint">No songs match your search.</p>
  {/if}
  {#if $filesError}
    <p class="error">{$filesError}</p>
  {/if}
  {#if error}
    <p class="error"><ErrorText message={error} /></p>
  {/if}
</article>
