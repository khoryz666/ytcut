<script lang="ts">
  let { message }: { message: string } = $props();
</script>

{#if /sign in/i.test(message)}
  YouTube wants you to sign in.
  <a href="https://www.youtube.com/" target="_blank" rel="noreferrer">Sign in to YouTube</a>, then try again.
{:else}
  {message}
{/if}
