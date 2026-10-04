<script lang="ts">
  import { Layers } from '@lucide/svelte';
  import { app, togglePanel } from '../lib/app.svelte';
  import { call } from '../lib/api';
  import { tokens } from '../lib/format';
  import type { ContextPack } from '../lib/types';

  let pack = $state<ContextPack | null>(null);

  $effect(() => {
    const draft = app.draft;
    const pid = app.projectId;
    const cid = app.conversationId;
    if (!pid) return;
    const timer = setTimeout(
      () => {
        call<ContextPack>('context_pack', { projectId: pid, prompt: draft, conversationId: cid })
          .then((p) => (pack = p))
          .catch(() => (pack = null));
      },
      draft ? 700 : 50,
    );
    return () => clearTimeout(timer);
  });

  const level = $derived(
    !pack
      ? 'idle'
      : pack.total_tokens > pack.budget_tokens * 0.9
        ? 'high'
        : pack.total_tokens > pack.budget_tokens * 0.6
          ? 'mid'
          : 'low',
  );
  const pct = $derived(pack ? Math.min(100, (pack.total_tokens / Math.max(1, pack.budget_tokens)) * 100) : 0);
</script>

{#if pack}
  <button
    class="pill {level}"
    onclick={() => togglePanel('inspector')}
    title="Estimated context: {pack.total_tokens.toLocaleString()} of {pack.budget_tokens.toLocaleString()} tokens. {pack.compression_active
      ? 'Compression on'
      : 'No compression needed'}{pack.cache_reused_tokens
      ? ` · ${tokens(pack.cache_reused_tokens)} reused from cache`
      : ''}. Click to inspect."
    aria-label="Context budget, estimated {pack.total_tokens} tokens. Open context inspector."
  >
    <Layers size={12} />
    <span class="bar"><span style:width="{pct}%"></span></span>
    {tokens(pack.total_tokens)}
  </button>
{/if}

<style>
  .pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    padding: 0 var(--s2);
    border: none;
    border-radius: 999px;
    background: transparent;
    color: var(--text-faint);
    font: inherit;
    font-size: 11px;
    cursor: pointer;
    font-variant-numeric: tabular-nums;
  }
  .pill:hover {
    background: var(--surface-hover);
    color: var(--text-muted);
  }
  .bar {
    width: 28px;
    height: 4px;
    border-radius: 2px;
    background: var(--border);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--success);
    transition: width var(--duration) var(--ease);
  }
  .mid .bar span {
    background: var(--warning);
  }
  .high .bar span {
    background: var(--danger);
  }
</style>
