<script lang="ts">
  import { onMount } from 'svelte';
  import { api, errorMessage, type VideoInfo } from './lib/api';
  import { clock } from './lib/format';
  import { jobs, pollJobs } from './lib/jobs';
  import Cut from './Cut.svelte';
  import ErrorText from './ErrorText.svelte';

  let url = $state('');
  let name = $state('');
  let info = $state<VideoInfo | null>(null);
  let looking = $state(false);
  let lookupError = $state('');
  let formError = $state('');
  let start = $state(0);
  let end = $state(0);
  let lookupTimer: ReturnType<typeof setTimeout> | undefined;
  let lookupUrl = '';

  $effect(() => {
    const link = url.trim();
    clearTimeout(lookupTimer);
    if (!link) {
      info = null;
      lookupError = '';
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
    lookupError = '';
    looking = true;
    try {
      const video = await api.info(link);
      if (link === lookupUrl) info = video;
    } catch (e) {
      if (link === lookupUrl) lookupError = errorMessage(e);
    } finally {
      if (link === lookupUrl) looking = false;
    }
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    formError = '';
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
      await pollJobs();
    } catch (e) {
      formError = errorMessage(e);
    }
  }

  onMount(() => {
    void pollJobs();
  });
</script>

<section id="download">
  <div class="intro">
    <h1>Save a song</h1>
    <p class="hint">Paste a YouTube link, keep the part you like, and it lands in your Music folder.</p>
  </div>

  <form class="card" onsubmit={submit}>
    <input name="url" type="url" required aria-label="YouTube link"
      placeholder="Paste a YouTube link" bind:value={url}>

    {#if looking}
      <p class="hint">Looking up video…</p>
    {:else if lookupError}
      <p class="error"><ErrorText message={lookupError} /></p>
    {/if}

    {#if info}
      <div class="preview">
        {#if info.thumbnail}
          <img src={info.thumbnail} alt="" referrerpolicy="no-referrer">
        {/if}
        <div>
          <strong>{info.title}</strong>
          {#if info.duration}
            <span class="hint">{clock(info.duration)}</span>
          {/if}
        </div>
      </div>
    {/if}

    <label>File name
      <input name="name" placeholder={info?.title ?? 'Same as the video title'} bind:value={name}>
    </label>

    <Cut duration={info?.duration ?? null} bind:start bind:end />

    <button class="primary" disabled={looking || !url.trim()}>Download</button>
    {#if formError}
      <p class="error"><ErrorText message={formError} /></p>
    {/if}
  </form>

  {#if $jobs.length}
    <ul class="card jobs">
      {#each $jobs as job}
        <li>
          <span>{job.status === 'error' ? job.label : job.message || job.label}</span>
          {#if job.status === 'running'}
            <progress max="100" value={job.progress}></progress>
          {:else if job.status === 'done'}
            <span class="hint">Saved ✓</span>
          {:else if job.status === 'exists'}
            <span class="hint">Already in your library</span>
          {:else}
            <span class="error"><ErrorText message={job.message || 'Download failed'} /></span>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</section>
