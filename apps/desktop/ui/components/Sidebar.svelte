<script lang="ts">
  import {
    Plus,
    Settings,
    ChevronDown,
    FolderOpen,
    FolderPlus,
    PanelLeft,
    MessageSquare,
    Target,
    Trash2,
    Search,
    Shield,
    ShieldAlert,
    ShieldCheck,
  } from '@lucide/svelte';
  import {
    app,
    attempt,
    newConversation,
    openConversation,
    openGoal,
    openProjectDialog,
    refresh,
    selectProject,
  } from '../lib/app.svelte';
  import { call } from '../lib/api';
  import { ago, keyLabel } from '../lib/format';
  import Logo from './Logo.svelte';

  let menuOpen = $state(false);

  const goalStateLabel: Record<string, string> = {
    planning: 'Planning',
    running: 'Running',
    verifying: 'Verifying',
    waiting_for_user: 'Needs you',
    paused: 'Paused',
    blocked: 'Blocked',
    complete: 'Done',
    stopped: 'Stopped',
    clarifying: 'Clarifying',
  };

  async function del(id: string, e: MouseEvent) {
    e.stopPropagation();
    await attempt(() => call('delete_conversation', { id }));
    if (app.conversationId === id) {
      app.conversationId = null;
      app.view = 'home';
    }
    await refresh();
  }

  const perm = $derived(app.settings.permission);
</script>

<nav class="sidebar" aria-label="Projects and conversations">
  <div class="brand">
    <Logo size={22} />
    <span class="name">Conductor</span>
    <span class="spacer"></span>
    <button
      class="btn ghost icon sm"
      aria-label="Hide sidebar"
      title="Hide sidebar"
      onclick={() => (app.sidebarOpen = false)}><PanelLeft size={16} /></button
    >
  </div>

  <div class="project">
    <button class="switcher" aria-haspopup="menu" aria-expanded={menuOpen} onclick={() => (menuOpen = !menuOpen)}>
      <span class="grow ellipsis">{app.project?.name ?? 'No project'}</span>
      <ChevronDown size={14} />
    </button>
    {#if menuOpen}
      <div class="menu" role="menu" tabindex="-1" onmouseleave={() => (menuOpen = false)}>
        {#each app.projects.slice(0, 8) as p (p.id)}
          <button
            role="menuitem"
            class:active={p.id === app.projectId}
            onclick={() => {
              selectProject(p.id);
              menuOpen = false;
            }}
          >
            <span class="ellipsis">{p.name}</span>
            <span class="faint xsmall ellipsis">{p.path}</span>
          </button>
        {/each}
        {#if app.projects.length}<hr class="divider tight" />{/if}
        <button
          role="menuitem"
          onclick={() => {
            menuOpen = false;
            void openProjectDialog();
          }}><span class="row"><FolderOpen size={14} />Open folder…</span></button
        >
        <button
          role="menuitem"
          onclick={() => {
            menuOpen = false;
            app.newProjectOpen = true;
          }}><span class="row"><FolderPlus size={14} />New project…</span></button
        >
      </div>
    {/if}
  </div>

  <div class="actions">
    <button
      class="btn new"
      onclick={() => {
        app.mode = app.mode === 'goal' ? 'chat' : app.mode;
        void newConversation();
      }}
      disabled={!app.project}
    >
      <Plus size={15} /> New chat
    </button>
    <button
      class="btn ghost icon"
      aria-label="New Goal"
      title="New Goal"
      disabled={!app.project}
      onclick={() => {
        app.mode = 'goal';
        app.view = 'home';
        app.conversationId = null;
        app.goalId = null;
      }}
    >
      <Target size={15} />
    </button>
    <button
      class="btn ghost icon"
      aria-label="Search and commands"
      title="Search and commands ({keyLabel('Mod+K')})"
      onclick={() => (app.paletteOpen = true)}><Search size={15} /></button
    >
  </div>

  <div class="lists">
    {#if app.projectGoals.length}
      <div class="section">Goals</div>
      <ul class="list">
        {#each app.projectGoals as g (g.id)}
          <li>
            <button
              class="item"
              class:active={app.goalId === g.id && app.view === 'goal'}
              onclick={() => openGoal(g.id)}
            >
              <span
                class="dot"
                class:running={['planning', 'running', 'verifying'].includes(g.state)}
                class:ok={g.state === 'complete'}
                class:warn={['paused', 'waiting_for_user', 'stopped'].includes(g.state)}
                class:bad={g.state === 'blocked'}
              ></span>
              <span class="grow ellipsis">{g.objective}</span>
              <span class="meta">{g.total ? `${g.done}/${g.total}` : goalStateLabel[g.state]}</span>
            </button>
          </li>
        {/each}
      </ul>
    {/if}

    <div class="section">Conversations</div>
    {#if !app.project}
      <p class="hint pad">Open a project to start.</p>
    {:else if !app.projectConversations.length}
      <p class="hint pad">No conversations yet.</p>
    {:else}
      <ul class="list">
        {#each app.projectConversations as c (c.id)}
          <li class="conv">
            <button
              class="item"
              class:active={app.conversationId === c.id && app.view === 'chat'}
              onclick={() => openConversation(c.id)}
            >
              {#if app.running[c.id]}<span class="dot running"></span>{:else}<MessageSquare
                  size={13}
                  class="faint"
                />{/if}
              <span class="grow ellipsis">{c.title}</span>
              <span class="meta">{ago(c.updated_at)}</span>
            </button>
            <button
              class="del btn ghost icon sm"
              aria-label="Delete conversation {c.title}"
              onclick={(e) => del(c.id, e)}><Trash2 size={13} /></button
            >
          </li>
        {/each}
      </ul>
    {/if}
  </div>

  <div class="foot">
    <button
      class="perm {perm}"
      title="Permissions"
      aria-label="Permissions: {perm === 'full_access'
        ? 'Full Access'
        : perm === 'auto_approve'
          ? 'Auto Approve'
          : 'Ask'}"
      onclick={() => (app.settingsTab = 'permissions')}
    >
      {#if perm === 'full_access'}<ShieldAlert size={14} />Full Access{:else if perm === 'auto_approve'}<ShieldCheck
          size={14}
        />Auto Approve{:else}<Shield size={14} />Ask{/if}
    </button>
    <span class="spacer"></span>
    <button
      class="btn ghost icon"
      aria-label="Settings"
      title="Settings ({keyLabel('Mod+,')})"
      onclick={() => (app.settingsTab = 'general')}><Settings size={16} /></button
    >
  </div>
</nav>

<style>
  .sidebar {
    width: var(--sidebar-w);
    flex: none;
    display: flex;
    flex-direction: column;
    background: var(--bg-sunken);
    border-right: 1px solid var(--border);
    min-height: 0;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: var(--s2);
    height: 52px;
    padding: 0 var(--s2) 0 var(--s4);
  }
  .name {
    font-weight: 600;
    letter-spacing: -0.01em;
  }
  .project {
    position: relative;
    padding: 0 var(--s3);
  }
  .switcher {
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--s2);
    height: 34px;
    padding: 0 var(--s3);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-sm);
    font-weight: 500;
    cursor: pointer;
    text-align: left;
  }
  .switcher:hover {
    border-color: var(--border-strong);
  }
  .menu {
    position: absolute;
    left: var(--s3);
    right: var(--s3);
    top: 38px;
    z-index: 20;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
    padding: var(--s1);
    display: flex;
    flex-direction: column;
  }
  .menu button {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0;
    width: 100%;
    padding: 6px var(--s2);
    border: none;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-sm);
    border-radius: var(--radius-sm);
    cursor: pointer;
    text-align: left;
  }
  .menu button:hover,
  .menu button.active {
    background: var(--surface-hover);
  }
  .menu .ellipsis {
    max-width: 100%;
  }
  .divider.tight {
    margin: var(--s1) 0;
  }
  .actions {
    display: flex;
    gap: var(--s1);
    padding: var(--s3);
  }
  .new {
    flex: 1;
    justify-content: flex-start;
  }
  .lists {
    flex: 1;
    overflow: auto;
    padding: 0 var(--s2) var(--s3);
  }
  .section {
    font-size: var(--fs-xs);
    font-weight: 600;
    color: var(--text-faint);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    padding: var(--s3) var(--s2) var(--s1);
  }
  .item {
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--s2);
    height: 32px;
    padding: 0 var(--s2);
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-sm);
    cursor: pointer;
    text-align: left;
  }
  .item:hover {
    background: var(--surface-hover);
  }
  .item.active {
    background: var(--surface);
    box-shadow: 0 0 0 1px var(--border);
  }
  .meta {
    font-size: 11px;
    color: var(--text-faint);
    flex: none;
  }
  .conv {
    position: relative;
  }
  .del {
    position: absolute;
    right: 2px;
    top: 3px;
    opacity: 0;
    background: var(--surface-hover);
  }
  .conv:hover .del,
  .del:focus-visible {
    opacity: 1;
  }
  .conv:hover .meta {
    visibility: hidden;
  }
  .pad {
    padding: 0 var(--s2);
  }
  .ellipsis {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .foot {
    display: flex;
    align-items: center;
    gap: var(--s2);
    padding: var(--s2) var(--s3);
    border-top: 1px solid var(--border);
  }
  .perm {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    padding: 0 var(--s2);
    border-radius: 999px;
    border: 1px solid var(--border);
    background: var(--surface);
    color: var(--text-muted);
    font: inherit;
    font-size: var(--fs-xs);
    font-weight: 500;
    cursor: pointer;
  }
  .perm.full_access {
    color: var(--danger);
    border-color: color-mix(in srgb, var(--danger) 40%, var(--border));
    background: var(--danger-soft);
  }
  .perm.auto_approve {
    color: var(--accent);
  }
  :global(.sidebar .faint) {
    color: var(--text-faint);
    flex: none;
  }
</style>
