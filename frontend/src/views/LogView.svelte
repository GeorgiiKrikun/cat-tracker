<script lang="ts">
  import { byCategory, normalOption, type Checkin, type Entry, type Tracker } from '../lib/api';
  import { store } from '../lib/store.svelte';
  import Timeline from '../lib/Timeline.svelte';
  import { relTime, startOfDay } from '../lib/time';

  let {
    onopen,
    onedit,
    oncheckin,
  }: { onopen: (t: Tracker) => void; onedit: (e: Entry) => void; oncheckin: (c: Checkin) => void } = $props();

  let showMore = $state(false);
  let now = $state(Date.now());
  $effect(() => {
    const t = setInterval(() => (now = Date.now()), 30_000);
    return () => clearInterval(t);
  });

  const instant = (t: Tracker) => t.kind === 'bool' || t.kind === 'count' || t.kind === 'med';

  function tap(t: Tracker) {
    if (instant(t)) {
      navigator.vibrate?.(15);
      store.log(t, { value: t.kind === 'med' ? t.unit : null, option_id: null });
    } else {
      onopen(t);
    }
  }

  const today = $derived.by(() => {
    void now;
    const start = startOfDay(0).toISOString();
    return store.recent.filter((e) => e.occurred_at >= start);
  });
  const anyNormal = $derived(store.trackers.some((t) => normalOption(t)));

  function lastText(t: Tracker): string {
    const e = store.lastFor(t.id);
    const normal = normalOption(t);
    // Nothing logged today for a tracker with a normal option: that's what today is.
    if (normal && !store.todayCount(t.id)) return `assumed ${normal.label}`;
    if (!e) return 'not logged yet';
    const v = t.kind === 'scale' || t.kind === 'choice' || t.kind === 'number' ? `${e.value}${t.unit && t.kind === 'number' ? ' ' + t.unit : ''} · ` : '';
    return `${v}${relTime(e.occurred_at, now)}`;
  }
</script>

{#snippet tile(t: Tracker)}
  {@const n = store.todayCount(t.id)}
  {@const assumed = !!normalOption(t) && n === 0}
  <button class="tile card" class:med={t.kind === 'med'} class:done={t.kind === 'med' && n > 0} onclick={() => tap(t)}>
    <span class="icon">{t.icon ?? '•'}</span>
    <span class="name">{t.name}</span>
    <span class="last small" class:assumed>{lastText(t)}</span>
    {#if n > 0}
      <span class="badge" title="today">{t.kind === 'med' ? '✓' : ''}{n > 1 || t.kind !== 'med' ? n : ''}</span>
    {/if}
  </button>
{/snippet}

{#snippet groups(list: Tracker[])}
  {#each byCategory(list) as g (g.label)}
    <h3 class="group">{g.label}</h3>
    <div class="grid">
      {#each g.items as t (t.id)}
        {@render tile(t)}
      {/each}
    </div>
  {/each}
{/snippet}

{@render groups(store.pinned)}

{#if store.unpinned.length}
  <button class="more" onclick={() => (showMore = !showMore)}>
    {showMore ? '− Fewer' : `+ More (${store.unpinned.length})`}
  </button>
  {#if showMore}
    {@render groups(store.unpinned)}
  {/if}
{/if}

{#if store.trackers.length === 0}
  <p class="muted">Nothing to track yet. Add trackers under Settings.</p>
{:else if !store.observedToday}
  <button class="card checkin" onclick={() => store.checkIn()}>
    <span class="ci-title">✓ Nothing unusual today</span>
    <span class="small muted">
      {anyNormal ? 'Everything you don’t log today counts as normal.' : 'Marks today as checked, so “nothing logged” means none.'}
    </span>
  </button>
{:else if anyNormal}
  <p class="muted small observed">
    {store.checkinToday ? 'Checked: nothing unusual today.' : 'Anything you don’t log today counts as normal.'}
  </p>
{/if}

<h2>Today</h2>
{#if today.length === 0 && !store.checkinToday}
  <p class="muted">Nothing logged today yet.</p>
{:else}
  <Timeline entries={today} checkins={store.checkinToday ? [store.checkinToday] : []} {onedit} {oncheckin} />
{/if}

<style>
  .group { margin: 14px 2px 6px; font-size: 13px; font-weight: 650; color: var(--muted); text-transform: uppercase; letter-spacing: 0.04em; }
  .group:first-child { margin-top: 2px; }
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
  .last.assumed { font-style: italic; }
  .badge {
    position: absolute; top: 10px; right: 10px; min-width: 24px; height: 24px; padding: 0 6px;
    border-radius: 12px; background: var(--accent-soft); color: var(--accent);
    font-size: 13px; font-weight: 700; display: grid; place-items: center;
  }
  .tile.done .badge { background: var(--good); color: #fff; }
  .more { margin: 14px 0 4px; color: var(--accent); font-weight: 600; padding: 6px 0; }
  .checkin {
    display: flex; flex-direction: column; gap: 2px; width: 100%; margin-top: 16px; padding: 14px 16px;
    text-align: left; border: 2px dashed var(--good); box-shadow: none; background: var(--good-soft);
  }
  .checkin:active { transform: scale(0.99); }
  .ci-title { font-weight: 650; color: var(--good); }
  .observed { margin: 16px 2px 0; }
</style>
