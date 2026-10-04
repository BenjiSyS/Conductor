<script lang="ts">
  import { Gauge } from '@lucide/svelte';
  import EffortSlider from './EffortSlider.svelte';
  import { app } from '../lib/app.svelte';

  const model = $derived(
    app.target?.startsWith('model:')
      ? app.models.find((m) => `model:${m.provider}/${m.model}` === app.target)
      : undefined,
  );
  const isCombo = $derived(!!app.target?.startsWith('combo:'));
  // For a Combo: every level any member supports; members skip levels they lack.
  const combo = $derived(isCombo ? app.combos.find((c) => `combo:${c.id}` === app.target) : undefined);
  const levels = $derived(
    combo
      ? [
          ...new Set(
            combo.members
              .filter((m) => m.enabled)
              .flatMap((m) => app.models.find((x) => `${x.provider}/${x.model}` === m.model)?.efforts ?? []),
          ),
        ]
      : (model?.efforts ?? []),
  );

  $effect(() => {
    if (app.effort !== 'auto' && !levels.includes(app.effort)) app.effort = 'auto';
  });
</script>

{#if levels.length}
  <span
    class="effort"
    title={isCombo
      ? 'Effort for every Combo member that supports it. Auto lets each member decide.'
      : 'Reasoning effort. Auto picks the lowest level that fits the task and never escalates to the highest level without your permission.'}
  >
    <Gauge size={13} />
    <EffortSlider {levels} value={app.effort} compact onchange={(v) => (app.effort = v ?? 'auto')} />
  </span>
{:else if isCombo}
  <span class="effort muted" title="Each Combo member uses its own effort (automatic unless set in the Combo)."
    ><Gauge size={13} /> Auto</span
  >
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
</style>
