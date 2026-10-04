<script lang="ts">
  import { RefreshCw } from '@lucide/svelte';
  import { app } from '../lib/app.svelte';
  import { call, readable } from '../lib/api';
  import { tokens } from '../lib/format';
  import type { ContextPack } from '../lib/types';
  import Drawer from './Drawer.svelte';

  let pack = $state<ContextPack | null>(null);
  let error = $state('');
  let loading = $state(false);
  let open = $state<Record<number, boolean>>({});

  async function load() {
    if (!app.projectId) return;
    loading = true;
    error = '';
    try {
      pack = await call<ContextPack>('context_pack', {
        projectId: app.projectId,
        prompt: app.draft || app.conversation?.messages.findLast((m) => m.role === 'user')?.text || '',
        conversationId: app.conversationId,
      });
    } catch (e) {
      error = readable(e);
    } finally {
      loading = false;
    }
  }
  $effect(() => {
    void app.projectId;
    void load();
  });
</script>

<Drawer title="Context Inspector" onclose={() => (app.inspectorOpen = false)} width={460}>
  {#snippet actions()}
    <button class="btn ghost icon sm" aria-label="Refresh" onclick={load} disabled={loading}
      ><RefreshCw size={14} /></button
    >
  {/snippet}
  {#if error}<p class="small danger">{error}</p>{/if}
  {#if !app.projectId}
    <p class="muted small">Open a project to see what would be sent.</p>
  {:else if pack}
    <p class="xsmall muted intro">
      What Conductor would send for {app.draft ? 'your current draft' : 'the latest request'}. All numbers are
      estimates.
    </p>
    <div class="stats">
      <div>
        <span class="big">{tokens(pack.total_tokens)}</span><span class="xsmall muted"
          >of {tokens(pack.budget_tokens)} budget</span
        >
      </div>
      <div>
        <span class="big">{pack.naive_tokens ? Math.round((1 - pack.total_tokens / pack.naive_tokens) * 100) : 0}%</span
        ><span class="xsmall muted">avoided vs. naive</span>
      </div>
      <div>
        <span class="big">{tokens(pack.cache_reused_tokens)}</span><span class="xsmall muted">reused from cache</span>
      </div>
    </div>
    {#if pack.redactions || pack.injection_warnings}
      <p class="small warn">
        {#if pack.redactions}{pack.redactions} likely secret{pack.redactions === 1 ? '' : 's'} redacted.{/if}
        {#if pack.injection_warnings}
          {pack.injection_warnings} instruction-like phrase{pack.injection_warnings === 1 ? '' : 's'} in project files will
          be treated as data.{/if}
      </p>
    {/if}

    <h4 class="sec">Included</h4>
    <ul class="list items">
      {#each pack.items as it, i (i)}
        <li>
          <button class="item" aria-expanded={!!open[i]} onclick={() => (open = { ...open, [i]: !open[i] })}>
            <span class="kind">{it.kind}</span>
            <span class="grow src">{it.source}</span>
            <span class="xsmall faint num">{tokens(it.tokens)}</span>
          </button>
          <p class="reason xsmall muted">
            {it.reason}{it.cached ? ' · cached' : ''}{it.reduced && !it.cached ? ' · reduced' : ''}
          </p>
          {#if open[i]}<pre class="content">{it.content.slice(0, 8000)}</pre>{/if}
        </li>
      {/each}
    </ul>

    {#if pack.omitted.length}
      <details class="omit">
        <summary class="small">Omitted ({pack.omitted.length})</summary>
        <ul class="list">
          {#each pack.omitted.slice(0, 60) as o, i (i)}
            <li class="row xsmall"><span class="grow mono">{o.source}</span><span class="faint">{o.reason}</span></li>
          {/each}
        </ul>
      </details>
    {/if}

    <h4 class="sec">Pipeline</h4>
    <ol class="list stages">
      {#each pack.stages as s, i (i)}
        <li><span class="small">{s.stage}</span><span class="xsmall muted">{s.detail}</span></li>
      {/each}
    </ol>
  {:else if loading}
    <div class="row muted small"><span class="spinner"></span> Indexing project…</div>
  {/if}
</Drawer>

<style>
  .intro {
    margin-bottom: var(--s3);
  }
  .stats {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: var(--s2);
    margin-bottom: var(--s3);
  }
  .stats div {
    display: flex;
    flex-direction: column;
    padding: var(--s2) var(--s3);
    background: var(--bg-sunken);
    border-radius: var(--radius-sm);
  }
  .big {
    font-size: var(--fs-lg);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .warn {
    color: var(--warning);
    margin-bottom: var(--s3);
  }
  .danger {
    color: var(--danger);
  }
  .sec {
    margin: var(--s4) 0 var(--s2);
  }
  .items li {
    padding: var(--s1) 0;
    border-bottom: 1px solid var(--border);
  }
  .item {
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--s2);
    border: none;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-sm);
    padding: 2px 0;
    cursor: pointer;
    text-align: left;
  }
  .kind {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--accent);
    width: 62px;
    flex: none;
  }
  .src {
    font-family: var(--font-mono);
    font-size: var(--fs-xs);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .num {
    flex: none;
    font-variant-numeric: tabular-nums;
  }
  .reason {
    padding-left: 70px;
  }
  .content {
    font-size: 11px;
    background: var(--code-bg);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: var(--s2);
    max-height: 260px;
    overflow: auto;
    white-space: pre-wrap;
  }
  .omit {
    margin-top: var(--s3);
  }
  .omit summary {
    cursor: pointer;
  }
  .omit li {
    padding: 2px 0;
  }
  .stages li {
    display: flex;
    flex-direction: column;
    padding: 4px 0;
  }
</style>
