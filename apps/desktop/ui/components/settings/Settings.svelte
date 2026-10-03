<script lang="ts">
  import { X } from '@lucide/svelte';
  import { app, type SettingsTab } from '../../lib/app.svelte';
  import General from './General.svelte';
  import Providers from './Providers.svelte';
  import Combos from './Combos.svelte';
  import Permissions from './Permissions.svelte';
  import Optimization from './Optimization.svelte';
  import Instructions from './Instructions.svelte';
  import Integrations from './Integrations.svelte';
  import Remote from './Remote.svelte';
  import Appearance from './Appearance.svelte';
  import Updates from './Updates.svelte';
  import Privacy from './Privacy.svelte';
  import Environment from './Environment.svelte';
  import About from './About.svelte';

  const tabs: { id: SettingsTab; label: string; group: string }[] = [
    { id: 'general', label: 'General', group: 'App' },
    { id: 'appearance', label: 'Appearance', group: 'App' },
    { id: 'updates', label: 'Updates', group: 'App' },
    { id: 'providers', label: 'Providers', group: 'Models' },
    { id: 'combos', label: 'Combos', group: 'Models' },
    { id: 'instructions', label: 'Instructions', group: 'Models' },
    { id: 'permissions', label: 'Permissions', group: 'Safety' },
    { id: 'privacy', label: 'Privacy & data', group: 'Safety' },
    { id: 'optimization', label: 'Optimization', group: 'Advanced' },
    { id: 'integrations', label: 'MCP, skills & plugins', group: 'Advanced' },
    { id: 'environment', label: 'Environment', group: 'Advanced' },
    { id: 'remote', label: 'Remote access', group: 'Advanced' },
    { id: 'about', label: 'About', group: 'Advanced' },
  ];
  const close = () => (app.settingsTab = null);
</script>

<svelte:window
  onkeydown={(e) => {
    // Close on Escape wherever focus is, unless a nested dialog handles it.
    if (e.key === 'Escape' && !e.defaultPrevented && !document.querySelector('.modal')) close();
  }}
/>

<div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && close()}>
  <div class="sheet" role="dialog" aria-modal="true" aria-label="Settings" tabindex="-1">
    <nav aria-label="Settings sections">
      <h3 class="title">Settings</h3>
      {#each tabs as t, i (t.id)}
        {#if i === 0 || tabs[i - 1].group !== t.group}<div class="group">{t.group}</div>{/if}
        <button
          class="tab"
          aria-current={app.settingsTab === t.id ? 'page' : undefined}
          onclick={() => (app.settingsTab = t.id)}>{t.label}</button
        >
      {/each}
    </nav>
    <section class="panel">
      <button class="btn ghost icon close" onclick={close} aria-label="Close settings"><X size={16} /></button>
      {#if app.settingsTab === 'general'}<General />
      {:else if app.settingsTab === 'providers'}<Providers />
      {:else if app.settingsTab === 'combos'}<Combos />
      {:else if app.settingsTab === 'permissions'}<Permissions />
      {:else if app.settingsTab === 'optimization'}<Optimization />
      {:else if app.settingsTab === 'instructions'}<Instructions />
      {:else if app.settingsTab === 'integrations'}<Integrations />
      {:else if app.settingsTab === 'remote'}<Remote />
      {:else if app.settingsTab === 'appearance'}<Appearance />
      {:else if app.settingsTab === 'updates'}<Updates />
      {:else if app.settingsTab === 'privacy'}<Privacy />
      {:else if app.settingsTab === 'environment'}<Environment />
      {:else if app.settingsTab === 'about'}<About />{/if}
    </section>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 55;
    background: rgb(10 10 14 / 38%);
    display: grid;
    place-items: center;
    padding: var(--s5);
    animation: fade var(--duration) var(--ease);
  }
  .sheet {
    width: min(980px, 100%);
    height: min(720px, calc(100vh - 40px));
    display: flex;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-lg);
    overflow: hidden;
    outline: none;
  }
  nav {
    width: 210px;
    flex: none;
    background: var(--bg-sunken);
    border-right: 1px solid var(--border);
    padding: var(--s4) var(--s2);
    overflow: auto;
  }
  .title {
    padding: 0 var(--s2) var(--s2);
  }
  .group {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-faint);
    padding: var(--s3) var(--s2) var(--s1);
  }
  .tab {
    width: 100%;
    display: block;
    height: 30px;
    padding: 0 var(--s2);
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-muted);
    font: inherit;
    font-size: var(--fs-sm);
    text-align: left;
    cursor: pointer;
  }
  .tab:hover {
    color: var(--text);
    background: var(--surface-hover);
  }
  .tab[aria-current='page'] {
    color: var(--text);
    background: var(--surface);
    box-shadow: 0 0 0 1px var(--border);
    font-weight: 500;
  }
  .panel {
    flex: 1;
    overflow: auto;
    padding: var(--s6) var(--s7);
    position: relative;
  }
  .close {
    position: absolute;
    right: var(--s3);
    top: var(--s3);
  }
  .panel :global(h2) {
    margin-bottom: var(--s1);
  }
  .panel :global(.lede) {
    color: var(--text-muted);
    font-size: var(--fs-sm);
    margin-bottom: var(--s5);
  }
  .panel :global(.setting) {
    display: flex;
    align-items: center;
    gap: var(--s4);
    padding: var(--s3) 0;
    border-bottom: 1px solid var(--border);
  }
  .panel :global(.setting .text) {
    flex: 1;
    min-width: 0;
  }
  .panel :global(.setting .text p) {
    font-size: var(--fs-xs);
    color: var(--text-muted);
  }
  .panel :global(.setting .text strong) {
    font-size: var(--fs-sm);
    font-weight: 500;
  }
  .panel :global(.setting .select) {
    width: auto;
    min-width: 160px;
  }
  .panel :global(h3.sub) {
    margin: var(--s6) 0 var(--s2);
    font-size: var(--fs-md);
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
  @media (max-width: 760px) {
    .backdrop {
      padding: 0;
    }
    .sheet {
      height: 100vh;
      border-radius: 0;
      flex-direction: column;
    }
    nav {
      width: 100%;
      height: auto;
      display: flex;
      overflow-x: auto;
      padding: var(--s2);
      border-right: none;
      border-bottom: 1px solid var(--border);
    }
    .title,
    .group {
      display: none;
    }
    .tab {
      width: auto;
      white-space: nowrap;
    }
    .panel {
      padding: var(--s5) var(--s4);
    }
  }
</style>
