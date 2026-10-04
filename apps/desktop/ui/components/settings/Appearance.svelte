<script lang="ts">
  import { FolderOpen, Trash2, Plus } from '@lucide/svelte';
  import { app, applyTheme, attempt, savePrefs } from '../../lib/app.svelte';
  import { call, chooseFolder } from '../../lib/api';
  import type { ThemeManifest } from '../../lib/types';
  import Toggle from '../Toggle.svelte';

  let themes = $state<ThemeManifest[]>([]);
  let source = $state('');
  const load = async () => (themes = await call<ThemeManifest[]>('themes_list').catch(() => []));
  $effect(() => {
    void load();
  });

  async function install() {
    const r = await attempt(() => call<{ name: string }>('themes_install', { source: source.trim() }));
    if (r) {
      source = '';
      await load();
    }
  }
  async function create() {
    const dir = await chooseFolder('Where should the new theme folder go?');
    if (!dir) return;
    const name = prompt('Theme name', 'my-theme');
    if (!name) return;
    await attempt(
      () => call('theme_scaffold', { dir: `${dir}/${name}`, name }),
      'Theme created. Edit theme.json, then install it here.',
    );
  }
  async function use(name: string) {
    await savePrefs({ theme_name: name });
    await applyTheme();
  }
</script>

<h2>Appearance</h2>
<p class="lede">Calm by default. Themes only change colours, type and spacing — they can't run code.</p>

<div class="setting">
  <div class="text">
    <strong>Mode</strong>
    <p>Follow the system or choose one.</p>
  </div>
  <div class="segmented" role="group" aria-label="Color mode">
    {#each ['system', 'light', 'dark'] as const as m (m)}
      <button aria-pressed={app.prefs?.theme_mode === m} onclick={() => attempt(() => savePrefs({ theme_mode: m }))}
        >{m[0].toUpperCase() + m.slice(1)}</button
      >
    {/each}
  </div>
</div>
<div class="setting">
  <div class="text">
    <strong>Interface size</strong>
    <p>Scales text and controls.</p>
  </div>
  <select
    class="select"
    value={String(app.prefs?.ui_scale ?? 1)}
    onchange={(e) => attempt(() => savePrefs({ ui_scale: Number((e.target as HTMLSelectElement).value) }))}
    aria-label="Interface size"
  >
    <option value="0.9">Compact</option><option value="1">Default</option><option value="1.1">Large</option><option
      value="1.25">Larger</option
    >
  </select>
</div>
<div class="setting">
  <div class="text">
    <strong>Reduce motion</strong>
    <p>Turns off transitions. Potato performance mode does this automatically.</p>
  </div>
  <Toggle
    checked={!!app.prefs?.reduced_motion}
    label="Reduce motion"
    onchange={(v) => attempt(() => savePrefs({ reduced_motion: v }))}
  />
</div>

<h3 class="sub">Themes</h3>
<div class="themes">
  <button class="theme" aria-pressed={!app.prefs?.theme_name} onclick={() => use('')}>
    <span class="sw" style="background:#fbfaf8"></span><span class="sw" style="background:#4b5cf0"></span><span
      class="sw"
      style="background:#131417"
    ></span>
    <span class="small">Conductor</span>
  </button>
  {#each themes as t (t.name)}
    <div class="theme-wrap">
      <button class="theme" aria-pressed={app.prefs?.theme_name === t.name} onclick={() => use(t.name)}>
        {#each ['bg', 'accent', 'text'] as k (k)}<span
            class="sw"
            style:background={t.light[k] ?? t.dark[k] ?? 'transparent'}
          ></span>{/each}
        <span class="small">{t.name}</span>
      </button>
      <button
        class="btn ghost icon sm rm"
        aria-label="Remove theme {t.name}"
        onclick={async () => {
          await attempt(() => call('themes_remove', { name: t.name }));
          if (app.prefs?.theme_name === t.name) await use('');
          await load();
        }}><Trash2 size={12} /></button
      >
    </div>
  {/each}
</div>
<div class="row install">
  <input
    class="input"
    bind:value={source}
    placeholder="Theme Git URL (https://…) or folder"
    aria-label="Theme source"
  />
  <button
    class="btn"
    aria-label="Browse"
    onclick={async () => {
      const p = await chooseFolder('Choose a theme folder');
      if (p) source = p;
    }}><FolderOpen size={14} /></button
  >
  <button class="btn primary" onclick={install} disabled={!source.trim()}>Install</button>
</div>
<button class="btn sm" onclick={create}><Plus size={13} /> Create a theme</button>
<p class="xsmall faint">See docs/THEMES.md for the format, starter template and sharing.</p>

<style>
  .themes {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s2);
    margin-bottom: var(--s3);
  }
  .theme-wrap {
    position: relative;
  }
  .theme {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 36px;
    padding: 0 var(--s3);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--bg);
    color: var(--text);
    font: inherit;
    cursor: pointer;
  }
  .theme[aria-pressed='true'] {
    border-color: var(--accent);
    box-shadow: 0 0 0 2px var(--accent-soft);
  }
  .sw {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    border: 1px solid var(--border);
  }
  .theme .small {
    margin-left: 4px;
  }
  .rm {
    position: absolute;
    top: -8px;
    right: -8px;
    background: var(--bg-elevated);
    opacity: 0;
  }
  .theme-wrap:hover .rm {
    opacity: 1;
  }
  .install {
    margin-bottom: var(--s2);
  }
</style>
