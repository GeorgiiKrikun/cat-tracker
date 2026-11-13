<script lang="ts">
  import { untrack } from 'svelte';
  import { hasOptions as kindHasOptions, type Entry, type EntryValue, type Tracker } from './api';
  import { fromLocalInput, toLocalInput } from './time';

  let {
    tracker,
    entry = null,
    onsubmit,
    ondelete,
  }: {
    tracker: Tracker;
    /** When given, the form edits this entry instead of creating one. */
    entry?: Entry | null;
    onsubmit: (value: EntryValue, note: string | null, occurredAt: string) => void | Promise<void>;
    ondelete?: () => void | Promise<void>;
  } = $props();

  // The form is re-created ({#key}) per entry, so initial values are all we need.
  const init = untrack(() => ({ entry, tracker }));
  const editing = !!init.entry;
  const kind = init.tracker.kind;

  const hasOptions = kindHasOptions(kind);
  let value = $state<string>(hasOptions ? '' : (init.entry?.value ?? (kind === 'med' ? (init.tracker.unit ?? '') : '')));
  let optionId = $state<number | null>(init.entry?.option_id ?? null);
  // Retired options aren't offered, except the one this entry already has.
  const options = init.tracker.options.filter((o) => !o.retired || o.id === init.entry?.option_id);
  let note = $state(init.entry?.note ?? '');
  let when = $state(toLocalInput(init.entry?.occurred_at ?? new Date().toISOString()));
  let details = $state(editing || kind === 'note');
  let busy = $state(false);
  let error = $state('');

  const needsSubmit = $derived(editing || details || !hasOptions);

  async function submit(v: string | null = value, opt: number | null = optionId) {
    error = '';
    if (kind === 'number' && (v === null || v.trim() === '' || isNaN(Number(v.replace(',', '.'))))) {
      error = 'Enter a number';
      return;
    }
    if (hasOptions && opt === null) {
      error = 'Pick one';
      return;
    }
    if (kind === 'note' && !note.trim()) {
      error = 'Write something';
      return;
    }
    busy = true;
    try {
      const val = kind === 'note' || hasOptions ? null : v && v.trim() !== '' ? v.trim() : null;
      await onsubmit({ value: val, option_id: hasOptions ? opt : null }, note.trim() || null, fromLocalInput(when));
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  function pick(id: number) {
    optionId = id;
    if (!needsSubmit) submit(null, id);
  }
</script>

<div class="form">
  {#if hasOptions}
    <div class="options" class:scale={kind === 'scale'}>
      {#each options as opt (opt.id)}
        <button
          class="opt"
          class:selected={optionId === opt.id}
          class:retired={opt.retired}
          onclick={() => pick(opt.id)}
          disabled={busy}>{opt.label}{#if opt.retired}<span class="small muted"> (retired)</span>{/if}</button
        >
      {/each}
    </div>
  {:else if kind === 'number'}
    <label for="num">Value{tracker.unit ? ` (${tracker.unit})` : ''}</label>
    <!-- svelte-ignore a11y_autofocus -->
    <input id="num" inputmode="decimal" bind:value autofocus placeholder={tracker.unit ?? ''} />
  {:else if kind === 'med'}
    <label for="dose">Dose</label>
    <input id="dose" bind:value placeholder="e.g. ½ tablet" />
  {:else if kind === 'count'}
    <label for="cnt">How many times</label>
    <input id="cnt" inputmode="numeric" bind:value placeholder="1" />
  {/if}

  {#if details}
    <label for="note">Note{kind === 'note' ? '' : ' (optional)'}</label>
    <textarea id="note" bind:value={note} placeholder={kind === 'note' ? 'What did you notice?' : ''}></textarea>
    <label for="when">When</label>
    <input id="when" type="datetime-local" bind:value={when} />
  {:else}
    <button class="more" onclick={() => (details = true)}>+ Add note or change time</button>
  {/if}

  {#if error}<p class="error">{error}</p>{/if}

  {#if needsSubmit}
    <button class="btn primary block submit" onclick={() => submit()} disabled={busy}>
      {editing ? 'Save' : kind === 'med' ? 'Mark as given' : 'Log'}
    </button>
  {/if}
  {#if editing && ondelete}
    <button class="btn danger block" onclick={ondelete} disabled={busy}>Delete entry</button>
  {/if}
  {#if entry?.logged_by_name}
    <p class="muted small center">Logged by {entry.logged_by_name}</p>
  {/if}
</div>

<style>
  .options { display: grid; grid-template-columns: repeat(auto-fit, minmax(120px, 1fr)); gap: 10px; margin-top: 6px; }
  .opt {
    min-height: 60px; border-radius: 14px; background: var(--surface); box-shadow: var(--shadow);
    font-weight: 600; font-size: 17px; border: 2px solid transparent; text-transform: capitalize;
  }
  .opt.selected { border-color: var(--accent); background: var(--accent-soft); }
  .opt.retired { font-weight: 500; font-style: italic; }
  .more { color: var(--accent); font-weight: 600; padding: 16px 0 4px; }
  .submit { margin-top: 18px; }
  .btn.danger { margin-top: 10px; }
  .error { color: var(--bad); margin: 10px 0 0; }
  .center { text-align: center; }
</style>
