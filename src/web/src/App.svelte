<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from './lib/api';
  import { startPolling } from './lib/jobs';
  import { current } from './lib/player';
  import Download from './Download.svelte';
  import Playlist from './Playlist.svelte';
  import Player from './Player.svelte';

  let tab = $state<'download' | 'playlist'>(
    location.hash === '#playlist' ? 'playlist' : 'download',
  );

  function syncTab() {
    tab = location.hash === '#playlist' ? 'playlist' : 'download';
  }

  onMount(() => {
    addEventListener('hashchange', syncTab);
    const stopPolling = startPolling();
    const heartbeat = setInterval(api.alive, 1000);
    void api.alive();
    return () => {
      removeEventListener('hashchange', syncTab);
      stopPolling();
      clearInterval(heartbeat);
    };
  });

  $effect(() => {
    document.body.classList.toggle('docked', !!$current);
  });
</script>

<header>
  <nav class="container">
    <a class="brand" href="#download">
      <img src="/logo.svg" alt="" width="30" height="30">
      <span>ytcut</span>
    </a>
    <ul class="tabs">
      <li><a href="#download" aria-current={tab === 'download' ? 'page' : undefined}>Download</a></li>
      <li><a href="#playlist" aria-current={tab === 'playlist' ? 'page' : undefined}>Playlist</a></li>
    </ul>
  </nav>
</header>

<main class="container">
  {#if tab === 'download'}
    <Download />
  {:else}
    <Playlist />
  {/if}
</main>

<Player onPlaylistPage={tab === 'playlist'} />
