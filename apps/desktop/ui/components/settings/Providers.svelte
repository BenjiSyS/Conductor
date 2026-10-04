<script lang="ts">
  import { Plus, Unplug, Trash2, RefreshCw } from '@lucide/svelte';
  import { app, attempt, refresh, savePrefs, toast } from '../../lib/app.svelte';
  import Toggle from '../Toggle.svelte';
  import { call } from '../../lib/api';
  import type { Provider } from '../../lib/types';
  import ProviderForm from '../ProviderForm.svelte';

  let adding = $state(!app.providers.length);
  let reconnect = $state<Provider | null>(null);
  let key = $state('');

  const kindLabel: Record<string, string> = {
    openai: 'OpenAI',
    anthropic: 'Anthropic',
    gemini: 'Google Gemini',
    openai_compatible: 'OpenAI-compatible',
  };

  let checking = $state(false);
  // Model lists also refresh on their own at start-up and every 6 hours.
  const hasApps = $derived(app.providers.some((p) => p.id.startsWith('cli-')));
  async function checkModels() {
    checking = true;
    // Update connected signed-in apps first: their newest models come with
    // new app versions.
    if (hasApps) {
      const r = await attempt(() =>
        call<{ app: string; ok: boolean; message: string }[]>('cli_bridge_update', { cli: null }),
      );
      for (const x of r ?? []) if (!x.ok) toast(x.message, 'error');
    }
    const n = await attempt(() => call<number>('models_refresh'));
    checking = false;
    if (n === undefined) return;
    await refresh();
    toast(
      n ? `Updated the model list for ${n} provider${n === 1 ? '' : 's'}` : 'Model lists are up to date',
      'success',
    );
  }

  async function disconnect(p: Provider) {
    await attempt(
      () => call('disconnect_provider', { id: p.id }),
      `${p.name} disconnected; its key was removed from the keychain.`,
    );
    await refresh();
  }
  async function remove(p: Provider) {
    if (!confirm(`Remove ${p.name}? Its key is deleted from the OS keychain.`)) return;
    await attempt(() => call('remove_provider', { id: p.id }));
    await refresh();
  }
  // Signed-in apps reconnect without a key.
  async function reconnectApp(p: Provider) {
    if (await attempt(() => call('cli_bridge_connect', { cli: p.id.slice(4) }), `${p.name} connected`)) await refresh();
  }
  async function refreshModels(p: Provider, apiKey: string | null) {
    const r = await attempt(() => call<Provider>('save_provider', { config: p, apiKey }), `${p.name} connected`);
    if (r) {
      reconnect = null;
      key = '';
      await refresh();
    }
  }
</script>

<div class="row head">
  <h2 class="grow">Providers</h2>
  {#if app.providers.length}
    <button class="btn sm" onclick={checkModels} disabled={checking}
      ><RefreshCw size={13} /> {checking ? 'Checking…' : 'Check for new models'}</button
    >
  {/if}
</div>
<p class="lede">
  Connect the AI providers you already use. Keys live in your OS credential store; Conductor has no account of its own.
</p>

{#if hasApps}
  <div class="setting apps-update">
    <div class="text">
      <strong>Keep signed-in apps up to date</strong>
      <p>
        Updates Codex CLI, Antigravity CLI, Claude Code and Grok Build once a day so their newest models can be used
        right away.
      </p>
    </div>
    <Toggle
      checked={app.prefs?.auto_update_apps !== false}
      label="Keep signed-in apps up to date"
      onchange={(v) => attempt(() => savePrefs({ auto_update_apps: v }))}
    />
  </div>
{/if}

{#each app.providers as p (p.id)}
  <div class="prov card">
    <div class="row">
      <span class="dot" class:ok={p.enabled} class:bad={!p.enabled}></span>
      <div class="grow">
        <strong>{p.name}</strong>
        <span class="xsmall muted"
          >· {p.id.startsWith('cli-') ? 'Signed-in app' : kindLabel[p.kind]} · {p.models.length} models</span
        >
        <p class="xsmall {p.enabled ? 'muted' : 'warn'}">
          {p.enabled ? 'Connected' : `${p.name} signed out — reconnect to continue.`}
        </p>
      </div>
      {#if p.enabled}
        <button class="btn sm ghost" onclick={() => refreshModels(p, null)} title="Re-test and refresh the model list"
          ><RefreshCw size={13} /> Refresh</button
        >
        <button class="btn sm ghost" onclick={() => disconnect(p)}><Unplug size={13} /> Disconnect</button>
      {:else}
        <button class="btn sm primary" onclick={() => (p.id.startsWith('cli-') ? reconnectApp(p) : (reconnect = p))}
          >Reconnect</button
        >
      {/if}
      <button class="btn sm ghost icon" aria-label="Remove {p.name}" onclick={() => remove(p)}
        ><Trash2 size={13} /></button
      >
    </div>
    {#if reconnect?.id === p.id}
      <div class="row recon">
        <input
          class="input"
          type="password"
          placeholder={p.kind === 'openai_compatible' ? 'API key (optional)' : 'API key'}
          bind:value={key}
          aria-label="API key for {p.name}"
        />
        <button class="btn primary" onclick={() => refreshModels(p, key || null)}>Connect</button>
      </div>
    {/if}
    {#if p.enabled && p.models.length}
      <details>
        <summary class="xsmall muted">Models</summary>
        <ul class="list models">
          {#each p.models as m (m.id)}
            <li class="row xsmall">
              <span class="mono grow">{m.id}</span>{#if m.efforts.length}<span class="faint"
                  >effort: {m.efforts.join(', ')}</span
                >{/if}{#if m.context_window}<span class="faint">{Math.round(m.context_window / 1000)}k ctx</span>{/if}
            </li>
          {/each}
        </ul>
      </details>
    {/if}
  </div>
{/each}

{#if adding}
  <div class="card add">
    <h3 class="sub first">Add a provider</h3>
    <ProviderForm ondone={() => (adding = false)} />
  </div>
{:else}
  <button class="btn" onclick={() => (adding = true)}><Plus size={14} /> Add provider</button>
{/if}

<p class="xsmall faint note">
  The OpenAI, Anthropic and Gemini developer APIs authenticate with API keys. Conductor never extracts credentials from
  other apps. See how much you have left in <button class="linkish" onclick={() => (app.settingsTab = 'usage')}
    >Usage</button
  >, which also has an unsupported, opt-in sign-in for Claude, ChatGPT and Gemini subscriptions.
</p>

<style>
  .head h2 {
    margin: 0;
  }
  .prov {
    margin-bottom: var(--s2);
    padding: var(--s3) var(--s4);
  }
  .warn {
    color: var(--warning);
  }
  .recon {
    margin-top: var(--s3);
  }
  details {
    margin-top: var(--s2);
  }
  summary {
    cursor: pointer;
  }
  .models {
    max-height: 180px;
    overflow: auto;
    margin-top: var(--s1);
  }
  .models li {
    padding: 2px 0;
  }
  .add {
    margin-top: var(--s3);
  }
  .first {
    margin-top: 0 !important;
    margin-bottom: var(--s3) !important;
  }
  .note {
    margin-top: var(--s5);
  }
  .apps-update {
    margin-bottom: var(--s3);
  }
  .linkish {
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    color: var(--accent);
    text-decoration: underline;
    cursor: pointer;
  }
</style>
