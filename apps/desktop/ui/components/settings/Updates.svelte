<script lang="ts">
  import { app, attempt, savePrefs, toast } from '../../lib/app.svelte';
  import { readable } from '../../lib/api';
  import Toggle from '../Toggle.svelte';

  let status = $state('');
  let progress = $state<number | null>(null);
  let available = $state<{ version: string; notes: string } | null>(null);
  let updateObj: {
    downloadAndInstall: (
      cb: (e: { event: string; data?: { contentLength?: number; chunkLength?: number } }) => void,
    ) => Promise<void>;
  } | null = null;

  async function check() {
    if (!app.native) {
      status = 'Updates are checked in the desktop app.';
      return;
    }
    status = 'Checking…';
    try {
      const { check } = await import('@tauri-apps/plugin-updater');
      const u = await check();
      if (u) {
        available = { version: u.version, notes: u.body ?? '' };
        updateObj = u as unknown as typeof updateObj;
        status = '';
      } else {
        status = 'Conductor is up to date.';
      }
    } catch (e) {
      status = `Couldn't check for updates: ${readable(e)}`;
    }
  }
  async function install() {
    if (!updateObj) return;
    if (
      app.anyRunning &&
      !confirm(
        'Work is running. The update installs now and Conductor restarts; running Goals resume afterwards from where they stopped. Continue?',
      )
    )
      return;
    let total = 0;
    let got = 0;
    progress = 0;
    try {
      await updateObj.downloadAndInstall((e) => {
        if (e.event === 'Started') total = e.data?.contentLength ?? 0;
        if (e.event === 'Progress') {
          got += e.data?.chunkLength ?? 0;
          progress = total ? Math.round((got / total) * 100) : null;
        }
      });
      toast('Update verified and installed. Restarting…', 'success');
      const { relaunch } = await import('@tauri-apps/plugin-process');
      await relaunch();
    } catch (e) {
      progress = null;
      toast(`Update failed — your current version is unchanged. ${readable(e)}`, 'error');
    }
  }
</script>

<h2>Updates</h2>
<p class="lede">
  Conductor updates are signed; the signature is verified before anything is installed, and a failed update leaves your
  current version in place.
</p>

<div class="setting">
  <div class="text">
    <strong>Conductor app</strong>
    <p>{status || (available ? `Version ${available.version} is available.` : 'Check for a new version.')}</p>
  </div>
  {#if available}
    <button class="btn primary" onclick={install} disabled={progress !== null}
      >{progress !== null ? `Installing ${progress ?? ''}%` : `Install ${available.version}`}</button
    >
  {:else}
    <button class="btn" onclick={check}>Check now</button>
  {/if}
</div>
{#if available?.notes}<pre class="notes small">{available.notes}</pre>{/if}
<div class="setting">
  <div class="text">
    <strong>Check automatically</strong>
    <p>Looks for updates on start; never installs without you.</p>
  </div>
  <Toggle
    checked={!!app.prefs?.auto_update_check}
    label="Check automatically"
    onchange={(v) => attempt(() => savePrefs({ auto_update_check: v }))}
  />
</div>
<div class="setting">
  <div class="text">
    <strong>Channel</strong>
    <p>Beta and Nightly get changes sooner and may be less stable.</p>
  </div>
  <select
    class="select"
    value={app.prefs?.update_channel}
    onchange={(e) =>
      attempt(() =>
        savePrefs({ update_channel: (e.target as HTMLSelectElement).value as 'stable' | 'beta' | 'nightly' }),
      )}
    aria-label="Update channel"
  >
    <option value="stable">Stable</option><option value="beta">Beta</option><option value="nightly">Nightly</option>
  </select>
</div>

<h3 class="sub">Separate from app updates</h3>
<div class="setting">
  <div class="text">
    <strong>Provider models</strong>
    <p>Refresh a provider in Settings → Providers to pick up new models — no app release needed.</p>
  </div>
  <button class="btn sm" onclick={() => (app.settingsTab = 'providers')}>Providers</button>
</div>
<div class="setting">
  <div class="text">
    <strong>Caveman</strong>
    <p>Updated and rolled back independently in Optimization.</p>
  </div>
  <button class="btn sm" onclick={() => (app.settingsTab = 'optimization')}>Optimization</button>
</div>
<div class="setting">
  <div class="text">
    <strong>Skills, plugins and themes</strong>
    <p>Each keeps its previous version for rollback.</p>
  </div>
  <button class="btn sm" onclick={() => (app.settingsTab = 'integrations')}>Integrations</button>
</div>

<style>
  .notes {
    white-space: pre-wrap;
    background: var(--bg-sunken);
    border-radius: var(--radius-sm);
    padding: var(--s3);
    max-height: 200px;
    overflow: auto;
  }
</style>
