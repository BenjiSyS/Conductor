<script lang="ts">
  import { Check, Plug } from '@lucide/svelte';
  import { app, refresh, toast } from '../lib/app.svelte';
  import { call, readable } from '../lib/api';
  import type { BridgeInfo, Provider, SetupScan } from '../lib/types';

  let { ondone }: { ondone?: (p: Provider) => void } = $props();

  let bridges = $state<BridgeInfo[] | null>(null);
  let busy = $state<string | null>(null);
  let error = $state<Record<string, string>>({});

  let local = $state<SetupScan['local']>([]);
  async function loadLocal() {
    local = await call<SetupScan>('setup_scan')
      .then((r) => r.local)
      .catch(() => []);
  }
  $effect(() => {
    void loadLocal();
  });
  async function connectLocal(l: SetupScan['local'][number]) {
    busy = l.id;
    error[l.id] = '';
    try {
      const saved = await call<Provider>('save_provider', {
        config: { id: l.id, name: l.label, kind: 'openai_compatible', base_url: l.url, models: [], enabled: true },
        apiKey: null,
      });
      toast(
        `${saved.name} connected · ${saved.models.length} model${saved.models.length === 1 ? '' : 's'} (works offline)`,
        'success',
      );
      await refresh();
      ondone?.(saved);
    } catch (e) {
      error[l.id] = readable(e);
    } finally {
      busy = null;
    }
  }

  async function load() {
    bridges = await call<{ bridges: BridgeInfo[] }>('cli_bridges')
      .then((r) => r.bridges)
      .catch(() => []);
  }
  $effect(() => {
    void load();
  });

  async function connect(b: BridgeInfo) {
    busy = b.cli;
    error[b.cli] = '';
    try {
      const saved = await call<Provider>('cli_bridge_connect', { cli: b.cli });
      toast(`${saved.name} connected · ${saved.models.length} model${saved.models.length === 1 ? '' : 's'}`, 'success');
      await refresh();
      await load();
      ondone?.(saved);
    } catch (e) {
      error[b.cli] = readable(e);
    } finally {
      busy = null;
    }
  }
  const installed = $derived(bridges?.filter((b) => b.installed) ?? []);
</script>

{#if installed.length || local.length}
  <section class="bridges" aria-label="Apps you already have">
    <div class="head">
      <strong>Use an app you already have</strong>
      <span class="xsmall muted">No API key needed — uses apps you're signed in to, or models on this computer.</span>
    </div>
    {#each local as l (l.id)}
      <div class="bridge local">
        <span class="dot" aria-hidden="true"></span>
        <div class="grow">
          <div>{l.label} <span class="xsmall muted">· {l.models} models on this computer · works offline</span></div>
          {#if error[l.id]}<p class="xsmall warn">{error[l.id]}</p>{/if}
        </div>
        {#if app.providers.some((p) => p.id === l.id && p.enabled)}
          <span class="xsmall ok"><Check size={13} /> Connected</span>
        {:else}
          <button class="btn sm" onclick={() => connectLocal(l)} disabled={busy !== null} aria-label="Connect {l.label}"
            ><Plug size={13} /> {busy === l.id ? 'Connecting…' : 'Connect'}</button
          >
        {/if}
      </div>
    {/each}
    {#each installed as b (b.cli)}
      <div class="bridge brand-{b.brand}">
        <span class="dot" aria-hidden="true"></span>
        <div class="grow">
          <div>{b.label}</div>
          {#if error[b.cli]}
            <p class="xsmall warn">{error[b.cli]}</p>
          {:else if b.version}
            <p class="xsmall muted">{b.version}</p>
          {/if}
        </div>
        {#if b.connected}
          <span class="xsmall ok"><Check size={13} /> Connected</span>
        {:else}
          <button class="btn sm" onclick={() => connect(b)} disabled={busy !== null} aria-label="Connect {b.label}"
            ><Plug size={13} /> {busy === b.cli ? 'Connecting…' : 'Connect'}</button
          >
        {/if}
      </div>
    {/each}
  </section>
{/if}

<style>
  .bridges {
    display: grid;
    gap: var(--s2);
    margin-bottom: var(--s4);
    padding-bottom: var(--s4);
    border-bottom: 1px solid var(--border);
  }
  .head {
    display: grid;
    gap: 2px;
  }
  .bridge {
    display: flex;
    align-items: center;
    gap: var(--s3);
    padding: var(--s2) var(--s3);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface);
  }
  .bridge p {
    margin: 2px 0 0;
  }
  .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    flex: none;
    background: var(--accent);
  }
  .local .dot {
    background: var(--success);
  }
  .brand-claude .dot {
    background: #d97757;
  }
  .brand-chatgpt .dot {
    background: #10a37f;
  }
  .brand-gemini .dot {
    background: linear-gradient(135deg, #4285f4, #9b72cb, #d96570);
  }
  .warn {
    color: var(--warning);
    white-space: pre-line;
  }
  .ok {
    color: var(--success);
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
</style>
