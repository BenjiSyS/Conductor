<script lang="ts">
  import { Gauge, ChevronDown } from '@lucide/svelte';
  import { app } from '../lib/app.svelte';

  const model = $derived(
    app.target?.startsWith('model:')
      ? app.models.find((m) => `model:${m.provider}/${m.model}` === app.target)
      : undefined,
  );
  const levels = $derived(model?.efforts ?? []);
  const isCombo = $derived(!!app.target?.startsWith('combo:'));
  const labels: Record<string, string> = {
    none: 'Off',
    minimal: 'Minimal',
    low: 'Low',
    medium: 'Medium',
    high: 'High',
    xhigh: 'Extra high',
    max: 'Max',
    ultra: 'Ultra',
  };

  $effect(() => {
    if (app.effort !== 'auto' && !levels.includes(app.effort)) app.effort = 'auto';
  });
</script>

{#if isCombo}
  <span class="effort muted" title="Each Combo member uses its own effort (automatic unless set in the Combo)."
    ><Gauge size={13} /> Auto</span
  >
{:else if levels.length}
  <label
    class="effort"
    title="Reasoning effort. Auto picks the lowest level that fits the task and never escalates to the highest level without your permission."
  >
    <Gauge size={13} />
    <span class="sr-only">Effort</span>
    <select bind:value={app.effort} aria-label="Effort">
      <option value="auto">Auto</option>
      {#each levels as l (l)}<option value={l}>{labels[l] ?? l}</option>{/each}
    </select>
    <ChevronDown size={12} class="chev" />
  </label>
{:else if model}
  <span class="effort faint" title="This model doesn't declare effort controls."><Gauge size={13} /> —</span>
{/if}

<style>
  .effort {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 26px;
    padding: 0 var(--s2);
    border-radius: var(--radius-sm);
    font-size: var(--fs-sm);
    color: var(--text-muted);
  }
  label.effort:hover {
    background: var(--surface-hover);
  }
  select {
    appearance: none;
    -webkit-appearance: none;
    border: none;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-sm);
    cursor: pointer;
    outline: none;
    padding: 0;
  }
  label.effort :global(.chev) {
    margin-left: -2px;
    pointer-events: none;
  }
  select option {
    background: var(--bg-elevated);
  }
</style>
