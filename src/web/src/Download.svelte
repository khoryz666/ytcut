<script lang="ts">
  import { api, errorMessage, type VideoInfo } from './lib/api';
  import { clock } from './lib/format';
  import { jobs } from './lib/library';
  import Cut from './Cut.svelte';
  import ErrorText from './ErrorText.svelte';

  let url = $state('');
  let name = $state('');
  let info = $state<VideoInfo | null>(null);
  let looking = $state(false);
  let error = $state('');
  let start = $state(0);
  let end = $state(0);
  let lookupTimer: ReturnType<typeof setTimeout> | undefined;
  let lookupUrl = '';

  $effect(() => {
    const link = url.trim();
    clearTimeout(lookupTimer);
    if (!link) {
      info = null;
      error = '';
      looking = false;
      return;
    }
    lookupTimer = setTimeout(() => void lookup(link), 400);
    return () => clearTimeout(lookupTimer);
  });

  /** Show the video behind the link, or why it can't be found. */
  async function lookup(link: string) {
    lookupUrl = link;
    info = null;
    error = '';
    looking = true;
    try {
      const video = await api.info(link);
      if (link === lookupUrl) info = video;
    } catch (e) {
      if (link === lookupUrl) error = errorMessage(e);
    } finally {
      if (link === lookupUrl) looking = false;
    }
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    error = '';
    const fields: Record<string, string> = { url, name };
    const max = Math.floor(info?.duration ?? 0);
    if (max >= 2) {
      if (start > 0) fields.start = String(start);
      if (end < max) fields.end = String(end);
    }
    try {
      await api.download(fields);
      url = '';
      name = '';
      info = null;
      start = 0;
      end = 0;
    } catch (e) {
      error = errorMessage(e);
    }
  }
</script>

<article class="stack">
  <div class="row between">
    <h2>Add a song</h2>
    <span class="hint">Saved to your Music folder</span>
  </div>

  <form class="stack" onsubmit={submit}>
    <div class="row">
      <input name="url" type="url" required aria-label="YouTube link"
        placeholder="Paste a YouTube link" bind:value={url}>
      <button class="primary" disabled={looking || !info}>
        {looking ? 'Looking up…' : 'Download'}
      </button>
    </div>

    {#if error}
      <p class="error"><ErrorText message={error} /></p>
    {/if}

    {#if info}
      <div class="preview">
        {#if info.thumbnail}
          <img src={info.thumbnail} alt="" referrerpolicy="no-referrer">
        {/if}
        <div class="stack">
          <strong>{info.title}</strong>
          {#if info.duration}
            <span class="hint">{clock(info.duration)}</span>
          {/if}
        </div>
      </div>

      <label>File name
        <input name="name" placeholder={info.title || 'Video title'} bind:value={name}>
      </label>

      <Cut duration={info.duration} bind:start bind:end />
    {/if}
  </form>

  {#if $jobs.length}
    <ul class="jobs">
      {#each $jobs as job}
        <li>
          {#if job.status === 'running'}
            <span>{job.message || job.label}</span>
            <progress max="100" value={job.progress}></progress>
          {:else if job.status === 'done'}
            <span>{job.message || job.label}</span>
            <span class="hint">Saved ✓</span>
          {:else if job.status === 'exists'}
            <span>{job.message || job.label}</span>
            <span class="hint">Already in your library</span>
          {:else}
            <span>{job.label}</span>
            <span class="error"><ErrorText message={job.message || 'Download failed'} /></span>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</article>
