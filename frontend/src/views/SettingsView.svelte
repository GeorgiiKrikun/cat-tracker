<script lang="ts">
  import {
    api,
    byCategory,
    hasOptions,
    KIND_LABELS,
    type Kind,
    type OptionInput,
    type Tracker,
    type TrackerInput,
  } from '../lib/api';
  import Sheet from '../lib/Sheet.svelte';
  import { store } from '../lib/store.svelte';

  let all = $state<Tracker[]>([]);
  let editing = $state<Partial<Tracker> | null>(null);
  /** The option list being edited; `id` is missing for options not saved yet. */
  type EditOption = OptionInput & { uses: number };
  let opts = $state<EditOption[]>([]);
  let error = $state('');
  let busy = $state(false);

  /** Patient whose trackers are shown (Settings only renders with one selected). */
  const pid = () => store.patientId!;

  async function load() {
    all = await api.trackers(pid(), true);
  }
  $effect(() => {
    if (store.patientId === null) {
      all = [];
      return;
    }
    load().catch(() => (all = store.trackers));
  });

  const active = $derived(all.filter((s) => !s.archived));
  const archived = $derived(all.filter((s) => s.archived));

  async function patch(s: Tracker, p: TrackerInput) {
    await api.updateTracker(pid(), s.id, p);
    await Promise.all([load(), store.reloadTrackers()]);
  }

  /** Swap a tracker with its neighbour in the same group (groups are shown by kind). */
  async function move(group: Tracker[], k: number, dir: -1 | 1) {
    const other = group[k + dir];
    if (!other) return;
    const ids = active.map((t) => t.id);
    const a = ids.indexOf(group[k].id);
    const b = ids.indexOf(other.id);
    [ids[a], ids[b]] = [ids[b], ids[a]];
    await api.reorderTrackers(pid(), [...ids, ...archived.map((t) => t.id)]);
    await Promise.all([load(), store.reloadTrackers()]);
  }

  function openNew() {
    editing = { name: '', kind: 'bool', icon: '', unit: '', pinned: true, options: [] };
    opts = [newOption(), newOption()];
    error = '';
  }
  function openEdit(s: Tracker) {
    editing = { ...s };
    opts = s.options.map((o) => ({ ...o }));
    error = '';
  }

  const needsOptions = $derived(hasOptions(editing?.kind));

  // ---- option list editor ----
  const newOption = (): EditOption => ({ label: '', retired: false, normal: false, uses: 0 });
  /** Indexes into `opts` of the active options, in order. */
  const activeIdx = $derived(opts.flatMap((o, i) => (o.retired ? [] : [i])));
  const retiredOpts = $derived(opts.filter((o) => o.retired));

  function moveOpt(k: number, dir: -1 | 1) {
    const a = activeIdx[k];
    const b = activeIdx[k + dir];
    if (b === undefined) return;
    [opts[a], opts[b]] = [opts[b], opts[a]];
  }
  /** Unused options are simply removed; used ones are retired so their history stays. */
  function retireOrRemove(i: number) {
    if (opts[i].uses > 0) {
      opts[i].retired = true;
      opts[i].normal = false;
    } else opts.splice(i, 1);
  }
  /** Index into `opts` of the normal option, or -1: what a day without an entry means. */
  const normalIdx = $derived(opts.findIndex((o) => o.normal && !o.retired));
  function setNormal(i: number) {
    opts.forEach((o, j) => (o.normal = j === i));
  }
  function unusedRetired(o: EditOption) {
    opts.splice(opts.indexOf(o), 1);
  }
  async function merge(from: EditOption, intoId: number) {
    const into = opts.find((o) => o.id === intoId);
    if (!editing?.id || from.id === undefined || !into) return;
    const n = from.uses === 1 ? '1 entry' : `${from.uses} entries`;
    if (!confirm(`Move ${n} from “${from.label}” to “${into.label}” and delete “${from.label}”?\n\nThis rewrites history and can't be undone.`)) return;
    error = '';
    try {
      await api.mergeOption(pid(), editing.id, from.id, intoId);
      // Apply locally so other unsaved edits in the form survive.
      into.uses += from.uses;
      opts.splice(opts.indexOf(from), 1);
      await Promise.all([load(), store.reloadTrackers()]);
      store.version++;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function saveEdit() {
    if (!editing) return;
    busy = true;
    error = '';
    const body: TrackerInput = {
      name: editing.name,
      icon: editing.icon || null,
      unit: editing.unit || null,
      pinned: editing.pinned,
    };
    if (needsOptions) {
      body.options = opts
        .filter((o) => o.id !== undefined || o.label.trim()) // drop blank new rows
        .map(({ id, label, retired, normal }) => ({ id, label, retired, normal }));
    }
    try {
      if (editing.id) await api.updateTracker(pid(), editing.id, body);
      else await api.createTracker(pid(), { ...body, kind: editing.kind });
      editing = null;
      await Promise.all([load(), store.reloadTrackers()]);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  const kinds = Object.keys(KIND_LABELS) as Kind[];
</script>

{#if store.patient}
<h2>Trackers · {store.patient.name}</h2>
<p class="muted small">
  {#if store.patient.shared_with.length}Shared with {store.patient.shared_with.join(', ')}.{:else}Only you can see this patient.{/if}
  ⭐ = shown as a button on the Log screen. The rest are under “More”.
</p>

{#each byCategory(active) as g (g.label)}
  <h3 class="group">{g.label}</h3>
  <ul class="list card">
    {#each g.items as t, k (t.id)}
      {@const normal = t.options.find((o) => o.normal && !o.retired)}
      <li>
        <button class="star" class:on={t.pinned} onclick={() => patch(t, { pinned: !t.pinned })} aria-label={t.pinned ? 'Unpin' : 'Pin'}>
          {t.pinned ? '⭐' : '☆'}
        </button>
        <button class="name" onclick={() => openEdit(t)}>
          <span>{t.icon ?? ''} {t.name}</span>
          <span class="muted small">
            {KIND_LABELS[t.kind]}{t.unit ? ` · ${t.unit}` : ''}{normal ? ` · usually ${normal.label}` : ''}
          </span>
        </button>
        <span class="order">
          <button onclick={() => move(g.items, k, -1)} disabled={k === 0} aria-label="Move up">▲</button>
          <button onclick={() => move(g.items, k, 1)} disabled={k === g.items.length - 1} aria-label="Move down">▼</button>
        </span>
      </li>
    {/each}
  </ul>
{/each}
<button class="btn primary block add" onclick={openNew}>+ Add tracker</button>

{#if archived.length}
  <h2>Archived</h2>
  <p class="muted small">Hidden from logging. Their history stays in History and the summary.</p>
  <ul class="list card">
    {#each archived as s (s.id)}
      <li>
        <span class="name static"><span>{s.icon ?? ''} {s.name}</span></span>
        <button class="btn" onclick={() => patch(s, { archived: false })}>Restore</button>
      </li>
    {/each}
  </ul>
{/if}

{/if}

<h2>Account</h2>
<div class="card account">
  <p>Logged in as <b>{store.user?.display_name}</b> <span class="muted">({store.user?.username})</span></p>
  <button class="btn block" onclick={() => store.logout()}>Log out</button>
</div>
<p class="muted small">
  People and patients are managed on the server: <code>cattracker add-user</code>, <code>add-patient</code>, <code>share</code>.
</p>

{#if editing}
  <Sheet title={editing.id ? `Edit ${editing.name}` : 'New tracker'} onclose={() => (editing = null)}>
    <label for="s-name">Name</label>
    <input id="s-name" bind:value={editing.name} placeholder="e.g. Sneezing" />

    <label for="s-icon">Icon (emoji)</label>
    <input id="s-icon" bind:value={editing.icon} placeholder="e.g. 🤧" maxlength="8" />

    <label for="s-kind">How do you log it?</label>
    {#if editing.id}
      <p class="fixed">{KIND_LABELS[editing.kind as Kind]} <span class="muted small">— can't change after creating</span></p>
    {:else}
      <select id="s-kind" bind:value={editing.kind}>
        {#each kinds as k}<option value={k}>{KIND_LABELS[k]}</option>{/each}
      </select>
    {/if}

    {#if needsOptions}
      <span class="label">Options{editing.kind === 'scale' ? ' (worst/least first)' : ''}</span>
      <ul class="opts">
        {#each activeIdx as i, k (opts[i])}
          {@const o = opts[i]}
          <li>
            {#if editing.kind === 'scale'}
              <span class="order">
                <button onclick={() => moveOpt(k, -1)} disabled={k === 0} aria-label="Move up">▲</button>
                <button onclick={() => moveOpt(k, 1)} disabled={k === activeIdx.length - 1} aria-label="Move down">▼</button>
              </span>
            {/if}
            <input bind:value={o.label} placeholder="e.g. little" aria-label="Option {k + 1}" />
            <span class="uses small muted">{o.uses ? `${o.uses}×` : ''}</span>
            <button class="btn small-btn" onclick={() => retireOrRemove(i)}>{o.uses ? 'Retire' : 'Remove'}</button>
          </li>
        {/each}
      </ul>
      <button class="link" onclick={() => opts.push(newOption())}>+ Add option</button>

      <label for="s-normal">On a day without an entry</label>
      <select id="s-normal" value={normalIdx} onchange={(ev) => setNormal(Number(ev.currentTarget.value))}>
        <option value={-1}>Unknown: show a gap</option>
        {#each activeIdx as i, k (opts[i])}
          <option value={i}>Assume “{opts[i].label.trim() || `option ${k + 1}`}”</option>
        {/each}
      </select>
      <p class="muted small hint">
        Pick the usual state (e.g. “normal”) to log only when something is off. It's assumed on days someone
        logged anything for {store.patient?.name ?? "this patient"}, or marked “Nothing unusual today”.
      </p>
      <p class="muted small hint">
        Editing a label renames it everywhere, past entries included. To replace an option with something
        different, retire it and add a new one: old entries keep the old option.
      </p>

      {#if retiredOpts.length}
        <span class="label">Retired options</span>
        <p class="muted small hint">Not offered when logging. Past entries and the summary still show them.</p>
        <ul class="opts">
          {#each retiredOpts as o (o)}
            <li class="retired">
              <span class="rlabel">{o.label} <span class="small muted">· {o.uses}×</span></span>
              <button class="btn small-btn" onclick={() => (o.retired = false)}>Restore</button>
              {#if o.uses && o.id !== undefined}
                <select
                  class="merge"
                  aria-label="Merge {o.label} into another option"
                  onchange={(ev) => {
                    const t = ev.currentTarget;
                    const into = Number(t.value);
                    t.value = '';
                    if (into) merge(o, into);
                  }}
                >
                  <option value="">Merge into…</option>
                  {#each opts.filter((x) => x !== o && x.id !== undefined) as t (t.id)}
                    <option value={t.id}>{t.label}{t.retired ? ' (retired)' : ''}</option>
                  {/each}
                </select>
              {:else if !o.uses}
                <button class="btn small-btn" onclick={() => unusedRetired(o)}>Remove</button>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    {/if}
    {#if editing.kind === 'number'}
      <label for="s-unit">Unit</label>
      <input id="s-unit" bind:value={editing.unit} placeholder="e.g. kg" />
    {/if}
    {#if editing.kind === 'med'}
      <label for="s-unit">Default dose (optional)</label>
      <input id="s-unit" bind:value={editing.unit} placeholder="e.g. ½ tablet" />
    {/if}

    <label class="check"><input type="checkbox" bind:checked={editing.pinned} /> Show as a button on the Log screen</label>

    {#if error}<p class="error">{error}</p>{/if}
    <button class="btn primary block save" onclick={saveEdit} disabled={busy}>Save</button>
    {#if editing.id}
      <button class="btn block archive" onclick={async () => { await patch(editing as Tracker, { archived: true }); editing = null; }}>
        Archive (hide, keep history)
      </button>
    {/if}
  </Sheet>
{/if}

<style>
  .list { list-style: none; margin: 0; padding: 4px 0; }
  .list li { display: flex; align-items: center; gap: 4px; padding: 4px 8px; }
  .list li + li { border-top: 1px solid var(--line); }
  .star { width: 40px; height: 44px; font-size: 20px; color: var(--muted); }
  .name { flex: 1; display: flex; flex-direction: column; text-align: left; padding: 6px 4px; min-height: 44px; justify-content: center; }
  .name.static { padding-left: 8px; }
  .order { display: flex; }
  .order button { width: 36px; height: 44px; color: var(--muted); font-size: 12px; }
  .order button:disabled { opacity: 0.25; }
  .add { margin-top: 12px; }
  .group { margin: 14px 2px 6px; font-size: 13px; font-weight: 650; color: var(--muted); text-transform: uppercase; letter-spacing: 0.04em; }
  .account { padding: 4px 14px 14px; }
  .fixed { margin: 0; }
  .check { display: flex; gap: 10px; align-items: center; color: var(--text); margin-top: 18px; }
  .check input { width: 22px; height: 22px; }
  .save { margin-top: 18px; }
  .archive { margin-top: 10px; }
  .error { color: var(--bad); }
  .label { display: block; font-size: 14px; color: var(--muted); margin: 14px 0 6px; }
  .opts { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; }
  .opts li { display: flex; align-items: center; gap: 6px; }
  .opts input { flex: 1; min-width: 0; margin: 0; }
  .opts .order button { width: 30px; height: 40px; }
  .opts .uses { min-width: 28px; text-align: right; }
  .opts .small-btn { min-height: 40px; padding: 0 12px; font-size: 14px; flex-shrink: 0; }
  .opts li.retired { flex-wrap: wrap; }
  .rlabel { flex: 1; font-style: italic; min-width: 0; overflow-wrap: anywhere; }
  .merge { width: auto; min-height: 40px; margin: 0; font-size: 14px; }
  .link { color: var(--accent); font-weight: 600; padding: 10px 0 0; }
  .hint { margin: 6px 0 0; }
  code { background: var(--surface-2); padding: 1px 5px; border-radius: 5px; font-size: 13px; }
</style>
