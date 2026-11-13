import { api, ApiError, uuid, type Entry, type NewEntry, type Symptom, type User } from './api';
import { startOfDay } from './time';

const LS = {
  user: 'ct.user',
  symptoms: 'ct.symptoms',
  recent: 'ct.recent',
  queue: 'ct.queue',
};

function load<T>(key: string, fallback: T): T {
  try {
    const v = localStorage.getItem(key);
    return v ? (JSON.parse(v) as T) : fallback;
  } catch {
    return fallback;
  }
}
function save(key: string, v: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(v));
  } catch {
    /* storage full or unavailable */
  }
}

/** How far back the home screen keeps entries in memory. */
const RECENT_DAYS = 3;

class Store {
  user = $state<User | null>(load(LS.user, null));
  symptoms = $state<Symptom[]>(load(LS.symptoms, []));
  recent = $state<Entry[]>(load(LS.recent, []));
  queue = $state<NewEntry[]>(load(LS.queue, []));
  online = $state(navigator.onLine);
  ready = $state(false);
  toast = $state<{ text: string; undoId?: string; error?: boolean } | null>(null);
  /** Bumped whenever entries change so other views can refetch. */
  version = $state(0);

  private toastTimer: ReturnType<typeof setTimeout> | undefined;

  pinned = $derived(this.symptoms.filter((s) => s.pinned));
  unpinned = $derived(this.symptoms.filter((s) => !s.pinned));

  async init() {
    window.addEventListener('online', () => {
      this.online = true;
      this.flush();
    });
    window.addEventListener('offline', () => (this.online = false));
    document.addEventListener('visibilitychange', () => {
      if (document.visibilityState === 'visible' && this.user) this.refresh();
    });
    setInterval(() => this.queue.length && this.flush(), 30_000);

    try {
      this.setUser(await api.me());
      await this.refresh();
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) this.setUser(null);
      // Network error: keep cached user and data, work offline.
    }
    this.ready = true;
  }

  private setUser(u: User | null) {
    this.user = u;
    save(LS.user, u);
    if (!u) {
      this.recent = [];
      save(LS.recent, []);
    }
  }

  async login(username: string, password: string) {
    this.setUser(await api.login(username, password));
    await this.refresh();
  }

  async logout() {
    try {
      await api.logout();
    } catch {
      /* offline: drop local session anyway */
    }
    this.setUser(null);
  }

  async refresh() {
    await this.flush();
    try {
      const [symptoms, recent] = await Promise.all([
        api.symptoms(),
        api.entries({ from: startOfDay(RECENT_DAYS).toISOString(), limit: 1000 }),
      ]);
      this.symptoms = symptoms;
      save(LS.symptoms, symptoms);
      this.setRecent(recent);
      this.version++;
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) this.setUser(null);
    }
  }

  async reloadSymptoms() {
    this.symptoms = await api.symptoms();
    save(LS.symptoms, this.symptoms);
  }

  private setRecent(serverEntries: Entry[]) {
    // Keep entries that are still waiting to sync on top.
    const pendingIds = new Set(this.queue.map((q) => q.id));
    const pending = this.recent.filter((e) => pendingIds.has(e.id));
    const merged = [...pending, ...serverEntries.filter((e) => !pendingIds.has(e.id))];
    merged.sort((a, b) => b.occurred_at.localeCompare(a.occurred_at));
    this.recent = merged;
    save(LS.recent, merged);
  }

  lastFor(symptomId: number): Entry | undefined {
    return this.recent.find((e) => e.symptom_id === symptomId);
  }

  todayCount(symptomId: number): number {
    const start = startOfDay(0).toISOString();
    return this.recent.filter((e) => e.symptom_id === symptomId && e.occurred_at >= start).length;
  }

  /** Log an entry. Shows immediately; syncs now or when back online. */
  async log(sym: Symptom, value: string | null, note: string | null = null, occurredAt?: string) {
    const e: NewEntry = {
      id: uuid(),
      symptom_id: sym.id,
      value,
      note,
      occurred_at: occurredAt ?? new Date().toISOString(),
    };
    const local: Entry = {
      ...e,
      value: value ?? (sym.kind === 'bool' ? 'yes' : sym.kind === 'count' ? '1' : null),
      note,
      symptom_name: sym.name,
      kind: sym.kind,
      logged_by: this.user?.id ?? null,
      logged_by_name: this.user?.display_name ?? null,
      created_at: e.occurred_at,
      pending: true,
    };
    this.recent = [local, ...this.recent].sort((a, b) => b.occurred_at.localeCompare(a.occurred_at));
    this.queue = [...this.queue, e];
    save(LS.queue, this.queue);
    save(LS.recent, this.recent);

    const label = value && sym.kind !== 'count' && sym.kind !== 'bool' ? `${sym.name}: ${value}` : sym.name;
    this.showToast(`Logged ${label}`, e.id);
    await this.flush();
  }

  private flushing = false;
  async flush() {
    if (this.flushing || !this.queue.length) return;
    this.flushing = true;
    try {
      while (this.queue.length) {
        const next = this.queue[0];
        try {
          const saved = await api.createEntry(next);
          if (!this.queue.some((q) => q.id === next.id)) {
            // Undone while it was being sent: remove it on the server too.
            await api.deleteEntry(saved.id).catch(() => {});
          } else {
            this.recent = this.recent.map((r) => (r.id === saved.id ? saved : r));
          }
          this.version++;
        } catch (err) {
          if (err instanceof ApiError && err.status === 400) {
            // Server rejected it (e.g. symptom changed); drop it so the queue isn't stuck.
            this.recent = this.recent.filter((r) => r.id !== next.id);
            this.showToast(`Couldn't save: ${err.message}`, undefined, true);
          } else {
            if (err instanceof ApiError && err.status === 401) this.setUser(null);
            break; // offline or server down: try later
          }
        }
        this.queue = this.queue.filter((q) => q.id !== next.id);
        save(LS.queue, this.queue);
      }
    } finally {
      this.flushing = false;
      save(LS.recent, this.recent);
    }
  }

  async remove(id: string) {
    const queued = this.queue.some((q) => q.id === id);
    if (queued) {
      this.queue = this.queue.filter((q) => q.id !== id);
      save(LS.queue, this.queue);
    } else {
      await api.deleteEntry(id);
    }
    this.recent = this.recent.filter((r) => r.id !== id);
    save(LS.recent, this.recent);
    this.version++;
  }

  async update(id: string, patch: { value?: string | null; note?: string | null; occurred_at?: string }) {
    const saved = await api.updateEntry(id, patch);
    this.recent = this.recent
      .map((r) => (r.id === id ? saved : r))
      .sort((a, b) => b.occurred_at.localeCompare(a.occurred_at));
    save(LS.recent, this.recent);
    this.version++;
    return saved;
  }

  showToast(text: string, undoId?: string, error = false) {
    clearTimeout(this.toastTimer);
    this.toast = { text, undoId, error };
    this.toastTimer = setTimeout(() => (this.toast = null), error ? 6000 : 4000);
  }

  async undo(id: string) {
    this.toast = null;
    try {
      await this.remove(id);
    } catch {
      this.showToast("Couldn't undo — try from History", undefined, true);
    }
  }
}

export const store = new Store();
