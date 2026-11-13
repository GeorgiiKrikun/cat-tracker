<script lang="ts">
  import { untrack } from 'svelte';
  import type { Entry, Symptom } from './api';
  import { fromLocalInput, toLocalInput } from './time';

  let {
    symptom,
    entry = null,
    onsubmit,
    ondelete,
  }: {
    symptom: Symptom;
    /** When given, the form edits this entry instead of creating one. */
    entry?: Entry | null;
    onsubmit: (value: string | null, note: string | null, occurredAt: string) => void | Promise<void>;
    ondelete?: () => void | Promise<void>;
  } = $props();

  // The form is re-created ({#key}) per entry, so initial values are all we need.
  const init = untrack(() => ({ entry, symptom }));
  const editing = !!init.entry;
  const kind = init.symptom.kind;

  let value = $state<string>(init.entry?.value ?? (kind === 'med' ? (init.symptom.unit ?? '') : ''));
  let note = $state(init.entry?.note ?? '');
  let when = $state(toLocalInput(init.entry?.occurred_at ?? new Date().toISOString()));
  let details = $state(editing || kind === 'note');
  let busy = $state(false);
  let error = $state('');

  const hasOptions = kind === 'scale' || kind === 'choice';
  const needsSubmit = $derived(editing || details || !hasOptions);

  async function submit(v: string | null = value) {
    error = '';
    if (kind === 'number' && (v === null || v.trim() === '' || isNaN(Number(v.replace(',', '.'))))) {
      error = 'Enter a number';
      return;
    }
    if (hasOptions && !v) {
      error = 'Pick one';
      return;
    }
    if (kind === 'note' && !note.trim()) {
      error = 'Write something';
      return;
    }
    busy = true;
    try {
      const val = kind === 'note' ? null : v && v.trim() !== '' ? v.trim() : null;
      await onsubmit(val, note.trim() || null, fromLocalInput(when));
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  function pick(opt: string) {
    value = opt;
    if (!needsSubmit) submit(opt);
  }
</script>

<div class="form">
  {#if hasOptions}
    <div class="options" class:scale={kind === 'scale'}>
      {#each symptom.options as opt}
        <button
          class="opt"
          class:selected={value === opt}
          onclick={() => pick(opt)}
          disabled={busy}>{opt}</button
        >
      {/each}
    </div>
  {:else if kind === 'number'}
    <label for="num">Value{symptom.unit ? ` (${symptom.unit})` : ''}</label>
    <!-- svelte-ignore a11y_autofocus -->
    <input id="num" inputmode="decimal" bind:value autofocus placeholder={symptom.unit ?? ''} />
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
  .more { color: var(--accent); font-weight: 600; padding: 16px 0 4px; }
  .submit { margin-top: 18px; }
  .btn.danger { margin-top: 10px; }
  .error { color: var(--bad); margin: 10px 0 0; }
  .center { text-align: center; }
</style>
