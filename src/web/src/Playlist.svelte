<script lang="ts">
  import { onMount } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import { api, errorMessage } from './lib/api';
  import ErrorText from './ErrorText.svelte';
  import { files, filesError, loadFiles } from './lib/files';
  import { megabytes } from './lib/format';
  import {
    addToQueue,
    current,
    move,
    playing,
    playAt,
    playNow,
    playQueue,
    pos,
    queued,
    queue,
    remove,
    updateQueue,
  } from './lib/player';

  let search = $state('');
  let selected = new SvelteSet<string>();
  let sidebarOpen = $state(false);
  let error = $state('');
  let dragFrom = $state(-1);
  let dropAt = $state(-1);
  let dropAfter = $state(false);

  const visible = $derived(
    $files.filter((file) => file.name.toLowerCase().includes(search.trim().toLowerCase())),
  );
  const allChecked = $derived(visible.length > 0 && visible.every((file) => selected.has(file.name)));
  const selectedCount = $derived($files.filter((file) => selected.has(file.name)).length);
  const playlistInfo = $derived(
    !$current ? '' : $queue.length === 1 ? '1 song, on repeat' : `${$queue.length} songs · drag ⠿ to reorder`,
  );

  onMount(() => {
    void loadFiles();
    const close = (event: MouseEvent) => {
      const sidebar = document.querySelector('.sidebar');
      if (sidebar && !sidebar.contains(event.target as Node)) sidebarOpen = false;
    };
    document.addEventListener('click', close);
    return () => document.removeEventListener('click', close);
  });

  function toggleAll() {
    for (const file of visible) {
      if (allChecked) selected.delete(file.name);
      else selected.add(file.name);
    }
  }

  function playSelected() {
    playQueue($files.map((file) => file.name).filter((name) => selected.has(name)));
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
      const newName = await api.rename(name, to);
      if (selected.delete(name)) selected.add(newName);
      updateQueue(name, newName);
    });
  }

  function deleteSong(name: string) {
    if (!confirm(`Delete "${name}"?\nThis cannot be undone.`)) return;
    void act(async () => {
      await api.delete(name);
      selected.delete(name);
      updateQueue(name);
    });
  }

  /** Open a row's menu upward when it would fall under the sidebar edge or the player. */
  function placeMenu(event: Event) {
    const details = event.currentTarget as HTMLDetailsElement;
    if (!details.open) return;
    const dock = document.querySelector<HTMLElement>('#dock');
    const sidebar = document.querySelector<HTMLElement>('.sidebar');
    const floor = Math.min(
      dock && !dock.hidden ? dock.getBoundingClientRect().top : innerHeight,
      sidebar?.getBoundingClientRect().bottom ?? innerHeight,
    );
    details.classList.toggle('up', floor - details.getBoundingClientRect().bottom < 110);
  }

  function drop() {
    if (dragFrom < 0 || dropAt < 0) return;
    move(dragFrom, dropAt + (dropAfter ? 1 : 0));
    dragFrom = -1;
    dropAt = -1;
  }
</script>

<section id="playlist">
  <!-- The library slides out from the left edge on hover. -->
  <aside class="sidebar" class:open={sidebarOpen} aria-label="Library">
    <div class="sidebar-body">
      <div class="row between">
        <h2>Library</h2>
        <span class="hint">{$files.length ? `${$files.length} songs` : ''}</span>
      </div>
      <input id="search" type="search" placeholder="Search your songs" aria-label="Search"
        bind:value={search}>

      {#if visible.length}
        <div class="row between">
          <label class="select-all">
            <input type="checkbox" checked={allChecked} onchange={toggleAll}> Select all
          </label>
          <button class="primary" disabled={!selectedCount} onclick={playSelected}>
            ▶ Play selected ({selectedCount})
          </button>
        </div>
      {/if}

      <ul id="songs">
        {#each visible as file (file.name)}
          <li class="song" class:playing={$current === file.name}>
            <input type="checkbox" checked={selected.has(file.name)} title="Select"
              onchange={(e) => {
                if (e.currentTarget.checked) selected.add(file.name);
                else selected.delete(file.name);
              }}>
            <button class="name" title={`Play ${file.name}`}
              onclick={() => playNow(file.name)}>{file.name}</button>
            <span class="size">{megabytes(file.size)}</span>
            {#if !$queued.has(file.name)}
              <button class="mini" title="Add to playlist" aria-label="Add to playlist"
                onclick={() => addToQueue(file.name)}>➕</button>
            {/if}
            <details class="menu" ontoggle={placeMenu}>
              <summary title="More" aria-label="More options">⋮</summary>
              <div class="menu-items">
                <button onclick={(e) => {
                  (e.currentTarget.closest('details') as HTMLDetailsElement).open = false;
                  renameSong(file.name);
                }}>Rename</button>
                <button class="danger" onclick={(e) => {
                  (e.currentTarget.closest('details') as HTMLDetailsElement).open = false;
                  deleteSong(file.name);
                }}>Delete</button>
              </div>
            </details>
          </li>
        {/each}
      </ul>

      {#if !$files.length}
        <p class="hint">No songs yet. Download one first.</p>
      {:else if !visible.length}
        <p class="hint">No songs match your search.</p>
      {/if}
      {#if $filesError}
        <p class="error">{$filesError}</p>
      {/if}
      {#if error}
        <p class="error"><ErrorText message={error} /></p>
      {/if}
      <p class="hint">Click a song to play it. ➕ adds it to the playlist.</p>
    </div>
    <button class="sidebar-tab" type="button" aria-label="Library"
      onclick={() => (sidebarOpen = !sidebarOpen)}>
      <span aria-hidden="true">🎵</span>
      <span class="vertical">Library</span>
    </button>
  </aside>

  <div class="card" id="playlist-card">
    <div class="playlist-head">
      <h2>Playlist</h2>
      <p class="hint">{playlistInfo}</p>
    </div>

    {#if !$current}
      <p class="hint">
        Your playlist is empty. Hover <strong>🎵 Library</strong> on the left, then click a song or use ➕.
      </p>
    {:else}
      <ol id="queue">
        {#each $queue as name, i (i)}
          <li class="track"
            class:current={i === $pos}
            class:dragging={i === dragFrom}
            class:drop-before={i === dropAt && !dropAfter}
            class:drop-after={i === dropAt && dropAfter}
            draggable={$queue.length > 1}
            ondragstart={(e) => {
              dragFrom = i;
              if (e.dataTransfer) {
                e.dataTransfer.effectAllowed = 'move';
                e.dataTransfer.setData('text/plain', name);
              }
            }}
            ondragend={() => {
              dragFrom = -1;
              dropAt = -1;
            }}
            ondragover={(e) => {
              if (dragFrom < 0) return;
              e.preventDefault();
              const box = e.currentTarget.getBoundingClientRect();
              dropAt = i;
              dropAfter = e.clientY > box.top + box.height / 2;
            }}
            ondrop={(e) => {
              e.preventDefault();
              drop();
            }}>
            {#if $queue.length > 1}
              <span class="grip" title="Drag to reorder">⠿</span>
            {/if}
            <span class="index">
              {#if i === $pos}
                <span class="eq" class:paused={!$playing} title="Now playing">
                  <i></i><i></i><i></i>
                </span>
              {:else}
                {i + 1}
              {/if}
            </span>
            <button class="name" title={i === $pos ? `Now playing: ${name}` : `Play ${name}`}
              onclick={() => playAt(i)}>{name}</button>
            <button class="remove" title="Remove from playlist" aria-label="Remove from playlist"
              onclick={() => remove(name)}>✕</button>
          </li>
        {/each}
      </ol>
    {/if}
  </div>
</section>
