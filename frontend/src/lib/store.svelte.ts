import {
  api,
  ApiError,
  uuid,
  type Entry,
  type EntryValue,
  type NewEntry,
  type Patient,
  type Symptom,
  type User,
} from './api';
import { startOfDay } from './time';

const LS = {
  user: 'ct.user',
  patients: 'ct.patients',
  patient: 'ct.patient',
  // Cached per patient, so switching works offline and never shows another patient's data.
  symptoms: (pid: number) => `ct.symptoms.${pid}`,
  recent: (pid: number) => `ct.recent.${pid}`,
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
function forgetCaches() {
  try {
    for (const k of Object.keys(localStorage)) {
      if (k.startsWith('ct.symptoms.') || k.startsWith('ct.recent.')) localStorage.removeItem(k);
    }
    localStorage.removeItem(LS.patients);
  } catch {
    /* storage unavailable */
  }
}

/** An entry waiting to be sent, remembering which patient it belongs to. */
type QueuedEntry = NewEntry & { patient_id: number };

/** How far back the home screen keeps entries in memory. */
const RECENT_DAYS = 3;

const initialPatient = load<number | null>(LS.patient, null);

class Store {
  user = $state<User | null>(load(LS.user, null));
  patients = $state<Patient[]>(load(LS.patients, []));
  patientId = $state<number | null>(initialPatient);
  symptoms = $state<Symptom[]>(initialPatient === null ? [] : load(LS.symptoms(initialPatient), []));
  recent = $state<Entry[]>(initialPatient === null ? [] : load(LS.recent(initialPatient), []));
  queue = $state<QueuedEntry[]>(load(LS.queue, []));
  online = $state(navigator.onLine);
  ready = $state(false);
  toast = $state<{ text: string; undoId?: string; error?: boolean } | null>(null);
  /** Bumped whenever entries change so other views can refetch. */
  version = $state(0);

  private toastTimer: ReturnType<typeof setTimeout> | undefined;

  patient = $derived(this.patients.find((p) => p.id === this.patientId) ?? null);
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
    const changed = u?.id !== this.user?.id;
    this.user = u;
    save(LS.user, u);
    if (changed) {
      // Another person on this device must not see the previous one's patients.
      forgetCaches();
      this.patients = [];
      this.symptoms = [];
      this.recent = [];
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

  /** Switch to another patient: show their cached data at once, then refresh. */
  async selectPatient(id: number) {
    if (id === this.patientId) return;
    this.patientId = id;
    save(LS.patient, id);
    this.symptoms = load(LS.symptoms(id), []);
    this.recent = load(LS.recent(id), []);
    this.version++;
    await this.refresh();
  }

  async refresh() {
    await this.flush();
    try {
      this.patients = await api.patients();
      save(LS.patients, this.patients);
      if (!this.patients.some((p) => p.id === this.patientId)) {
        this.patientId = this.patients[0]?.id ?? null;
        save(LS.patient, this.patientId);
        this.symptoms = [];
        this.recent = [];
      }
      const pid = this.patientId;
      if (pid === null) return;
      const [symptoms, recent] = await Promise.all([
        api.symptoms(pid),
        api.entries(pid, { from: startOfDay(RECENT_DAYS).toISOString(), limit: 1000 }),
      ]);
      if (pid !== this.patientId) return; // switched meanwhile
      this.symptoms = symptoms;
      save(LS.symptoms(pid), symptoms);
      this.setRecent(recent);
      this.version++;
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) this.setUser(null);
    }
  }

  async reloadSymptoms() {
    const pid = this.patientId;
    if (pid === null) return;
    this.symptoms = await api.symptoms(pid);
    save(LS.symptoms(pid), this.symptoms);
  }

  private saveRecent() {
    if (this.patientId !== null) save(LS.recent(this.patientId), this.recent);
  }

  private setRecent(serverEntries: Entry[]) {
    // Keep entries that are still waiting to sync on top.
    const pendingIds = new Set(this.queue.map((q) => q.id));
    const pending = this.recent.filter((e) => pendingIds.has(e.id));
    const merged = [...pending, ...serverEntries.filter((e) => !pendingIds.has(e.id))];
    merged.sort((a, b) => b.occurred_at.localeCompare(a.occurred_at));
    this.recent = merged;
    this.saveRecent();
  }

  lastFor(symptomId: number): Entry | undefined {
    return this.recent.find((e) => e.symptom_id === symptomId);
  }

  todayCount(symptomId: number): number {
    const start = startOfDay(0).toISOString();
    return this.recent.filter((e) => e.symptom_id === symptomId && e.occurred_at >= start).length;
  }

  /** Log an entry for the current patient. Shows immediately; syncs now or when back online. */
  async log(sym: Symptom, { value, option_id }: EntryValue, note: string | null = null, occurredAt?: string) {
    if (this.patientId === null) return;
    const e: QueuedEntry = {
      id: uuid(),
      patient_id: this.patientId,
      symptom_id: sym.id,
      value,
      option_id,
      note,
      occurred_at: occurredAt ?? new Date().toISOString(),
    };
    // Shown until the server confirms it, so fill in what the server would.
    const shown =
      option_id !== null
        ? (sym.options.find((o) => o.id === option_id)?.label ?? null)
        : (value ?? (sym.kind === 'bool' ? 'yes' : sym.kind === 'count' ? '1' : null));
    const local: Entry = {
      id: e.id,
      symptom_id: sym.id,
      option_id,
      value: shown,
      note,
      occurred_at: e.occurred_at,
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
    this.saveRecent();

    const label = shown && sym.kind !== 'count' && sym.kind !== 'bool' ? `${sym.name}: ${shown}` : sym.name;
    this.showToast(`Logged ${label}`, e.id);
    await this.flush();
  }

  private flushing = false;
  async flush() {
    if (this.flushing || !this.queue.length) return;
    this.flushing = true;
    try {
      while (this.queue.length) {
        const { patient_id, ...next } = this.queue[0];
        try {
          const saved = await api.createEntry(patient_id, next);
          if (!this.queue.some((q) => q.id === next.id)) {
            // Undone while it was being sent: remove it on the server too.
            await api.deleteEntry(patient_id, saved.id).catch(() => {});
          } else if (patient_id === this.patientId) {
            this.recent = this.recent.map((r) => (r.id === saved.id ? saved : r));
          }
          this.version++;
        } catch (err) {
          if (err instanceof ApiError && (err.status === 400 || err.status === 404)) {
            // Server rejected it (symptom changed, access removed); drop it so the queue isn't stuck.
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
      this.saveRecent();
    }
  }

  async remove(id: string) {
    const queued = this.queue.find((q) => q.id === id);
    if (queued) {
      this.queue = this.queue.filter((q) => q.id !== id);
      save(LS.queue, this.queue);
    } else if (this.patientId !== null) {
      await api.deleteEntry(this.patientId, id);
    }
    this.recent = this.recent.filter((r) => r.id !== id);
    this.saveRecent();
    this.version++;
  }

  async update(id: string, patch: Partial<EntryValue> & { note?: string | null; occurred_at?: string }) {
    if (this.patientId === null) throw new Error('No patient selected');
    const saved = await api.updateEntry(this.patientId, id, patch);
    this.recent = this.recent
      .map((r) => (r.id === id ? saved : r))
      .sort((a, b) => b.occurred_at.localeCompare(a.occurred_at));
    this.saveRecent();
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
