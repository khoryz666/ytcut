<script lang="ts">
  import { onMount } from 'svelte';
  import { clock } from './lib/format';
  import {
    attach,
    current,
    duration,
    message,
    next,
    playing,
    pos,
    prev,
    queue,
    repeatOne,
    seekTo,
    setVideo,
    showVideo,
    shuffle,
    stop,
    time,
    toggle,
    toggleRepeat,
    toggleShuffle,
  } from './lib/player';

  let { onPlaylistPage = false }: { onPlaylistPage?: boolean } = $props();
  let audio = $state<HTMLAudioElement>();
  let video = $state<HTMLVideoElement>();
  let seeking = $state(false);
  let seekValue = $state(0);

  onMount(() => {
    if (!audio || !video) return;
    return attach(audio, video);
  });

  const many = $derived($queue.length > 1);
  const shownTime = $derived(seeking ? seekValue : $time);
  const nowPos = $derived(
    !many
      ? 'On repeat'
      : `${$pos + 1} of ${$queue.length}${$repeatOne ? ' · repeating' : ''}`,
  );

  $effect(() => {
    const loop = $repeatOne || !many;
    if (audio) audio.loop = loop;
    if (video) video.loop = loop;
  });

  // A new song starts from its own beginning, not the previous seek position.
  $effect(() => {
    $current;
    seeking = false;
    seekValue = 0;
  });
</script>

<footer id="dock" hidden={!$current}>
  <audio bind:this={audio} preload="auto"></audio>
  <div class="panel" hidden={!$showVideo}>
    <video bind:this={video} playsinline onclick={toggle}></video>
  </div>
  <div class="bar">
    <div class="controls">
      <button class="icon" hidden={!many} title="Previous" aria-label="Previous" onclick={prev}>⏮</button>
      <button class="icon play" title={$playing ? 'Pause' : 'Play'}
        aria-label={$playing ? 'Pause' : 'Play'} onclick={toggle}>
        {$playing ? '⏸' : '▶'}
      </button>
      <button class="icon" hidden={!many} title="Next" aria-label="Next" onclick={next}>⏭</button>
    </div>

    <div class="now-info">
      <div class="row between">
        <strong>{$current ?? ''}</strong>
        <span class="hint">{$message || nowPos}</span>
      </div>
      <div class="seek">
        <span class="hint">{clock(shownTime)}</span>
        <input type="range" min="0" max={$duration} step="0.1" value={shownTime}
          aria-label="Seek"
          oninput={(e) => {
            seeking = true;
            seekValue = Number(e.currentTarget.value);
          }}
          onchange={() => {
            seekTo(seekValue);
            seeking = false;
          }}>
        <span class="hint">{clock($duration)}</span>
      </div>
    </div>

    <div class="controls">
      <button class="icon" hidden={!many} aria-pressed={$repeatOne}
        title="Repeat this song" aria-label="Repeat this song" onclick={toggleRepeat}>🔂</button>
      <button class="icon" hidden={!many} aria-pressed={$shuffle}
        title="Shuffle" aria-label="Shuffle" onclick={toggleShuffle}>🔀</button>
      <a class="icon" hidden={!$current || onPlaylistPage} href="#playlist"
        title="Playlist" aria-label="Playlist">☰</a>
      <button class="icon" aria-pressed={$showVideo}
        title={$showVideo ? 'Hide video' : 'Show video'}
        aria-label={$showVideo ? 'Hide video' : 'Show video'}
        onclick={() => setVideo(!$showVideo)}>🎬</button>
      <button class="icon" title="Stop" aria-label="Stop" onclick={stop}>✕</button>
    </div>
  </div>
</footer>
