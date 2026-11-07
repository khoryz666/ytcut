<script lang="ts">
  import { onMount } from 'svelte';
  import { startPolling } from './lib/library';
  import Download from './Download.svelte';
  import Library from './Library.svelte';
  import Playlist from './Playlist.svelte';
  import Player from './Player.svelte';

  onMount(startPolling);
</script>

<header>
  <nav class="container">
    <a class="brand" href="/">
      <img src="/logo.svg" alt="" width="30" height="30">
      <span>ytcut</span>
    </a>
  </nav>
</header>

<main class="container">
  <div class="grid">
    <div class="stack">
      <Download />
      <Library />
    </div>
    <Playlist />
  </div>
</main>

<Player />
