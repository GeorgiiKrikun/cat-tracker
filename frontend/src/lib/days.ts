import { normalOption, type Checkin, type Entry, type Tracker } from './api';
import { dayKey } from './time';

/**
 * Days on which someone logged anything for the patient (an entry or a
 * check-in). Only on these does "nothing logged" mean "nothing happened" or
 * "normal"; on other days nobody looked.
 */
export function observedDays(entries: Entry[], checkins: Checkin[]): Set<string> {
  return new Set([...entries, ...checkins].map((x) => dayKey(x.occurred_at)));
}

/**
 * Of `days`, those where the tracker's normal option is assumed: observed,
 * nothing logged for this tracker, and the tracker already existed and is
 * still in use.
 */
export function assumedDays(tracker: Tracker, days: string[], observed: Set<string>, logged: Set<string>): string[] {
  if (!normalOption(tracker) || tracker.archived) return [];
  const since = dayKey(tracker.created_at);
  return days.filter((d) => d >= since && observed.has(d) && !logged.has(d));
}

/** Local 24h time, e.g. 09:05, so CSV rows sort as text whatever the locale. */
function hhmm(iso: string): string {
  const d = new Date(iso);
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
}

function csvField(s: string): string {
  return /[",\n\r]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
}

/** CSV of what the summary shows: logged entries, check-ins and assumed normal days, oldest first. */
export function summaryCsv(days: string[], trackers: Tracker[], entries: Entry[], checkins: Checkin[]): string {
  const rows: string[][] = [];
  for (const e of entries) {
    rows.push([dayKey(e.occurred_at), hhmm(e.occurred_at), e.tracker_name, e.value ?? '', e.note ?? '', e.logged_by_name ?? '', 'logged']);
  }
  for (const c of checkins) {
    rows.push([dayKey(c.occurred_at), hhmm(c.occurred_at), '(check-in)', 'nothing unusual', '', c.logged_by_name ?? '', 'check-in']);
  }
  const observed = observedDays(entries, checkins);
  for (const t of trackers) {
    const logged = new Set(entries.filter((e) => e.tracker_id === t.id).map((e) => dayKey(e.occurred_at)));
    const normal = normalOption(t);
    for (const d of assumedDays(t, days, observed, logged)) {
      rows.push([d, '', t.name, normal!.label, '', '', 'assumed (nothing logged)']);
    }
  }
  // Dates and 24h times sort as text; assumed rows (no time) come first in their day.
  rows.sort((a, b) => a[0].localeCompare(b[0]) || a[1].localeCompare(b[1]));
  const header = ['date', 'time', 'tracker', 'value', 'note', 'logged_by', 'source'];
  return [header, ...rows].map((r) => r.map(csvField).join(',')).join('\n') + '\n';
}
