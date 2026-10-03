<script lang="ts">
  import { RefreshCw, Copy } from '@lucide/svelte';
  import { toast } from '../../lib/app.svelte';
  import { call } from '../../lib/api';
  import type { ToolReport } from '../../lib/types';

  let tools = $state<ToolReport[] | null>(null);
  let ports = $state<{ port: number; address: string; pid: number | null; dev: boolean }[]>([]);
  let busy = $state(false);

  async function run() {
    busy = true;
    tools = await call<ToolReport[]>('env_doctor').catch(() => []);
    ports = await call<typeof ports>('ports').catch(() => []);
    busy = false;
  }
  $effect(() => {
    void run();
  });
  const icon = (s: string) => (s === 'ok' ? '✓' : s === 'missing' ? '·' : '!');
</script>

<h2>Environment</h2>
<p class="lede">
  What's installed on this computer, so agents use tools that exist. Checks only read — nothing is changed.
</p>

<div class="row">
  <span class="spacer"></span><button class="btn sm" onclick={run} disabled={busy}
    ><RefreshCw size={13} /> {busy ? 'Checking…' : 'Check again'}</button
  >
</div>
{#if tools}
  <table class="table">
    <thead><tr><th></th><th>Tool</th><th>Version</th><th>Notes</th></tr></thead>
    <tbody>
      {#each tools as t (t.id)}
        <tr class={t.state}>
          <td class="st">{icon(t.state)}</td>
          <td>{t.name}</td>
          <td class="mono xsmall">{t.version ?? ''}</td>
          <td class="xsmall muted">
            {t.detail}
            {#if t.install}
              <button
                class="btn ghost sm"
                title="Copy install command"
                onclick={() => {
                  void navigator.clipboard?.writeText(t.install!.join(' '));
                  toast('Install command copied');
                }}><Copy size={12} /> {t.install.join(' ')}</button
              >
            {/if}
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
{:else}
  <div class="row muted small"><span class="spinner"></span> Checking tools…</div>
{/if}

{#if ports.some((p) => p.dev)}
  <h3 class="sub">Local dev servers</h3>
  {#each ports.filter((p) => p.dev) as p (p.port)}
    <div class="setting">
      <div class="text">
        <strong class="mono">http://localhost:{p.port}</strong>
        <p>Process {p.pid ?? '?'}</p>
      </div>
      <a class="btn sm" href="http://localhost:{p.port}" target="_blank" rel="noreferrer">Open</a>
    </div>
  {/each}
{/if}

<style>
  .st {
    width: 20px;
    text-align: center;
    font-weight: 600;
  }
  tr.ok .st {
    color: var(--success);
  }
  tr.missing .st {
    color: var(--text-faint);
  }
  tr.broken .st,
  tr.too_old .st {
    color: var(--warning);
  }
</style>
