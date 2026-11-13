<script lang="ts">
  import type { Symptom } from '../lib/api';
  import { store } from '../lib/store.svelte';
  import { clock, relTime, startOfDay } from '../lib/time';

  let { onopen, onedit }: { onopen: (s: Symptom) => void; onedit: (id: string) => void } = $props();

  let showMore = $state(false);
  let now = $state(Date.now());
  $effect(() => {
    const t = setInterval(() => (now = Date.now()), 30_000);
    return () => clearInterval(t);
  });

  const instant = (s: Symptom) => s.kind === 'bool' || s.kind === 'count' || s.kind === 'med';

  function tap(s: Symptom) {
    if (instant(s)) {
      navigator.vibrate?.(15);
      store.log(s, { value: s.kind === 'med' ? s.unit : null, option_id: null });
    } else {
      onopen(s);
    }
  }

  const today = $derived.by(() => {
    const start = startOfDay(0).toISOString();
    return store.recent.filter((e) => e.occurred_at >= start);
  });

  function lastText(s: Symptom): string {
    const e = store.lastFor(s.id);
    if (!e) return 'not logged yet';
    const v = s.kind === 'scale' || s.kind === 'choice' || s.kind === 'number' ? `${e.value}${s.unit && s.kind === 'number' ? ' ' + s.unit : ''} · ` : '';
    return `${v}${relTime(e.occurred_at, now)}`;
  }
</script>

{#snippet tile(s: Symptom)}
  {@const n = store.todayCount(s.id)}
  <button class="tile card" class:med={s.kind === 'med'} class:done={s.kind === 'med' && n > 0} onclick={() => tap(s)}>
    <span class="icon">{s.icon ?? '•'}</span>
    <span class="name">{s.name}</span>
    <span class="last small">{lastText(s)}</span>
    {#if n > 0}
      <span class="badge" title="today">{s.kind === 'med' ? '✓' : ''}{n > 1 || s.kind !== 'med' ? n : ''}</span>
    {/if}
  </button>
{/snippet}

<div class="grid">
  {#each store.pinned as s (s.id)}
    {@render tile(s)}
  {/each}
</div>

{#if store.unpinned.length}
  <button class="more" onclick={() => (showMore = !showMore)}>
    {showMore ? '− Fewer' : `+ More (${store.unpinned.length})`}
  </button>
  {#if showMore}
    <div class="grid">
      {#each store.unpinned as s (s.id)}
        {@render tile(s)}
      {/each}
    </div>
  {/if}
{/if}

{#if store.symptoms.length === 0}
  <p class="muted">No symptoms yet. Add some under Settings.</p>
{/if}

<h2>Today</h2>
{#if today.length === 0}
  <p class="muted">Nothing logged today yet.</p>
{:else}
  <ul class="list card">
    {#each today as e (e.id)}
      <li>
        <button onclick={() => onedit(e.id)}>
          <span class="time">{clock(e.occurred_at)}</span>
          <span class="what">
            <b>{e.symptom_name}</b>{#if e.value && e.kind !== 'bool' && !(e.kind === 'count' && e.value === '1')}: {e.value}{/if}
            {#if e.note}<span class="note muted"> — {e.note}</span>{/if}
          </span>
          <span class="who small muted">
            {#if e.pending}<span class="chip pending">sync</span>{:else}{e.logged_by_name ?? ''}{/if}
          </span>
        </button>
      </li>
    {/each}
  </ul>
{/if}

<style>
  .grid { display: grid; grid-template-columns: repeat(2, 1fr); gap: 10px; }
  @media (min-width: 520px) { .grid { grid-template-columns: repeat(3, 1fr); } }
  .tile {
    position: relative; display: flex; flex-direction: column; align-items: flex-start; gap: 2px;
    padding: 14px; min-height: 96px; text-align: left; transition: transform 0.08s;
  }
  .tile:active { transform: scale(0.97); }
  .tile.done { background: var(--good-soft); }
  .icon { font-size: 26px; line-height: 1; margin-bottom: 4px; }
  .name { font-weight: 650; font-size: 16px; }
  .last { color: var(--muted); }
  .badge {
    position: absolute; top: 10px; right: 10px; min-width: 24px; height: 24px; padding: 0 6px;
    border-radius: 12px; background: var(--accent-soft); color: var(--accent);
    font-size: 13px; font-weight: 700; display: grid; place-items: center;
  }
  .tile.done .badge { background: var(--good); color: #fff; }
  .more { margin: 14px 0 4px; color: var(--accent); font-weight: 600; padding: 6px 0; }
  .list { list-style: none; margin: 0; padding: 4px 0; }
  .list li + li { border-top: 1px solid var(--line); }
  .list button { display: flex; width: 100%; gap: 12px; padding: 12px 14px; text-align: left; align-items: baseline; }
  .time { font-variant-numeric: tabular-nums; color: var(--muted); min-width: 44px; }
  .what { flex: 1; }
  .note { font-size: 14px; }
</style>
