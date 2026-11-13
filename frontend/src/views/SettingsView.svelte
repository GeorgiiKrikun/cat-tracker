<script lang="ts">
  import { api, KIND_LABELS, type Kind, type Symptom } from '../lib/api';
  import Sheet from '../lib/Sheet.svelte';
  import { store } from '../lib/store.svelte';

  let all = $state<Symptom[]>([]);
  let editing = $state<Partial<Symptom> | null>(null);
  let optionsText = $state('');
  let error = $state('');
  let busy = $state(false);

  async function load() {
    all = await api.symptoms(true);
  }
  $effect(() => {
    load().catch(() => (all = store.symptoms));
  });

  const active = $derived(all.filter((s) => !s.archived));
  const archived = $derived(all.filter((s) => s.archived));

  async function patch(s: Symptom, p: Partial<Symptom>) {
    await api.updateSymptom(s.id, p);
    await Promise.all([load(), store.reloadSymptoms()]);
  }

  async function move(i: number, dir: -1 | 1) {
    const ids = active.map((s) => s.id);
    const j = i + dir;
    if (j < 0 || j >= ids.length) return;
    [ids[i], ids[j]] = [ids[j], ids[i]];
    await api.reorderSymptoms([...ids, ...archived.map((s) => s.id)]);
    await Promise.all([load(), store.reloadSymptoms()]);
  }

  function openNew() {
    editing = { name: '', kind: 'bool', icon: '', unit: '', pinned: true, options: [] };
    optionsText = '';
    error = '';
  }
  function openEdit(s: Symptom) {
    editing = { ...s };
    optionsText = s.options.join('\n');
    error = '';
  }

  const needsOptions = $derived(editing?.kind === 'scale' || editing?.kind === 'choice');

  async function saveEdit() {
    if (!editing) return;
    busy = true;
    error = '';
    const body: Partial<Symptom> = {
      name: editing.name,
      icon: editing.icon || null,
      unit: editing.unit || null,
      pinned: editing.pinned,
    };
    if (needsOptions) body.options = optionsText.split('\n').map((o) => o.trim()).filter(Boolean);
    try {
      if (editing.id) await api.updateSymptom(editing.id, body);
      else await api.createSymptom({ ...body, kind: editing.kind });
      editing = null;
      await Promise.all([load(), store.reloadSymptoms()]);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  const kinds = Object.keys(KIND_LABELS) as Kind[];
</script>

<h2>Symptoms & medications</h2>
<p class="muted small">⭐ = shown as a button on the Log screen. The rest are under “More”.</p>

<ul class="list card">
  {#each active as s, i (s.id)}
    <li>
      <button class="star" class:on={s.pinned} onclick={() => patch(s, { pinned: !s.pinned })} aria-label={s.pinned ? 'Unpin' : 'Pin'}>
        {s.pinned ? '⭐' : '☆'}
      </button>
      <button class="name" onclick={() => openEdit(s)}>
        <span>{s.icon ?? ''} {s.name}</span>
        <span class="muted small">{KIND_LABELS[s.kind]}{s.unit ? ` · ${s.unit}` : ''}</span>
      </button>
      <span class="order">
        <button onclick={() => move(i, -1)} disabled={i === 0} aria-label="Move up">▲</button>
        <button onclick={() => move(i, 1)} disabled={i === active.length - 1} aria-label="Move down">▼</button>
      </span>
    </li>
  {/each}
</ul>
<button class="btn primary block add" onclick={openNew}>+ Add symptom or medication</button>

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

<h2>Account</h2>
<div class="card account">
  <p>Logged in as <b>{store.user?.display_name}</b> <span class="muted">({store.user?.username})</span></p>
  <button class="btn block" onclick={() => store.logout()}>Log out</button>
</div>
<p class="muted small">New people are added on the server with <code>cattracker add-user</code>.</p>

{#if editing}
  <Sheet title={editing.id ? `Edit ${editing.name}` : 'New symptom'} onclose={() => (editing = null)}>
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
      <label for="s-opts">Options, one per line{editing.kind === 'scale' ? ' (worst/least first)' : ''}</label>
      <textarea id="s-opts" bind:value={optionsText} rows="5" placeholder={'none\nlittle\nnormal'}></textarea>
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
      <button class="btn block archive" onclick={async () => { await patch(editing as Symptom, { archived: true }); editing = null; }}>
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
  .account { padding: 4px 14px 14px; }
  .fixed { margin: 0; }
  .check { display: flex; gap: 10px; align-items: center; color: var(--text); margin-top: 18px; }
  .check input { width: 22px; height: 22px; }
  .save { margin-top: 18px; }
  .archive { margin-top: 10px; }
  .error { color: var(--bad); }
  code { background: var(--surface-2); padding: 1px 5px; border-radius: 5px; font-size: 13px; }
</style>
