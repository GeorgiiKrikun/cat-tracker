<script lang="ts">
  import { store } from '../lib/store.svelte';

  let username = $state('');
  let password = $state('');
  let error = $state('');
  let busy = $state(false);

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    busy = true;
    error = '';
    try {
      await store.login(username, password);
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    } finally {
      busy = false;
    }
  }
</script>

<div class="wrap">
  <img src="/icons/icon-192.png" alt="" width="88" height="88" />
  <h1>Cat tracker</h1>
  <form class="card" onsubmit={submit}>
    <label for="u">Username</label>
    <input id="u" bind:value={username} autocomplete="username" autocapitalize="none" required />
    <label for="p">Password</label>
    <input id="p" type="password" bind:value={password} autocomplete="current-password" required />
    {#if error}<p class="error">{error}</p>{/if}
    <button class="btn primary block" disabled={busy}>{busy ? 'Logging in…' : 'Log in'}</button>
  </form>
  <p class="muted small">You stay logged in on this device for 90 days.</p>
</div>

<style>
  .wrap { max-width: 380px; margin: 0 auto; padding: calc(40px + var(--safe-t)) 18px 24px; text-align: center; }
  img { border-radius: 22px; }
  h1 { margin: 12px 0 20px; }
  form { padding: 4px 18px 18px; text-align: left; }
  .btn { margin-top: 20px; }
  .error { color: var(--bad); margin: 12px 0 0; }
</style>
