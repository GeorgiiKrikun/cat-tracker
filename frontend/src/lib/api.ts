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

export interface Symptom {
  id: number;
  name: string;
  kind: Kind;
  options: string[];
  unit: string | null;
  icon: string | null;
  pinned: boolean;
  archived: boolean;
  sort_order: number;
}

export interface Entry {
  id: string;
  symptom_id: number;
  symptom_name: string;
  kind: Kind;
  value: string | null;
  note: string | null;
  logged_by: number | null;
  logged_by_name: string | null;
  occurred_at: string;
  created_at: string;
  /** Local only: not yet confirmed by the server. */
  pending?: boolean;
}

export interface NewEntry {
  id: string;
  symptom_id: number;
  value?: string | null;
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

export const api = {
  me: () => request<User>('GET', '/me'),
  login: (username: string, password: string) =>
    request<User>('POST', '/auth/login', { username, password }),
  logout: () => request<void>('POST', '/auth/logout'),

  symptoms: (includeArchived = false) =>
    request<Symptom[]>('GET', `/symptoms${includeArchived ? '?include_archived=true' : ''}`),
  createSymptom: (s: Partial<Symptom>) => request<Symptom>('POST', '/symptoms', s),
  updateSymptom: (id: number, s: Partial<Symptom>) =>
    request<Symptom>('PATCH', `/symptoms/${id}`, s),
  reorderSymptoms: (ids: number[]) => request<void>('PUT', '/symptoms/order', ids),

  entries: (params: { from?: string; to?: string; symptom_id?: number; limit?: number } = {}) => {
    const q = new URLSearchParams();
    for (const [k, v] of Object.entries(params)) if (v !== undefined) q.set(k, String(v));
    return request<Entry[]>('GET', `/entries?${q}`);
  },
  createEntry: (e: NewEntry) => request<Entry>('POST', '/entries', e),
  updateEntry: (id: string, e: { value?: string | null; note?: string | null; occurred_at?: string }) =>
    request<Entry>('PATCH', `/entries/${id}`, e),
  deleteEntry: (id: string) => request<void>('DELETE', `/entries/${id}`),
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
