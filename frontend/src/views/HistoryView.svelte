<script lang="ts">
  import { api, type Entry, type Symptom } from '../lib/api';
  import { store } from '../lib/store.svelte';
  import { clock, dayKey, dayLabel, startOfDay } from '../lib/time';

  let { onedit }: { onedit: (e: Entry) => void } = $props();

  let days = $state(7);
  let symptomId = $state<number | ''>('');
  let entries = $state<Entry[]>([]);
  let loading = $state(false);
  let error = $state('');
  let allSymptoms = $state<Symptom[]>([]);

  $effect(() => {
    api.symptoms(true).then((s) => (allSymptoms = s)).catch(() => (allSymptoms = store.symptoms));
  });

  $effect(() => {
    // Re-run when filters change or entries were edited elsewhere.
    void store.version;
    const d = days;
    const sid = symptomId === '' ? undefined : symptomId;
    loading = true;
    error = '';
    api
      .entries({ from: startOfDay(d - 1).toISOString(), symptom_id: sid, limit: 5000 })
      .then((r) => (entries = r))
      .catch((e) => (error = store.online ? String(e.message ?? e) : 'Offline — showing nothing until you reconnect.'))
      .finally(() => (loading = false));
  });

  const groups = $derived.by(() => {
    const m = new Map<string, Entry[]>();
    for (const e of entries) {
      const k = dayKey(e.occurred_at);
      if (!m.has(k)) m.set(k, []);
      m.get(k)!.push(e);
    }
    return [...m.entries()];
  });
</script>

<div class="filters">
  <select bind:value={symptomId} aria-label="Filter by symptom">
    <option value="">All symptoms</option>
    {#each allSymptoms as s (s.id)}
      <option value={s.id}>{s.icon ?? ''} {s.name}{s.archived ? ' (archived)' : ''}</option>
    {/each}
  </select>
</div>

{#if error}<p class="muted">{error}</p>{/if}

{#each groups as [key, list] (key)}
  <h2>{dayLabel(key)} <span class="muted small">· {list.length}</span></h2>
  <ul class="list card">
    {#each list as e (e.id)}
      <li>
        <button onclick={() => onedit(e)}>
          <span class="time">{clock(e.occurred_at)}</span>
          <span class="what">
            <b>{e.symptom_name}</b>{#if e.value && e.kind !== 'bool' && !(e.kind === 'count' && e.value === '1')}: {e.value}{/if}
            {#if e.note}<div class="note muted">{e.note}</div>{/if}
          </span>
          <span class="who small muted">{e.logged_by_name ?? ''}</span>
        </button>
      </li>
    {/each}
  </ul>
{:else}
  {#if !loading && !error}<p class="muted">No entries in the last {days} days.</p>{/if}
{/each}

<button class="btn block older" onclick={() => (days += 14)} disabled={loading}>
  {loading ? 'Loading…' : 'Show older'}
</button>

<style>
  .filters { position: sticky; top: 0; padding: 4px 0 6px; background: var(--bg); z-index: 2; }
  .list { list-style: none; margin: 0; padding: 4px 0; }
  .list li + li { border-top: 1px solid var(--line); }
  .list button { display: flex; width: 100%; gap: 12px; padding: 12px 14px; text-align: left; align-items: baseline; }
  .time { font-variant-numeric: tabular-nums; color: var(--muted); min-width: 44px; }
  .what { flex: 1; }
  .note { font-size: 14px; white-space: pre-wrap; }
  .older { margin-top: 20px; }
</style>
