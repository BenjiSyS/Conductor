<script lang="ts">
  import { Download, Upload, FolderOpen } from '@lucide/svelte';
  import { app, attempt, refresh, savePrefs, toast } from '../../lib/app.svelte';
  import { call } from '../../lib/api';
  import Toggle from '../Toggle.svelte';
  import Modal from '../Modal.svelte';

  let exported = $state<string | null>(null);
  let importing = $state(false);
  let importText = $state('');
  let receipts = $state<
    { id: string; kind: string; name: string; source: string; version: string; installed_at: number }[]
  >([]);
  $effect(() => {
    call<typeof receipts>('receipts_list')
      .then((r) => (receipts = r))
      .catch(() => {});
  });
</script>

<h2>Privacy & data</h2>
<p class="lede">
  Everything stays on this computer. Conductor has no server and no account. Your project code goes only to the
  providers you choose.
</p>

<div class="setting">
  <div class="text">
    <strong>Privacy profile</strong>
    <p>
      Restricted keeps remote access off. Local First prefers local models when Conductor picks a default. Secret
      scanning is always on.
    </p>
  </div>
  <select
    class="select"
    value={app.prefs?.privacy}
    onchange={(e) =>
      attempt(() =>
        savePrefs({ privacy: (e.target as HTMLSelectElement).value as 'local_first' | 'standard' | 'restricted' }),
      )}
    aria-label="Privacy profile"
  >
    <option value="local_first">Local First</option><option value="standard">Standard</option><option value="restricted"
      >Restricted</option
    >
  </select>
</div>
<div class="setting">
  <div class="text">
    <strong>Usage statistics</strong>
    <p>
      Off, and there is currently nothing to send: Conductor contains no telemetry. If added in future it would be
      opt-in, list every field, and never include prompts or code.
    </p>
  </div>
  <Toggle checked={false} label="Usage statistics" disabled={true} onchange={() => {}} />
</div>

<h3 class="sub">Your data</h3>
<div class="setting">
  <div class="text">
    <strong>Data folder</strong>
    <p class="mono">{app.dataDir}</p>
  </div>
  <button class="btn sm" onclick={() => call('reveal_path', { path: app.dataDir })}
    ><FolderOpen size={13} /> Show</button
  >
</div>
<div class="setting">
  <div class="text">
    <strong>Export configuration</strong>
    <p>Combos, MCP servers, instructions and preferences. Never includes API keys or secrets.</p>
  </div>
  <button class="btn sm" onclick={async () => (exported = await call<string>('config_export'))}
    ><Download size={13} /> Export</button
  >
  <button class="btn sm" onclick={() => (importing = true)}><Upload size={13} /> Import</button>
</div>
<div class="setting">
  <div class="text">
    <strong>Diagnostics</strong>
    <p>A file with versions, tool checks and recent logs — secrets redacted, no prompts or code.</p>
  </div>
  <button
    class="btn sm"
    onclick={async () => {
      const p = await attempt(() => call<string>('diagnostics_export'));
      if (p) {
        toast('Diagnostics saved');
        void call('reveal_path', { path: p });
      }
    }}>Export diagnostics</button
  >
</div>

{#if receipts.length}
  <h3 class="sub">Installed by Conductor</h3>
  <table class="table">
    <thead><tr><th>Item</th><th>Kind</th><th>Version</th><th>Source</th></tr></thead>
    <tbody>
      {#each receipts as r (r.id)}<tr
          ><td>{r.name}</td><td>{r.kind}</td><td>{r.version}</td><td class="xsmall mono src">{r.source}</td></tr
        >{/each}
    </tbody>
  </table>
{/if}

{#if exported}
  <Modal title="Configuration export" onclose={() => (exported = null)}>
    <textarea class="textarea mono" rows="14" readonly value={exported} aria-label="Configuration"></textarea>
    {#snippet footer()}<button
        class="btn primary"
        onclick={() => {
          void navigator.clipboard?.writeText(exported ?? '');
          toast('Copied');
        }}>Copy</button
      >{/snippet}
  </Modal>
{/if}
{#if importing}
  <Modal title="Import configuration" onclose={() => (importing = false)}>
    <textarea
      class="textarea mono"
      rows="12"
      bind:value={importText}
      placeholder="Paste a Conductor configuration export"
      aria-label="Configuration JSON"
    ></textarea>
    {#snippet footer()}
      <button class="btn ghost" onclick={() => (importing = false)}>Cancel</button>
      <button
        class="btn primary"
        disabled={!importText.trim()}
        onclick={async () => {
          const r = await attempt(() => call<string[]>('config_import', { json: importText }));
          if (r) {
            toast(r.join(' · '), 'success');
            importing = false;
            await refresh();
          }
        }}>Import</button
      >
    {/snippet}
  </Modal>
{/if}

<style>
  .src {
    max-width: 260px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
