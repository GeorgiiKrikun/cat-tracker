import {
  api,
  ApiError,
  uuid,
  type Checkin,
  type Entry,
  type EntryValue,
  type NewEntry,
  type Patient,
  type Tracker,
  type User,
} from './api';
import { startOfDay } from './time';

const LS = {
  user: 'ct.user',
  patients: 'ct.patients',
  patient: 'ct.patient',
  // Cached per patient, so switching works offline and never shows another patient's data.
  trackers: (pid: number) => `ct.trackers.${pid}`,
  recent: (pid: number) => `ct.recent.${pid}`,
  checkins: (pid: number) => `ct.checkins.${pid}`,
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
      if (/^ct\.(trackers|recent|checkins)\./.test(k)) localStorage.removeItem(k);
    }
    localStorage.removeItem(LS.patients);
  } catch {
    /* storage unavailable */
  }
}

/** An entry or check-in waiting to be sent, remembering which patient it belongs to. */
type Queued =
  | (NewEntry & { type?: 'entry'; patient_id: number })
  | { type: 'checkin'; id: string; occurred_at: string; patient_id: number };

/** How far back the home screen keeps entries in memory. */
const RECENT_DAYS = 3;

const isToday = (iso: string) => iso >= startOfDay(0).toISOString();

const initialPatient = load<number | null>(LS.patient, null);

class Store {
  user = $state<User | null>(load(LS.user, null));
  patients = $state<Patient[]>(load(LS.patients, []));
  patientId = $state<number | null>(initialPatient);
  trackers = $state<Tracker[]>(initialPatient === null ? [] : load(LS.trackers(initialPatient), []));
  recent = $state<Entry[]>(initialPatient === null ? [] : load(LS.recent(initialPatient), []));
  checkins = $state<Checkin[]>(initialPatient === null ? [] : load(LS.checkins(initialPatient), []));
  queue = $state<Queued[]>(load(LS.queue, []));
  online = $state(navigator.onLine);
  ready = $state(false);
  /** `undoId` is an entry (with Edit and Undo) or, with `checkin`, a check-in (Undo only). */
  toast = $state<{ text: string; undoId?: string; checkin?: boolean; error?: boolean } | null>(null);
  /** Ticks every 30 s so "today" moves on after midnight; see init(). */
  private clock = $state(0);
  /** Bumped whenever entries change so other views can refetch. */
  version = $state(0);

  private toastTimer: ReturnType<typeof setTimeout> | undefined;

  patient = $derived(this.patients.find((p) => p.id === this.patientId) ?? null);
  pinned = $derived(this.trackers.filter((s) => s.pinned));
  unpinned = $derived(this.trackers.filter((s) => !s.pinned));
  /** Today's check-in, if someone marked today as "nothing unusual". */
  checkinToday = $derived.by(() => {
    void this.clock;
    return this.checkins.find((c) => isToday(c.occurred_at));
  });
  /** Someone logged anything today, so trackers with a normal option count as normal. */
  observedToday = $derived.by(() => {
    void this.clock;
    return !!this.checkinToday || this.recent.some((e) => isToday(e.occurred_at));
  });

  async init() {
    window.addEventListener('online', () => {
      this.online = true;
      this.flush();
    });
    window.addEventListener('offline', () => (this.online = false));
    document.addEventListener('visibilitychange', () => {
      if (document.visibilityState === 'visible' && this.user) this.refresh();
    });
    setInterval(() => {
      this.clock++;
      if (this.queue.length) this.flush();
    }, 30_000);

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
      this.trackers = [];
      this.recent = [];
      this.checkins = [];
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
    this.trackers = load(LS.trackers(id), []);
    this.recent = load(LS.recent(id), []);
    this.checkins = load(LS.checkins(id), []);
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
        this.trackers = [];
        this.recent = [];
        this.checkins = [];
      }
      const pid = this.patientId;
      if (pid === null) return;
      const from = startOfDay(RECENT_DAYS).toISOString();
      const [trackers, recent, checkins] = await Promise.all([
        api.trackers(pid),
        api.entries(pid, { from, limit: 1000 }),
        api.checkins(pid, { from }),
      ]);
      if (pid !== this.patientId) return; // switched meanwhile
      this.trackers = trackers;
      save(LS.trackers(pid), trackers);
      this.setRecent(recent);
      this.setCheckins(checkins);
      this.version++;
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) this.setUser(null);
    }
  }

  async reloadTrackers() {
    const pid = this.patientId;
    if (pid === null) return;
    this.trackers = await api.trackers(pid);
    save(LS.trackers(pid), this.trackers);
  }

  private saveRecent() {
    if (this.patientId !== null) save(LS.recent(this.patientId), this.recent);
  }

  private saveCheckins() {
    if (this.patientId !== null) save(LS.checkins(this.patientId), this.checkins);
  }

  private setCheckins(server: Checkin[]) {
    const pendingIds = new Set(this.queue.map((q) => q.id));
    const pending = this.checkins.filter((c) => pendingIds.has(c.id));
    this.checkins = [...pending, ...server.filter((c) => !pendingIds.has(c.id))].sort((a, b) =>
      b.occurred_at.localeCompare(a.occurred_at),
    );
    this.saveCheckins();
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

  lastFor(trackerId: number): Entry | undefined {
    return this.recent.find((e) => e.tracker_id === trackerId);
  }

  todayCount(trackerId: number): number {
    void this.clock;
    return this.recent.filter((e) => e.tracker_id === trackerId && isToday(e.occurred_at)).length;
  }

  /** "Nothing unusual today": the day counts as observed, normal options apply. */
  async checkIn() {
    if (this.patientId === null || this.checkinToday) return;
    const c: Checkin = {
      id: uuid(),
      occurred_at: new Date().toISOString(),
      logged_by: this.user?.id ?? null,
      logged_by_name: this.user?.display_name ?? null,
      created_at: new Date().toISOString(),
      pending: true,
    };
    this.checkins = [c, ...this.checkins];
    this.queue = [...this.queue, { type: 'checkin', id: c.id, occurred_at: c.occurred_at, patient_id: this.patientId }];
    save(LS.queue, this.queue);
    this.saveCheckins();
    this.showToast('Marked today as nothing unusual', c.id, false, true);
    await this.flush();
  }

  async removeCheckin(id: string) {
    if (this.queue.some((q) => q.id === id)) {
      this.queue = this.queue.filter((q) => q.id !== id);
      save(LS.queue, this.queue);
    } else if (this.patientId !== null) {
      await api.deleteCheckin(this.patientId, id);
    }
    this.checkins = this.checkins.filter((c) => c.id !== id);
    this.saveCheckins();
    this.version++;
  }

  /** Log an entry for the current patient. Shows immediately; syncs now or when back online. */
  async log(tracker: Tracker, { value, option_id }: EntryValue, note: string | null = null, occurredAt?: string) {
    if (this.patientId === null) return;
    const e: Queued = {
      id: uuid(),
      patient_id: this.patientId,
      tracker_id: tracker.id,
      value,
      option_id,
      note,
      occurred_at: occurredAt ?? new Date().toISOString(),
    };
    // Shown until the server confirms it, so fill in what the server would.
    const shown =
      option_id !== null
        ? (tracker.options.find((o) => o.id === option_id)?.label ?? null)
        : (value ?? (tracker.kind === 'bool' ? 'yes' : tracker.kind === 'count' ? '1' : null));
    const local: Entry = {
      id: e.id,
      tracker_id: tracker.id,
      option_id,
      value: shown,
      note,
      occurred_at: e.occurred_at,
      tracker_name: tracker.name,
      kind: tracker.kind,
      logged_by: this.user?.id ?? null,
      logged_by_name: this.user?.display_name ?? null,
      created_at: e.occurred_at,
      pending: true,
    };
    this.recent = [local, ...this.recent].sort((a, b) => b.occurred_at.localeCompare(a.occurred_at));
    this.queue = [...this.queue, e];
    save(LS.queue, this.queue);
    this.saveRecent();

    const label = shown && tracker.kind !== 'count' && tracker.kind !== 'bool' ? `${tracker.name}: ${shown}` : tracker.name;
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
        const { patient_id } = next;
        const stillQueued = () => this.queue.some((q) => q.id === next.id);
        try {
          if (next.type === 'checkin') {
            const saved = await api.createCheckin(patient_id, { id: next.id, occurred_at: next.occurred_at });
            if (!stillQueued()) await api.deleteCheckin(patient_id, saved.id).catch(() => {});
            else if (patient_id === this.patientId) this.checkins = this.checkins.map((c) => (c.id === saved.id ? saved : c));
          } else {
            const { patient_id: _, type: __, ...entry } = next;
            const saved = await api.createEntry(patient_id, entry);
            if (!stillQueued()) {
              // Undone while it was being sent: remove it on the server too.
              await api.deleteEntry(patient_id, saved.id).catch(() => {});
            } else if (patient_id === this.patientId) {
              this.recent = this.recent.map((r) => (r.id === saved.id ? saved : r));
            }
          }
          this.version++;
        } catch (err) {
          if (err instanceof ApiError && (err.status === 400 || err.status === 404)) {
            // Server rejected it (tracker changed, access removed); drop it so the queue isn't stuck.
            this.recent = this.recent.filter((r) => r.id !== next.id);
            this.checkins = this.checkins.filter((c) => c.id !== next.id);
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
      this.saveCheckins();
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

  showToast(text: string, undoId?: string, error = false, checkin = false) {
    clearTimeout(this.toastTimer);
    this.toast = { text, undoId, error, checkin };
    this.toastTimer = setTimeout(() => (this.toast = null), error ? 6000 : 4000);
  }

  async undo(id: string, checkin = false) {
    this.toast = null;
    try {
      await (checkin ? this.removeCheckin(id) : this.remove(id));
    } catch {
      this.showToast("Couldn't undo — try from History", undefined, true);
    }
  }
}

export const store = new Store();
