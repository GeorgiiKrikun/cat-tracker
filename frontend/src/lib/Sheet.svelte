<script lang="ts">
  import type { Snippet } from 'svelte';

  let { title, onclose, children }: { title: string; onclose: () => void; children: Snippet } = $props();

  function onkey(e: KeyboardEvent) {
    if (e.key === 'Escape') onclose();
  }
</script>

<svelte:window onkeydown={onkey} />

<div class="backdrop" onclick={onclose} role="presentation"></div>
<div class="sheet" role="dialog" aria-modal="true" aria-label={title}>
  <div class="grab"></div>
  <header>
    <h2>{title}</h2>
    <button class="close" onclick={onclose} aria-label="Close">✕</button>
  </header>
  <div class="body">
    {@render children()}
  </div>
</div>

<style>
  .backdrop {
    position: fixed; inset: 0; background: rgb(0 0 0 / 35%); z-index: 40;
    animation: fade 0.15s ease-out;
  }
  .sheet {
    position: fixed; left: 0; right: 0; bottom: 0; z-index: 41;
    max-height: 88dvh; overflow-y: auto;
    background: var(--bg); border-radius: 20px 20px 0 0;
    padding: 6px 18px calc(20px + var(--safe-b));
    animation: up 0.2s ease-out;
    max-width: 640px; margin: 0 auto;
  }
  .grab { width: 40px; height: 4px; border-radius: 2px; background: var(--line); margin: 4px auto 8px; }
  header { display: flex; align-items: center; justify-content: space-between; }
  header h2 { margin: 6px 0; font-size: 19px; }
  .close { width: 40px; height: 40px; border-radius: 50%; font-size: 18px; color: var(--muted); }
  @keyframes up { from { transform: translateY(100%); } }
  @keyframes fade { from { opacity: 0; } }
</style>
