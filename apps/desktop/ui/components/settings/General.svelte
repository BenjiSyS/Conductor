<script lang="ts">
  import { app, attempt, saveSettings, savePrefs } from '../../lib/app.svelte';
  import { call } from '../../lib/api';
  import type { Performance } from '../../lib/types';
  import Toggle from '../Toggle.svelte';

  let hw = $state<{ recommended: Performance; reason: string } | null>(null);
  let registered = $state<boolean | null>(null);
  $effect(() => {
    call<{ emergency_shortcut_registered: boolean }>('app_info').then(
      (i) => (registered = i.emergency_shortcut_registered),
    );
  });
  $effect(() => {
    call<typeof hw>('hardware').then((h) => (hw = h));
  });
</script>

<h2>General</h2>
<p class="lede">How Conductor behaves on this computer.</p>

<div class="setting">
  <div class="text">
    <strong>Performance profile</strong>
    <p>
      Potato keeps memory and CPU low (one agent, minimal animation). {#if hw}Recommended here: {hw.recommended} — {hw.reason}.{/if}
    </p>
  </div>
  <div class="segmented" role="group" aria-label="Performance profile">
    {#each ['potato', 'balanced', 'maximum'] as const as p (p)}
      <button
        aria-pressed={app.settings.performance === p}
        onclick={() => attempt(() => saveSettings({ performance: p }))}>{p[0].toUpperCase() + p.slice(1)}</button
      >
    {/each}
  </div>
</div>

<div class="setting">
  <div class="text">
    <strong>When I close the window</strong>
    <p>Keeping Conductor in the background lets Goals keep running. Quit from the tray icon.</p>
  </div>
  <select
    class="select"
    value={app.prefs?.close_behavior}
    onchange={(e) =>
      attempt(() =>
        savePrefs({ close_behavior: (e.target as HTMLSelectElement).value as 'background' | 'exit' | 'ask' }),
      )}
    aria-label="Close behavior"
  >
    <option value="background">Keep running in background</option>
    <option value="exit">Quit completely</option>
    <option value="ask">Ask each time</option>
  </select>
</div>

<div class="setting">
  <div class="text">
    <strong>Launch at login</strong>
    <p>Starts quietly in the tray so background Goals and remote access are available. Off by default.</p>
  </div>
  <Toggle
    checked={!!app.prefs?.launch_at_login}
    label="Launch at login"
    onchange={(v) => attempt(() => savePrefs({ launch_at_login: v }))}
  />
</div>

<div class="setting">
  <div class="text">
    <strong>Default model or Combo</strong>
    <p>Used for new conversations.</p>
  </div>
  <select
    class="select"
    value={app.prefs?.default_target ?? ''}
    onchange={(e) => attempt(() => savePrefs({ default_target: (e.target as HTMLSelectElement).value || null }))}
    aria-label="Default target"
  >
    <option value="">Most recently used</option>
    {#each app.combos as c (c.id)}<option value="combo:{c.id}">Combo · {c.name}</option>{/each}
    {#each app.models as m (m.provider + m.model)}<option value="model:{m.provider}/{m.model}">{m.display}</option
      >{/each}
  </select>
</div>

<div class="setting">
  <div class="text">
    <strong>Emergency stop shortcut</strong>
    <p>
      Halts every agent, command and Goal immediately, even when Conductor isn't focused.
      {#if registered === false}<span class="warn"
          >Another app is using this shortcut, so it only works inside Conductor. The tray menu's “Stop all work” always
          works.</span
        >{/if}
    </p>
  </div>
  <span class="kbd">{app.prefs?.emergency_shortcut.replace('CmdOrCtrl', 'Ctrl')}</span>
</div>

<div class="setting">
  <div class="text">
    <strong>Setup wizard</strong>
    <p>Run the first-time setup again.</p>
  </div>
  <button
    class="btn"
    onclick={() => {
      app.settingsTab = null;
      app.wizardOpen = true;
    }}>Run setup</button
  >
</div>

<style>
  .warn {
    color: var(--warning);
    display: block;
    margin-top: 2px;
  }
</style>
