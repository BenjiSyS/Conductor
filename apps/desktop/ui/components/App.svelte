<script lang="ts">
  import { onMount } from 'svelte';
  import { app, emergencyStop, init, newConversation, openProjectDialog, stopCurrent } from '../lib/app.svelte';
  import { call } from '../lib/api';
  import { matchesBinding } from '../lib/format';
  import Sidebar from './Sidebar.svelte';
  import Home from './Home.svelte';
  import ChatView from './ChatView.svelte';
  import GoalView from './GoalView.svelte';
  import Composer from './Composer.svelte';
  import Inspector from './Inspector.svelte';
  import GitPanel from './GitPanel.svelte';
  import HistoryPanel from './HistoryPanel.svelte';
  import MemoryPanel from './MemoryPanel.svelte';
  import PreviewPanel from './PreviewPanel.svelte';
  import Palette from './Palette.svelte';
  import Toasts from './Toasts.svelte';
  import Wizard from './Wizard.svelte';
  import NewProject from './NewProject.svelte';
  import Settings from './settings/Settings.svelte';
  import ApprovalBar from './ApprovalBar.svelte';
  import Modal from './Modal.svelte';
  import Logo from './Logo.svelte';

  onMount(() => {
    if (window.innerWidth < 900) app.sidebarOpen = false;
    void init();
  });
  let narrow = $state(false);
  $effect(() => {
    const mq = window.matchMedia('(max-width: 899px)');
    narrow = mq.matches;
    const on = (e: MediaQueryListEvent) => {
      narrow = e.matches;
      if (e.matches) app.sidebarOpen = false;
    };
    mq.addEventListener('change', on);
    return () => mq.removeEventListener('change', on);
  });

  function keydown(e: KeyboardEvent) {
    const kb = app.prefs?.keybindings ?? {};
    const overlay = app.paletteOpen || app.settingsTab || app.wizardOpen || app.newProjectOpen;
    if (matchesBinding(e, kb.palette ?? 'Mod+K')) {
      e.preventDefault();
      app.paletteOpen = !app.paletteOpen;
    } else if (matchesBinding(e, kb.settings ?? 'Mod+,')) {
      e.preventDefault();
      app.settingsTab = app.settingsTab ? null : 'general';
    } else if (matchesBinding(e, kb.new_chat ?? 'Mod+N') && !overlay) {
      e.preventDefault();
      void newConversation();
    } else if (matchesBinding(e, kb.open_project ?? 'Mod+O') && !overlay) {
      e.preventDefault();
      void openProjectDialog();
    } else if (matchesBinding(e, kb.inspector ?? 'Mod+I') && !overlay) {
      e.preventDefault();
      app.inspectorOpen = !app.inspectorOpen;
    } else if (matchesBinding(e, kb.toggle_sidebar ?? 'Mod+B')) {
      e.preventDefault();
      app.sidebarOpen = !app.sidebarOpen;
    } else if (e.key === 'Escape' && !overlay && app.anyRunning && document.activeElement?.tagName !== 'INPUT') {
      void stopCurrent();
    } else if (
      matchesBinding(e, (app.prefs?.emergency_shortcut ?? 'CmdOrCtrl+Alt+Shift+X').replace('CmdOrCtrl', 'Mod'))
    ) {
      e.preventDefault();
      void emergencyStop();
    }
  }
</script>

<svelte:window onkeydown={keydown} />

{#if !app.ready}
  <div class="boot" aria-busy="true">
    <Logo size={44} />
  </div>
{:else}
  <div class="shell" class:collapsed={!app.sidebarOpen}>
    {#if app.sidebarOpen}
      {#if narrow}<div class="scrim" role="presentation" onclick={() => (app.sidebarOpen = false)}></div>{/if}
      <div class="side" class:overlay={narrow}><Sidebar /></div>
    {/if}
    <main class="main">
      {#if !app.native}
        <div class="preview-banner" role="note">
          Browser preview — sample data only. Open the Conductor desktop app to work with real projects, providers and
          their live model lists.
        </div>
      {/if}
      <div class="stage">
        <div class="center">
          {#if app.view === 'goal' && app.goalId}
            <GoalView />
          {:else if app.view === 'chat' && app.conversation}
            <ChatView />
          {:else}
            <Home />
          {/if}
          <div class="dock">
            <ApprovalBar />
            {#if app.view !== 'goal' && app.project}
              <Composer />
            {/if}
          </div>
        </div>
        {#if app.inspectorOpen}<Inspector />{/if}
        {#if app.gitOpen}<GitPanel />{/if}
        {#if app.historyOpen}<HistoryPanel />{/if}
        {#if app.memoryOpen}<MemoryPanel />{/if}
        {#if app.previewOpen}<PreviewPanel />{/if}
      </div>
    </main>
  </div>
{/if}

{#if app.paletteOpen}<Palette />{/if}
{#if app.settingsTab}<Settings />{/if}
{#if app.newProjectOpen}<NewProject />{/if}
{#if app.wizardOpen && app.ready}<Wizard />{/if}
{#if app.closePrompt !== null}
  <Modal title="Close Conductor?" onclose={() => (app.closePrompt = null)} width={440}>
    <p class="muted">
      {app.closePrompt
        ? 'Work is still running. Keep it going in the background, or quit and stop it?'
        : 'Keep Conductor running in the background or quit completely?'}
    </p>
    {#snippet footer()}
      <button
        class="btn"
        onclick={() => {
          app.closePrompt = null;
          void call('quit_app');
        }}>Quit</button
      >
      <button
        class="btn primary"
        onclick={() => {
          app.closePrompt = null;
          void call('hide_window');
        }}>Keep running</button
      >
    {/snippet}
  </Modal>
{/if}
<Toasts />

<style>
  .boot {
    height: 100%;
    display: grid;
    place-items: center;
    opacity: 0.85;
  }
  .shell {
    display: flex;
    height: 100%;
  }
  .side {
    display: flex;
    height: 100%;
  }
  .side.overlay {
    position: fixed;
    left: 0;
    top: 0;
    bottom: 0;
    z-index: 45;
    box-shadow: var(--shadow-lg);
  }
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 44;
    background: rgb(10 10 14 / 30%);
  }
  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .preview-banner {
    font-size: var(--fs-xs);
    text-align: center;
    padding: 6px var(--s3);
    color: var(--text-muted);
    background: var(--bg-sunken);
    border-bottom: 1px solid var(--border);
  }
  .stage {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .center {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .dock {
    flex: none;
    padding: 0 var(--s5) var(--s4);
  }
  @media (max-width: 760px) {
    .dock {
      padding: 0 var(--s3) var(--s3);
    }
  }
</style>
