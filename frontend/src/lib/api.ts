export type Kind = 'bool' | 'count' | 'scale' | 'choice' | 'number' | 'med' | 'note';

export const KIND_LABELS: Record<Kind, string> = {
  bool: 'Yes / it happened',
  count: 'Count (one tap = once)',
  scale: 'Scale (ordered options)',
  choice: 'Choice (options)',
  number: 'Number with unit',
  med: 'Medication dose',
  note: 'Note only',
};

export interface User {
  id: number;
  username: string;
  display_name: string;
}

/** Whoever is observed: a cat, or yourself. Shared between users or private. */
export interface Patient {
  id: number;
  name: string;
  icon: string | null;
  /** Display names of the other users who can see this patient. */
  shared_with: string[];
}

export interface TrackerOption {
  id: number;
  label: string;
  /** Hidden from logging; old entries still show it. */
  retired: boolean;
  /** The usual state: assumed on observed days without an entry. */
  normal: boolean;
  /** Number of entries using this option. */
  uses: number;
}

/** An option as sent to the server; no id = new option. */
export interface OptionInput {
  id?: number;
  label: string;
  retired: boolean;
  normal: boolean;
}

export const hasOptions = (kind: Kind | undefined) => kind === 'scale' || kind === 'choice';

/** The option assumed when nothing is logged on an observed day, if the tracker has one. */
export const normalOption = (t: Tracker) => t.options.find((o) => o.normal && !o.retired);

/** How trackers are grouped on screen; the kind decides the group. */
export const CATEGORIES: { label: string; kinds: Kind[] }[] = [
  { label: 'Medications', kinds: ['med'] },
  { label: 'Events', kinds: ['bool', 'count'] },
  { label: 'Observations', kinds: ['scale', 'choice'] },
  { label: 'Measurements', kinds: ['number'] },
  { label: 'Notes', kinds: ['note'] },
];

/** Trackers grouped by category, keeping their order inside each group; empty groups left out. */
export function byCategory(trackers: Tracker[]): { label: string; items: Tracker[] }[] {
  return CATEGORIES.map((c) => ({ label: c.label, items: trackers.filter((t) => c.kinds.includes(t.kind)) })).filter(
    (g) => g.items.length,
  );
}

export interface Tracker {
  id: number;
  name: string;
  kind: Kind;
  /** In order (scale order for scales), retired ones included. */
  options: TrackerOption[];
  unit: string | null;
  icon: string | null;
  pinned: boolean;
  archived: boolean;
  sort_order: number;
  /** Normal options aren't assumed for days before this. */
  created_at: string;
}

export interface Entry {
  id: string;
  tracker_id: number;
  tracker_name: string;
  kind: Kind;
  /** Set for scale/choice entries; `value` then carries the option's label. */
  option_id: number | null;
  value: string | null;
  note: string | null;
  logged_by: number | null;
  logged_by_name: string | null;
  occurred_at: string;
  created_at: string;
  /** Local only: not yet confirmed by the server. */
  pending?: boolean;
}

/** "Nothing unusual today": marks the day as observed, so normal options apply. */
export interface Checkin {
  id: string;
  logged_by: number | null;
  logged_by_name: string | null;
  occurred_at: string;
  created_at: string;
  /** Local only: not yet confirmed by the server. */
  pending?: boolean;
}

/** What the user picked: a free value, or an option for scale/choice. */
export interface EntryValue {
  value: string | null;
  option_id: number | null;
}

export type TrackerInput = Partial<Omit<Tracker, 'options'>> & { options?: OptionInput[] };

export interface NewEntry {
  id: string;
  tracker_id: number;
  value?: string | null;
  option_id?: number | null;
  note?: string | null;
  occurred_at: string;
}

export class ApiError extends Error {
  constructor(public status: number, message: string) {
    super(message);
  }
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch('/api' + path, {
    method,
    credentials: 'same-origin',
    headers: body !== undefined ? { 'Content-Type': 'application/json' } : undefined,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  if (!res.ok) {
    let msg = res.statusText;
    try {
      msg = (await res.json()).error ?? msg;
    } catch {
      /* not JSON */
    }
    throw new ApiError(res.status, msg);
  }
  if (res.status === 204) return undefined as T;
  return res.json() as Promise<T>;
}

/** Everything about trackers and entries lives under the patient it belongs to. */
const pp = (pid: number) => `/patients/${pid}`;

export const api = {
  me: () => request<User>('GET', '/me'),
  login: (username: string, password: string) =>
    request<User>('POST', '/auth/login', { username, password }),
  logout: () => request<void>('POST', '/auth/logout'),
  patients: () => request<Patient[]>('GET', '/patients'),

  trackers: (pid: number, includeArchived = false) =>
    request<Tracker[]>('GET', `${pp(pid)}/trackers${includeArchived ? '?include_archived=true' : ''}`),
  createTracker: (pid: number, s: TrackerInput) => request<Tracker>('POST', `${pp(pid)}/trackers`, s),
  updateTracker: (pid: number, id: number, s: TrackerInput) =>
    request<Tracker>('PATCH', `${pp(pid)}/trackers/${id}`, s),
  /** Move all entries of option `from` to option `into`, then delete `from`. */
  mergeOption: (pid: number, trackerId: number, from: number, into: number) =>
    request<Tracker>('POST', `${pp(pid)}/trackers/${trackerId}/options/${from}/merge`, { into }),
  reorderTrackers: (pid: number, ids: number[]) => request<void>('PUT', `${pp(pid)}/trackers/order`, ids),

  entries: (pid: number, params: { from?: string; to?: string; tracker_id?: number; limit?: number } = {}) => {
    const q = new URLSearchParams();
    for (const [k, v] of Object.entries(params)) if (v !== undefined) q.set(k, String(v));
    return request<Entry[]>('GET', `${pp(pid)}/entries?${q}`);
  },
  createEntry: (pid: number, e: NewEntry) => request<Entry>('POST', `${pp(pid)}/entries`, e),
  updateEntry: (pid: number, id: string, e: Partial<EntryValue> & { note?: string | null; occurred_at?: string }) =>
    request<Entry>('PATCH', `${pp(pid)}/entries/${id}`, e),
  deleteEntry: (pid: number, id: string) => request<void>('DELETE', `${pp(pid)}/entries/${id}`),

  checkins: (pid: number, params: { from?: string; to?: string } = {}) => {
    const q = new URLSearchParams();
    for (const [k, v] of Object.entries(params)) if (v !== undefined) q.set(k, String(v));
    return request<Checkin[]>('GET', `${pp(pid)}/checkins?${q}`);
  },
  createCheckin: (pid: number, c: { id: string; occurred_at: string }) =>
    request<Checkin>('POST', `${pp(pid)}/checkins`, c),
  deleteCheckin: (pid: number, id: string) => request<void>('DELETE', `${pp(pid)}/checkins/${id}`),
};

/** crypto.randomUUID only exists in secure contexts; fall back for plain-http LAN testing. */
export function uuid(): string {
  if (typeof crypto.randomUUID === 'function') return crypto.randomUUID();
  const b = crypto.getRandomValues(new Uint8Array(16));
  b[6] = (b[6] & 0x0f) | 0x40;
  b[8] = (b[8] & 0x3f) | 0x80;
  const h = [...b].map((x) => x.toString(16).padStart(2, '0')).join('');
  return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`;
}
