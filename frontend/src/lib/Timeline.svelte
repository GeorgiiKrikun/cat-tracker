<script lang="ts">
  import type { Checkin, Entry } from './api';
  import { clock } from './time';

  let {
    entries,
    checkins = [],
    onedit,
    oncheckin,
  }: {
    entries: Entry[];
    checkins?: Checkin[];
    onedit: (e: Entry) => void;
    oncheckin: (c: Checkin) => void;
  } = $props();

  /** Entries and check-ins in one list, newest first. */
  const items = $derived(
    [
      ...entries.map((e) => ({ entry: e, checkin: null, at: e.occurred_at })),
      ...checkins.map((c) => ({ entry: null, checkin: c, at: c.occurred_at })),
    ].sort((a, b) => b.at.localeCompare(a.at)),
  );
</script>

<ul class="list card">
  {#each items as { entry: e, checkin: c, at } (e?.id ?? c!.id)}
    <li>
      {#if e}
        <button onclick={() => onedit(e)}>
          <span class="time">{clock(at)}</span>
          <span class="what">
            <b>{e.tracker_name}</b>{#if e.value && e.kind !== 'bool' && !(e.kind === 'count' && e.value === '1')}: {e.value}{/if}
            {#if e.note}<div class="note muted">{e.note}</div>{/if}
          </span>
          <span class="who small muted">
            {#if e.pending}<span class="chip pending">sync</span>{:else}{e.logged_by_name ?? ''}{/if}
          </span>
        </button>
      {:else if c}
        <button onclick={() => oncheckin(c)}>
          <span class="time">{clock(at)}</span>
          <span class="what checkin">✓ Nothing unusual</span>
          <span class="who small muted">
            {#if c.pending}<span class="chip pending">sync</span>{:else}{c.logged_by_name ?? ''}{/if}
          </span>
        </button>
      {/if}
    </li>
  {/each}
</ul>

<style>
  .list { list-style: none; margin: 0; padding: 4px 0; }
  .list li + li { border-top: 1px solid var(--line); }
  .list button { display: flex; width: 100%; gap: 12px; padding: 12px 14px; text-align: left; align-items: baseline; }
  .time { font-variant-numeric: tabular-nums; color: var(--muted); min-width: 44px; }
  .what { flex: 1; }
  .checkin { color: var(--good); font-weight: 600; }
  .note { font-size: 14px; white-space: pre-wrap; }
</style>
