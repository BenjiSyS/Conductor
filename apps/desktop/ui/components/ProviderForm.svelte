<script lang="ts">
  import { KeyRound } from '@lucide/svelte';
  import { refresh, toast } from '../lib/app.svelte';
  import { call, readable } from '../lib/api';
  import type { Provider, ProviderKind } from '../lib/types';
  import BridgeList from './BridgeList.svelte';

  let { ondone, compact = false }: { ondone?: (p: Provider) => void; compact?: boolean } = $props();

  const presets: { id: string; label: string; kind: ProviderKind; url: string; key: boolean; help: string }[] = [
    {
      id: 'openai',
      label: 'OpenAI',
      kind: 'openai',
      url: 'https://api.openai.com/v1',
      key: true,
      help: 'Create a key at platform.openai.com → API keys.',
    },
    {
      id: 'anthropic',
      label: 'Anthropic (Claude)',
      kind: 'anthropic',
      url: 'https://api.anthropic.com/v1',
      key: true,
      help: 'Create a key at console.anthropic.com → API keys.',
    },
    {
      id: 'gemini',
      label: 'Google Gemini',
      kind: 'gemini',
      url: 'https://generativelanguage.googleapis.com/v1beta',
      key: true,
      help: 'Create a key at aistudio.google.com → Get API key.',
    },
    {
      id: 'xai',
      label: 'xAI (Grok)',
      kind: 'openai_compatible',
      url: 'https://api.x.ai/v1',
      key: true,
      help: 'Create a key at console.x.ai → API Keys.',
    },
    {
      id: 'ollama',
      label: 'Ollama (local)',
      kind: 'openai_compatible',
      url: 'http://localhost:11434/v1',
      key: false,
      help: 'Start Ollama, then pull a model (e.g. ollama pull llama3.2).',
    },
    {
      id: 'lmstudio',
      label: 'LM Studio (local)',
      kind: 'openai_compatible',
      url: 'http://localhost:1234/v1',
      key: false,
      help: 'Start the LM Studio server and load a model.',
    },
    {
      id: 'custom',
      label: 'Custom (OpenAI-compatible)',
      kind: 'openai_compatible',
      url: 'https://',
      key: false,
      help: 'Any OpenAI-compatible endpoint. Custom providers are untrusted: only send what you would send to that service.',
    },
  ];

  let preset = $state('openai');
  let name = $state('OpenAI');
  let url = $state('https://api.openai.com/v1');
  let key = $state('');
  let busy = $state(false);
  let error = $state('');

  const p = $derived(presets.find((x) => x.id === preset)!);

  function choose(id: string) {
    preset = id;
    const x = presets.find((y) => y.id === id)!;
    name = x.label.replace(' (local)', '').replace(' (OpenAI-compatible)', '');
    url = x.url;
    error = '';
  }

  async function connect() {
    busy = true;
    error = '';
    const id =
      preset === 'custom'
        ? `custom-${
            name
              .toLowerCase()
              .replace(/[^a-z0-9]+/g, '-')
              .replace(/^-|-$/g, '') || 'provider'
          }`
        : preset;
    try {
      const saved = await call<Provider>('save_provider', {
        config: { id, name: name.trim() || p.label, kind: p.kind, base_url: url.trim(), models: [], enabled: true },
        apiKey: key.trim() || null,
      });
      key = '';
      toast(`${saved.name} connected · ${saved.models.length} model${saved.models.length === 1 ? '' : 's'}`, 'success');
      await refresh();
      ondone?.(saved);
    } catch (e) {
      error = readable(e);
    } finally {
      busy = false;
    }
  }
</script>

<BridgeList {ondone} />

<div class="pf" class:compact>
  <div class="kinds" role="radiogroup" aria-label="Provider">
    {#each presets as x (x.id)}
      <button role="radio" aria-checked={preset === x.id} class="kind" onclick={() => choose(x.id)}>{x.label}</button>
    {/each}
  </div>
  <div class="grid">
    {#if preset === 'custom'}
      <div class="field">
        <label class="label" for="pf-name">Name</label>
        <input id="pf-name" class="input" bind:value={name} />
      </div>
    {/if}
    {#if p.kind === 'openai_compatible' && p.id !== 'xai'}
      <div class="field">
        <label class="label" for="pf-url">Base URL</label>
        <input id="pf-url" class="input mono" bind:value={url} spellcheck="false" />
        <p class="hint">HTTPS, or HTTP on localhost only.</p>
      </div>
    {/if}
    <div class="field">
      <label class="label" for="pf-key">API key {p.key ? '' : '(optional)'}</label>
      <div class="keyrow">
        <KeyRound size={14} />
        <input
          id="pf-key"
          class="input"
          type="password"
          bind:value={key}
          autocomplete="off"
          spellcheck="false"
          placeholder={p.key ? 'Paste your API key' : 'Not needed for most local servers'}
          onkeydown={(e) => e.key === 'Enter' && connect()}
        />
      </div>
      <p class="hint">{p.help} Stored in your OS credential store — never in plain text, never exported.</p>
    </div>
  </div>
  {#if error}<p class="error small" role="alert">{error}</p>{/if}
  <div class="row">
    <span class="spacer"></span>
    <button class="btn primary" onclick={connect} disabled={busy || (p.key && !key.trim())}
      >{busy ? 'Testing connection…' : 'Test & connect'}</button
    >
  </div>
</div>

<style>
  .kinds {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s2);
    margin-bottom: var(--s4);
  }
  .kind {
    height: 30px;
    padding: 0 var(--s3);
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .kind[aria-checked='true'] {
    border-color: var(--accent);
    background: var(--accent-soft);
    color: var(--accent);
  }
  .keyrow {
    position: relative;
    display: flex;
    align-items: center;
  }
  .keyrow :global(svg) {
    position: absolute;
    left: 10px;
    color: var(--text-faint);
  }
  .keyrow .input {
    padding-left: 32px;
  }
  .error {
    color: var(--danger);
    margin-bottom: var(--s3);
  }
</style>
