<script lang="ts">
  import { Check, Download, Loader2, X } from '@lucide/svelte';
  import { attempt } from '../lib/app.svelte';
  import { call } from '../lib/api';
  import type { SetupScan } from '../lib/types';

  let scan = $state<SetupScan | null>(null);
  let installing = $state<string | null>(null);

  async function load(fresh = false) {
    scan = await call<SetupScan>('setup_scan', { fresh }).catch(() => null);
  }
  $effect(() => {
    void load();
  });

  async function install(id: string, name: string) {
    installing = id;
    const msg = await attempt(() => call<string>('env_install', { tool: id }), `${name} installed`);
    installing = null;
    if (msg !== undefined) await load(true);
  }
</script>

<section class="ready" aria-label="Getting your computer ready" aria-busy={!scan}>
  {#if !scan}
    <p class="row small muted"><Loader2 size={14} class="spin" /> Checking this computer in the background…</p>
  {:else}
    <ul class="list">
      {#each scan.tools as t (t.id)}
        <li class="row small">
          {#if t.state === 'ok'}<Check size={14} class="ok" />{:else}<X size={14} class="bad" />{/if}
          <span class="grow">{t.name}{t.version ? ` ${t.version}` : ''}{t.state === 'ok' ? '' : ' — not found'}</span>
          {#if t.state !== 'ok' && t.install}
            <button class="btn sm" onclick={() => install(t.id, t.name)} disabled={installing !== null}
              ><Download size={12} /> {installing === t.id ? 'Installing…' : 'Install'}</button
            >
          {/if}
        </li>
      {/each}
      {#if scan.bridges.length}
        <li class="row small">
          <Check size={14} class="ok" />
          <span class="grow">Signed-in AI apps: {scan.bridges.map((b) => b.label).join(', ')}</span>
        </li>
      {/if}
      {#if scan.local.length}
        <li class="row small">
          <Check size={14} class="ok" />
          <span class="grow"
            >Local models (work offline): {scan.local.map((l) => `${l.label} · ${l.models} models`).join(', ')}</span
          >
        </li>
      {/if}
    </ul>
  {/if}
</section>

<style>
  .ready {
    margin-top: var(--s5);
    padding: var(--s3) var(--s4);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    text-align: left;
    max-width: 460px;
    margin-inline: auto;
  }
  .list {
    display: grid;
    gap: 6px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .ready :global(.ok) {
    color: var(--success);
  }
  .ready :global(.bad) {
    color: var(--warning);
  }
  .ready :global(.spin) {
    animation: spin 1s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
