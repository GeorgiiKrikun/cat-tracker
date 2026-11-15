<script lang="ts">
  import { api, type Checkin, type Entry, type EntryValue, type Tracker } from './lib/api';
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
  let logging = $state<Tracker | null>(null);
  let editing = $state<{ entry: Entry; tracker: Tracker } | null>(null);

  async function trackerFor(e: Entry): Promise<Tracker> {
    const s = store.trackers.find((x) => x.id === e.tracker_id);
    if (s) return s;
    const all = await api.trackers(store.patientId!, true);
    const found = all.find((x) => x.id === e.tracker_id);
    if (!found) throw new Error('tracker not found');
    return found;
  }

  async function openEdit(e: Entry | string) {
    store.toast = null;
    const entry = typeof e === 'string' ? store.recent.find((r) => r.id === e) : e;
    if (!entry) return;
    try {
      editing = { entry, tracker: await trackerFor(entry) };
    } catch {
      store.showToast("Can't edit while offline", undefined, true);
    }
  }

  async function saveEdit(value: EntryValue, note: string | null, occurredAt: string) {
    if (!editing) return;
    if (editing.entry.pending) await store.flush();
    if (store.queue.some((q) => q.id === editing!.entry.id)) throw new Error('Not synced yet — try again when online.');
    await store.update(editing.entry.id, { ...value, note, occurred_at: occurredAt });
    editing = null;
  }

  async function deleteEdit() {
    if (!editing) return;
    if (!confirm(`Delete this ${editing.tracker.name} entry?`)) return;
    await store.remove(editing.entry.id);
    editing = null;
  }

  async function removeCheckin(c: Checkin) {
    if (!confirm('Remove the “nothing unusual” check-in?')) return;
    try {
      await store.removeCheckin(c.id);
    } catch {
      store.showToast("Couldn't remove it — are you online?", undefined, true);
    }
  }

  async function logFromSheet(value: EntryValue, note: string | null, occurredAt: string) {
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
    <div class="titles">
      <h1>{tabs.find((t) => t.id === tab)?.label}</h1>
      {#if store.patients.length > 1}
        <select
          class="patient"
          aria-label="Patient"
          value={store.patientId}
          onchange={(e) => store.selectPatient(Number(e.currentTarget.value))}
        >
          {#each store.patients as p (p.id)}
            <option value={p.id}>{[p.icon, p.name].filter(Boolean).join(' ')}</option>
          {/each}
        </select>
      {:else if store.patient}
        <span class="patient-name small muted">{[store.patient.icon, store.patient.name].filter(Boolean).join(' ')}</span>
      {/if}
    </div>
    <div class="status small">
      {#if !store.online}<span class="chip pending">offline</span>{/if}
      {#if store.queue.length}<span class="chip pending">{store.queue.length} to sync</span>{/if}
    </div>
  </header>

  <main>
    {#if !store.patient && tab !== 'settings'}
      <div class="card empty">
        <p><b>No patients yet.</b></p>
        <p class="muted">
          Someone with access to the server needs to create one and share it with you, e.g.
          <code>cattracker add-patient Murka --preset cat</code> and
          <code>cattracker share Murka {store.user.username}</code>.
        </p>
      </div>
    {:else if tab === 'log'}
      <LogView onopen={(t) => (logging = t)} onedit={(e) => openEdit(e)} oncheckin={removeCheckin} />
    {:else if tab === 'history'}
      <HistoryView onedit={(e) => openEdit(e)} oncheckin={removeCheckin} />
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
        {@const checkin = store.toast.checkin}
        {#if !checkin}<button onclick={() => openEdit(id)}>Edit</button>{/if}
        <button onclick={() => store.undo(id, checkin)}>Undo</button>
      {/if}
    </div>
  {/if}

  {#if logging}
    <Sheet title="{logging.icon ?? ''} {logging.name}" onclose={() => (logging = null)}>
      {#key logging.id}
        <EntryForm tracker={logging} onsubmit={logFromSheet} />
      {/key}
    </Sheet>
  {/if}

  {#if editing}
    <Sheet title="Edit {editing.tracker.name}" onclose={() => (editing = null)}>
      {#key editing.entry.id}
        <EntryForm tracker={editing.tracker} entry={editing.entry} onsubmit={saveEdit} ondelete={deleteEdit} />
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
  .titles { display: flex; align-items: baseline; gap: 10px; min-width: 0; }
  .patient-name { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .patient {
    width: auto; max-width: 50vw; padding: 4px 10px; border-radius: 99px; font-size: 14px; font-weight: 600;
    background: var(--surface-2); border: 0;
  }
  .empty { padding: 4px 16px; margin-top: 8px; }
  .empty code { background: var(--surface-2); padding: 1px 5px; border-radius: 5px; font-size: 13px; }
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
