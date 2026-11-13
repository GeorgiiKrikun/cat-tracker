<script lang="ts">
  import { api, type Entry, type Symptom } from './lib/api';
  import EntryForm from './lib/EntryForm.svelte';
  import Sheet from './lib/Sheet.svelte';
  import { store } from './lib/store.svelte';
  import HistoryView from './views/HistoryView.svelte';
  import LogView from './views/LogView.svelte';
  import LoginView from './views/LoginView.svelte';
  import SettingsView from './views/SettingsView.svelte';
  import SummaryView from './views/SummaryView.svelte';

  type Tab = 'log' | 'history' | 'summary' | 'settings';
  const tabs: { id: Tab; label: string; icon: string }[] = [
    { id: 'log', label: 'Log', icon: '➕' },
    { id: 'history', label: 'History', icon: '🕘' },
    { id: 'summary', label: 'Summary', icon: '📊' },
    { id: 'settings', label: 'Settings', icon: '⚙️' },
  ];

  const fromHash = (): Tab => {
    const h = location.hash.slice(1) as Tab;
    return tabs.some((t) => t.id === h) ? h : 'log';
  };
  let tab = $state<Tab>(fromHash());
  $effect(() => {
    const on = () => (tab = fromHash());
    window.addEventListener('hashchange', on);
    return () => window.removeEventListener('hashchange', on);
  });

  store.init();

  // Sheets
  let logging = $state<Symptom | null>(null);
  let editing = $state<{ entry: Entry; symptom: Symptom } | null>(null);

  async function symptomFor(e: Entry): Promise<Symptom> {
    const s = store.symptoms.find((x) => x.id === e.symptom_id);
    if (s) return s;
    const all = await api.symptoms(true);
    const found = all.find((x) => x.id === e.symptom_id);
    if (!found) throw new Error('symptom not found');
    return found;
  }

  async function openEdit(e: Entry | string) {
    const entry = typeof e === 'string' ? store.recent.find((r) => r.id === e) : e;
    if (!entry) return;
    try {
      editing = { entry, symptom: await symptomFor(entry) };
    } catch {
      store.showToast("Can't edit while offline", undefined, true);
    }
  }

  async function saveEdit(value: string | null, note: string | null, occurredAt: string) {
    if (!editing) return;
    if (editing.entry.pending) await store.flush();
    if (store.queue.some((q) => q.id === editing!.entry.id)) throw new Error('Not synced yet — try again when online.');
    await store.update(editing.entry.id, { value, note, occurred_at: occurredAt });
    editing = null;
  }

  async function deleteEdit() {
    if (!editing) return;
    if (!confirm(`Delete this ${editing.symptom.name} entry?`)) return;
    await store.remove(editing.entry.id);
    editing = null;
  }

  async function logFromSheet(value: string | null, note: string | null, occurredAt: string) {
    if (!logging) return;
    const s = logging;
    logging = null;
    await store.log(s, value, note, occurredAt);
  }
</script>

{#if !store.ready && !store.user}
  <div class="splash"></div>
{:else if !store.user}
  <LoginView />
{:else}
  <header class="top no-print">
    <h1>{tabs.find((t) => t.id === tab)?.label}</h1>
    <div class="status small">
      {#if !store.online}<span class="chip pending">offline</span>{/if}
      {#if store.queue.length}<span class="chip pending">{store.queue.length} to sync</span>{/if}
    </div>
  </header>

  <main>
    {#if tab === 'log'}
      <LogView onopen={(s) => (logging = s)} onedit={(id) => openEdit(id)} />
    {:else if tab === 'history'}
      <HistoryView onedit={(e) => openEdit(e)} />
    {:else if tab === 'summary'}
      <SummaryView />
    {:else}
      <SettingsView />
    {/if}
  </main>

  <nav class="no-print">
    {#each tabs as t}
      <a href="#{t.id}" class:active={tab === t.id} aria-current={tab === t.id ? 'page' : undefined}>
        <span class="ni">{t.icon}</span><span>{t.label}</span>
      </a>
    {/each}
  </nav>

  {#if store.toast}
    <div class="toast no-print" class:error={store.toast.error} role="status">
      <span>{store.toast.text}</span>
      {#if store.toast.undoId}
        {@const id = store.toast.undoId}
        <button onclick={() => { store.toast = null; openEdit(id); }}>Edit</button>
        <button onclick={() => store.undo(id)}>Undo</button>
      {/if}
    </div>
  {/if}

  {#if logging}
    <Sheet title="{logging.icon ?? ''} {logging.name}" onclose={() => (logging = null)}>
      {#key logging.id}
        <EntryForm symptom={logging} onsubmit={logFromSheet} />
      {/key}
    </Sheet>
  {/if}

  {#if editing}
    <Sheet title="Edit {editing.symptom.name}" onclose={() => (editing = null)}>
      {#key editing.entry.id}
        <EntryForm symptom={editing.symptom} entry={editing.entry} onsubmit={saveEdit} ondelete={deleteEdit} />
      {/key}
    </Sheet>
  {/if}
{/if}

<style>
  .splash { min-height: 100dvh; }
  .top {
    position: sticky; top: 0; z-index: 10; display: flex; align-items: center; justify-content: space-between;
    padding: calc(12px + var(--safe-t)) 18px 10px; background: color-mix(in srgb, var(--bg) 92%, transparent);
    backdrop-filter: blur(8px);
  }
  .status { display: flex; gap: 6px; }
  main { padding: 4px 16px calc(var(--nav-h) + var(--safe-b) + 90px); max-width: 640px; margin: 0 auto; }
  nav {
    position: fixed; left: 0; right: 0; bottom: 0; z-index: 20; display: flex;
    height: calc(var(--nav-h) + var(--safe-b)); padding-bottom: var(--safe-b);
    background: var(--surface); border-top: 1px solid var(--line);
  }
  nav a {
    flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 2px;
    color: var(--muted); text-decoration: none; font-size: 12px; font-weight: 600;
  }
  nav a.active { color: var(--accent); }
  .ni { font-size: 20px; filter: grayscale(1); opacity: 0.7; }
  nav a.active .ni { filter: none; opacity: 1; }
  .toast {
    position: fixed; left: 12px; right: 12px; bottom: calc(var(--nav-h) + var(--safe-b) + 12px); z-index: 30;
    max-width: 616px; margin: 0 auto; display: flex; align-items: center; gap: 4px;
    padding: 6px 6px 6px 16px; border-radius: 14px; background: #2b2520; color: #fff;
    box-shadow: 0 6px 24px rgb(0 0 0 / 25%); animation: pop 0.15s ease-out;
  }
  .toast.error { background: var(--bad); }
  .toast span { flex: 1; padding: 8px 0; }
  .toast button { color: #fdba74; font-weight: 700; padding: 10px 12px; }
  @keyframes pop { from { transform: translateY(10px); opacity: 0; } }
  @media print { main { padding: 0; } }
</style>
