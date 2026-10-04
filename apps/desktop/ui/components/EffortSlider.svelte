<script lang="ts">
  // Effort as a slider: Auto, then the model's own levels from lowest to
  // highest. Only levels the model declares are offered.
  let {
    levels,
    value,
    onchange,
    label = 'Effort',
    compact = false,
  }: {
    levels: string[];
    value: string | null;
    onchange: (v: string | null) => void;
    label?: string;
    compact?: boolean;
  } = $props();

  const ORDER = ['none', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max', 'ultra'];
  const NAMES: Record<string, string> = {
    none: 'Off',
    minimal: 'Minimal',
    low: 'Low',
    medium: 'Medium',
    high: 'High',
    xhigh: 'Extra high',
    max: 'Max',
    ultra: 'Ultra',
  };
  const rank = (l: string) => (ORDER.includes(l) ? ORDER.indexOf(l) : 99);
  const steps = $derived([null, ...[...new Set(levels)].sort((a, b) => rank(a) - rank(b))]);
  const index = $derived(Math.max(0, steps.indexOf(value === 'auto' ? null : value)));
  const name = (v: string | null) => (v === null ? 'Auto' : (NAMES[v] ?? v));
</script>

<div class="slider" class:compact>
  <input
    type="range"
    min="0"
    max={steps.length - 1}
    step="1"
    value={index}
    aria-label={label}
    aria-valuetext={name(steps[index])}
    oninput={(e) => onchange(steps[Number((e.target as HTMLInputElement).value)])}
    disabled={steps.length < 2}
  />
  <span class="value">{name(steps[index])}</span>
</div>

<style>
  .slider {
    display: inline-flex;
    align-items: center;
    gap: var(--s2);
    min-width: 0;
  }
  input[type='range'] {
    width: 120px;
    accent-color: var(--accent);
    cursor: pointer;
  }
  .compact input[type='range'] {
    width: 72px;
  }
  .value {
    font-size: var(--fs-sm);
    color: var(--text);
    min-width: 5.5em;
    white-space: nowrap;
  }
  .compact .value {
    min-width: 4.5em;
  }
</style>
