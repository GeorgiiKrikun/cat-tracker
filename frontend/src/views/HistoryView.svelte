<script lang="ts">
  import { api, type Checkin, type Entry, type Tracker } from '../lib/api';
  import { store } from '../lib/store.svelte';
  import Timeline from '../lib/Timeline.svelte';
  import { dayKey, dayLabel, startOfDay } from '../lib/time';

  let { onedit, oncheckin }: { onedit: (e: Entry) => void; oncheckin: (c: Checkin) => void } = $props();

  let days = $state(7);
  let trackerId = $state<number | ''>('');
  let entries = $state<Entry[]>([]);
  let checkins = $state<Checkin[]>([]);
  let loading = $state(false);
  let error = $state('');
  let allTrackers = $state<Tracker[]>([]);

  $effect(() => {
    const pid = store.patientId;
    trackerId = ''; // filters belong to the previous patient
    if (pid === null) return;
    api.trackers(pid, true).then((t) => (allTrackers = t)).catch(() => (allTrackers = store.trackers));
  });

  $effect(() => {
    // Re-run when filters or the patient change, or entries were edited elsewhere.
    void store.version;
    const pid = store.patientId;
    const d = days;
    const tid = trackerId === '' ? undefined : trackerId;
    if (pid === null) return;
    loading = true;
    error = '';
    const from = startOfDay(d - 1).toISOString();
    Promise.all([
      api.entries(pid, { from, tracker_id: tid, limit: 5000 }),
      // Check-ins aren't about one tracker, so a tracker filter hides them.
      tid === undefined ? api.checkins(pid, { from }) : Promise.resolve([]),
    ])
      .then(([e, c]) => {
        entries = e;
        checkins = c;
      })
      .catch((e) => (error = store.online ? String(e.message ?? e) : 'Offline — showing nothing until you reconnect.'))
      .finally(() => (loading = false));
  });

  const groups = $derived.by(() => {
    const m = new Map<string, { entries: Entry[]; checkins: Checkin[] }>();
    const day = (iso: string) => {
      const k = dayKey(iso);
      if (!m.has(k)) m.set(k, { entries: [], checkins: [] });
      return m.get(k)!;
    };
    for (const e of entries) day(e.occurred_at).entries.push(e);
    for (const c of checkins) day(c.occurred_at).checkins.push(c);
    return [...m.entries()].sort(([a], [b]) => b.localeCompare(a));
  });
</script>

<div class="filters">
  <select bind:value={trackerId} aria-label="Filter by tracker">
    <option value="">Everything</option>
    {#each allTrackers as t (t.id)}
      <option value={t.id}>{t.icon ?? ''} {t.name}{t.archived ? ' (archived)' : ''}</option>
    {/each}
  </select>
</div>

{#if error}<p class="muted">{error}</p>{/if}

{#each groups as [key, g] (key)}
  <h2>{dayLabel(key)} <span class="muted small">· {g.entries.length}</span></h2>
  <Timeline entries={g.entries} checkins={g.checkins} {onedit} {oncheckin} />
{:else}
  {#if !loading && !error}<p class="muted">Nothing logged in the last {days} days.</p>{/if}
{/each}

<button class="btn block older" onclick={() => (days += 14)} disabled={loading}>
  {loading ? 'Loading…' : 'Show older'}
</button>

<style>
  .filters { position: sticky; top: 0; padding: 4px 0 6px; background: var(--bg); z-index: 2; }
  .older { margin-top: 20px; }
</style>
