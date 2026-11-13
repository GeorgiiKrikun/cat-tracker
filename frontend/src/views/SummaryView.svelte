<script lang="ts">
  import { api, type Entry, type Symptom, type SymptomOption } from '../lib/api';
  import { store } from '../lib/store.svelte';
  import { clock, dayKey, dayLabel, lastDays, shortDay, startOfDay } from '../lib/time';

  let range = $state(14);
  let entries = $state<Entry[]>([]);
  let symptoms = $state<Symptom[]>([]);
  let loading = $state(true);
  let error = $state('');
  /** Tapped mark info per symptom id (tooltips don't exist on touch screens). */
  let tips = $state<Record<number, string>>({});

  $effect(() => {
    void store.version;
    const r = range;
    const pid = store.patientId;
    if (pid === null) return;
    loading = true;
    error = '';
    Promise.all([api.symptoms(pid, true), api.entries(pid, { from: startOfDay(r - 1).toISOString(), limit: 10000 })])
      .then(([s, e]) => {
        symptoms = s;
        entries = e;
      })
      .catch((e) => (error = store.online ? String(e.message ?? e) : 'Offline — the summary needs a connection.'))
      .finally(() => (loading = false));
  });

  const days = $derived(lastDays(range));

  interface Row {
    sym: Symptom;
    list: Entry[];
    byDay: Map<string, Entry[]>;
  }

  const rows = $derived.by(() => {
    const out: Row[] = [];
    for (const sym of symptoms) {
      const list = entries.filter((e) => e.symptom_id === sym.id);
      if (list.length === 0 && (sym.archived || sym.kind === 'note')) continue;
      const byDay = new Map<string, Entry[]>();
      for (const e of list) {
        const k = dayKey(e.occurred_at);
        if (!byDay.has(k)) byDay.set(k, []);
        byDay.get(k)!.push(e);
      }
      out.push({ sym, list, byDay });
    }
    return out;
  });

  // ---- chart geometry (SVG user units) ----
  const W = 360;
  const L = 74; // left gutter for labels
  const R = 6;
  const plotW = W - L - R;
  const colX = (i: number) => L + (plotW / days.length) * (i + 0.5);
  const colW = $derived(plotW / days.length);

  function countOf(e: Entry): number {
    return e.kind === 'count' ? Number(e.value) || 1 : 1;
  }
  function dayTotal(list: Entry[] | undefined): number {
    return (list ?? []).reduce((a, e) => a + countOf(e), 0);
  }
  function describe(day: string, list: Entry[] | undefined, unit?: string | null): string {
    if (!list?.length) return `${dayLabel(day)}: nothing logged`;
    const parts = list
      .slice()
      .reverse()
      .map((e) => `${clock(e.occurred_at)}${e.value && e.kind !== 'bool' ? ' ' + e.value + (unit && e.kind === 'number' ? ' ' + unit : '') : ''}${e.logged_by_name ? ' (' + e.logged_by_name + ')' : ''}`);
    return `${dayLabel(day)}: ${parts.join(', ')}`;
  }
  /** Chart rows: active options (best on top for scales), then retired ones that have data in this period. */
  function chartOptions(sym: Symptom, list: Entry[]): SymptomOption[] {
    const used = new Set(list.map((e) => e.option_id));
    const active = sym.options.filter((o) => !o.retired);
    const retired = sym.options.filter((o) => o.retired && used.has(o.id));
    return [...(sym.kind === 'scale' ? active.reverse() : active), ...retired];
  }
  function tip(id: number, text: string) {
    tips = { ...tips, [id]: text };
  }

  function exportUrl() {
    return store.patientId === null ? '' : api.exportUrl(store.patientId, startOfDay(range - 1).toISOString());
  }

  // Tick labels: first, last and every ~7th day.
  const tickEvery = $derived(range <= 7 ? 1 : range <= 14 ? 2 : 7);
</script>

<div class="toolbar no-print">
  <div class="seg" role="group" aria-label="Range">
    {#each [7, 14, 30] as r}
      <button class:active={range === r} onclick={() => (range = r)}>{r} days</button>
    {/each}
  </div>
</div>

<div class="print-head">
  <h1>Symptom summary{store.patient ? ` · ${store.patient.name}` : ''}</h1>
  <p class="muted">{dayLabel(days[0])} – {dayLabel(days[days.length - 1])} · {entries.length} entries</p>
</div>

{#if error}<p class="muted">{error}</p>{/if}
{#if loading && !entries.length}<p class="muted">Loading…</p>{/if}

{#snippet axis()}
  {#each days as d, i}
    {#if (i % tickEvery === 0 && days.length - 1 - i >= Math.max(2, tickEvery * 0.6)) || i === days.length - 1}
      <text x={colX(i)} y="0" class="tick" text-anchor="middle">{shortDay(d)}</text>
    {/if}
  {/each}
{/snippet}

{#each rows as { sym, list, byDay } (sym.id)}
  <section class="card chart">
    <header>
      <span class="title">{sym.icon ?? ''} {sym.name}</span>
      <span class="muted small">
        {#if sym.kind === 'med'}
          given on {days.filter((d) => byDay.has(d)).length} of {days.length} days
        {:else if sym.kind === 'number' && list.length}
          latest {list[0].value} {sym.unit ?? ''}
        {:else}
          {list.length} {list.length === 1 ? 'entry' : 'entries'}
        {/if}
      </span>
    </header>

    {#if list.length === 0}
      <p class="muted small empty">Nothing logged in this period.</p>
    {:else if sym.kind === 'note'}
      <ul class="notes">
        {#each list as e (e.id)}
          <li><span class="muted small">{dayLabel(dayKey(e.occurred_at))} {clock(e.occurred_at)}</span> {e.note}</li>
        {/each}
      </ul>
    {:else if sym.kind === 'scale' || sym.kind === 'choice'}
      {@const opts = chartOptions(sym, list)}
      {@const rowH = 18}
      {@const H = opts.length * rowH + 22}
      <svg viewBox="0 0 {W} {H}" role="img" aria-label="{sym.name} per day">
        {#each opts as o, j}
          <line x1={L} x2={W - R} y1={j * rowH + rowH / 2} y2={j * rowH + rowH / 2} class="grid" />
          <text x={L - 8} y={j * rowH + rowH / 2 + 4} class="ylab" class:retired={o.retired} text-anchor="end">{o.label.length > 10 ? o.label.slice(0, 9) + '…' : o.label}</text>
        {/each}
        {#each days as d, i}
          {@const de = byDay.get(d) ?? []}
          <rect x={colX(i) - colW / 2} y="0" width={colW} height={opts.length * rowH} class="hit" role="button" tabindex="-1"
            onclick={() => tip(sym.id, describe(d, de))} onkeydown={() => {}} />
          {#each opts as o, j}
            {@const n = de.filter((e) => e.option_id === o.id).length}
            {#if n}
              <circle cx={colX(i)} cy={j * rowH + rowH / 2} r={n > 1 ? 5.5 : 4} class="dot" class:retired={o.retired} pointer-events="none">
                <title>{describe(d, de)}</title>
              </circle>
            {/if}
          {/each}
        {/each}
        <g transform="translate(0 {H - 4})">{@render axis()}</g>
      </svg>
      {#if opts.some((o) => o.retired)}
        <p class="muted small legend">Greyed rows are retired options, still shown because they have entries in this period.</p>
      {/if}
    {:else if sym.kind === 'number'}
      {@const pts = days
        .map((d, i) => {
          const de = byDay.get(d);
          return de ? { i, d, v: Number(de[0].value), de } : null;
        })
        .filter((p) => p !== null && !isNaN(p.v))}
      {@const H = 90}
      {@const plotH = H - 30}
      {#if pts.length}
        {@const lo = Math.min(...pts.map((p) => p!.v))}
        {@const hi = Math.max(...pts.map((p) => p!.v))}
        {@const pad = (hi - lo) * 0.15 || Math.abs(hi) * 0.05 || 1}
        {@const y = (v: number) => 8 + plotH - ((v - (lo - pad)) / (hi + pad - (lo - pad))) * plotH}
        <svg viewBox="0 0 {W} {H}" role="img" aria-label="{sym.name} over time">
          <line x1={L} x2={W - R} y1={8 + plotH} y2={8 + plotH} class="grid" />
          <text x={L - 8} y={y(hi) + 4} class="ylab" text-anchor="end">{hi}</text>
          {#if hi !== lo}<text x={L - 8} y={y(lo) + 4} class="ylab" text-anchor="end">{lo}</text>{/if}
          {#if pts.length > 1}
            <polyline points={pts.map((p) => `${colX(p!.i)},${y(p!.v)}`).join(' ')} class="line" />
          {/if}
          {#each pts as p}
            <circle cx={colX(p!.i)} cy={y(p!.v)} r="4" class="dot ring" pointer-events="none" />
            <rect x={colX(p!.i) - 12} y="0" width="24" height={plotH + 10} class="hit" role="button" tabindex="-1"
              onclick={() => tip(sym.id, describe(p!.d, p!.de, sym.unit))} onkeydown={() => {}} />
          {/each}
          <g transform="translate(0 {H - 4})">{@render axis()}</g>
        </svg>
      {:else}
        <p class="muted small empty">No valid measurements in this period.</p>
      {/if}
    {:else}
      <!-- bool, count, med: bars per day -->
      {@const H = 80}
      {@const plotH = H - 32}
      {@const max = Math.max(1, ...days.map((d) => dayTotal(byDay.get(d))))}
      {@const bw = Math.max(3, Math.min(18, colW - 4))}
      <svg viewBox="0 0 {W} {H}" role="img" aria-label="{sym.name} per day">
        <line x1={L} x2={W - R} y1={12 + plotH} y2={12 + plotH} class="grid" />
        <text x={L - 8} y="16" class="ylab" text-anchor="end">{max}×</text>
        {#each days as d, i}
          {@const n = dayTotal(byDay.get(d))}
          {@const h = (n / max) * plotH}
          {#if n}
            <path
              d="M{colX(i) - bw / 2},{12 + plotH} v{-(h - 3)} q0,-3 3,-3 h{bw - 6} q3,0 3,3 v{h - 3} z"
              class="bar" class:med={sym.kind === 'med'} pointer-events="none"
            />
            {#if range <= 14}
              <text x={colX(i)} y={12 + plotH - h - 3} class="val" text-anchor="middle">{n}</text>
            {/if}
          {/if}
          <rect x={colX(i) - colW / 2} y="0" width={colW} height={plotH + 12} class="hit" role="button" tabindex="-1"
            onclick={() => tip(sym.id, describe(d, byDay.get(d)))} onkeydown={() => {}} />
        {/each}
        <g transform="translate(0 {H - 4})">{@render axis()}</g>
      </svg>
    {/if}

    {#if tips[sym.id]}<p class="tip small">{tips[sym.id]}</p>{/if}
  </section>
{/each}

<div class="actions no-print">
  <button class="btn" onclick={() => window.print()}>🖨️ Print / PDF</button>
  <a class="btn" href={exportUrl()} download>⬇️ Export CSV</a>
</div>
<p class="muted small no-print">Tap a day in a chart to see the details.</p>

<style>
  .toolbar { margin-bottom: 8px; }
  .seg { display: flex; background: var(--surface-2); border-radius: 12px; padding: 3px; }
  .seg button { flex: 1; min-height: 40px; border-radius: 10px; font-weight: 600; color: var(--muted); }
  .seg button.active { background: var(--surface); color: var(--text); box-shadow: var(--shadow); }
  .print-head { display: none; }
  .chart { padding: 12px 12px 8px; margin-top: 10px; break-inside: avoid; }
  .chart header { display: flex; justify-content: space-between; align-items: baseline; gap: 8px; margin-bottom: 6px; }
  .title { font-weight: 650; }
  svg { width: 100%; height: auto; display: block; overflow: visible; }
  .grid { stroke: var(--line); stroke-width: 1; }
  .ylab, .tick, .val { fill: var(--muted); font-size: 10px; }
  .val { fill: var(--text); font-size: 9px; }
  .dot { fill: var(--accent); }
  .dot.retired { fill: var(--muted); }
  .ylab.retired { font-style: italic; opacity: 0.7; }
  .legend { margin: 4px 2px 0; }
  .ring { stroke: var(--surface); stroke-width: 2; }
  .line { fill: none; stroke: var(--accent); stroke-width: 2; stroke-linejoin: round; }
  .bar { fill: var(--accent); }
  .bar.med { fill: var(--good); }
  .hit { fill: transparent; cursor: pointer; outline: none; }
  .tip { margin: 6px 2px 2px; padding: 8px 10px; background: var(--surface-2); border-radius: 10px; }
  .notes { margin: 0; padding-left: 18px; }
  .notes li { margin: 4px 0; }
  .empty { margin: 4px 0; }
  .actions { display: flex; gap: 10px; margin-top: 18px; }
  .actions .btn { flex: 1; text-decoration: none; }
  @media print {
    .print-head { display: block; margin-bottom: 8px; }
    .chart { border: 1px solid #ddd; }
    .tip { display: none; }
  }
</style>
