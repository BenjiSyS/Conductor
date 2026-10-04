<script lang="ts">
  import { RefreshCw, RotateCcw } from '@lucide/svelte';
  import { app, attempt, saveSettings } from '../../lib/app.svelte';
  import { call } from '../../lib/api';
  import Toggle from '../Toggle.svelte';

  interface CavemanStatus {
    enabled: boolean;
    active: string;
    pending: { version: string } | null;
    previous: { version: string } | null;
    last_error: string | null;
    upstream: string;
    license: string;
  }
  let cm = $state<CavemanStatus | null>(null);
  let busy = $state(false);
  const load = async () => (cm = await call<CavemanStatus>('caveman_status').catch(() => null));
  $effect(() => {
    void load();
  });

  async function update() {
    busy = true;
    const r = await attempt(() => call<{ version: string; activated: boolean }>('caveman_update'));
    busy = false;
    if (r) {
      await load();
    }
  }
</script>

<h2>Optimization</h2>
<p class="lede">Use the minimum context needed for a good answer. These run locally; nothing here needs a model.</p>

<div class="setting">
  <div class="text">
    <strong>Context compression</strong>
    <p>
      Relevant files and symbols only, large files as excerpts, logs reduced to the failure, older conversation
      summarised. Constraints and decisions are never dropped.
    </p>
  </div>
  <Toggle
    checked={app.settings.compression}
    label="Context compression"
    onchange={(v) => attempt(() => saveSettings({ compression: v }))}
  />
</div>
<div class="setting">
  <div class="text">
    <strong>Smart Context Cache</strong>
    <p>
      Indexes are cached by content hash and updated incrementally; unchanged files aren't resent within a conversation.
      Always on.
    </p>
  </div>
  <span class="badge success">On</span>
</div>
<div class="setting">
  <div class="text">
    <strong>Secret scanning</strong>
    <p>
      Likely API keys, tokens, private keys and .env values are redacted before anything leaves this computer. Always
      on.
    </p>
  </div>
  <span class="badge success">On</span>
</div>

<h3 class="sub">Caveman</h3>
<div class="setting">
  <div class="text">
    <strong>Terse responses</strong>
    <p>
      Asks models to answer tersely while keeping code, commands and errors exact — fewer output tokens. Conductor's own
      context pipeline works with or without it.
    </p>
  </div>
  <Toggle
    checked={app.settings.caveman}
    label="Caveman"
    onchange={(v) => attempt(() => saveSettings({ caveman: v }))}
  />
</div>
{#if cm}
  <div class="setting">
    <div class="text">
      <strong>Version {cm.active}</strong>
      <p>
        {cm.active.startsWith('builtin')
          ? 'Conductor’s built-in instruction.'
          : 'Upstream skill, verified by checksum.'}
        Updates come from
        <a href={cm.upstream} target="_blank" rel="noreferrer">{cm.upstream.replace('https://', '')}</a>
        ({cm.license}), are validated, and activate only when no Goal is running.
        {#if cm.pending}Pending: {cm.pending.version}.{/if}
        {#if cm.last_error}<span class="warn">{cm.last_error}</span>{/if}
      </p>
    </div>
    <button class="btn sm" onclick={update} disabled={busy}
      ><RefreshCw size={12} /> {busy ? 'Checking…' : 'Update'}</button
    >
    {#if !cm.active.startsWith('builtin')}<button
        class="btn sm ghost"
        onclick={() =>
          attempt(async () => {
            await call('caveman_rollback');
            await load();
          }, 'Rolled back')}><RotateCcw size={12} /> Roll back</button
      >{/if}
  </div>
{/if}
<p class="xsmall faint">
  No fixed savings are promised; the Context Inspector shows estimated tokens for each request.
</p>

<style>
  .warn {
    color: var(--warning);
  }
</style>
