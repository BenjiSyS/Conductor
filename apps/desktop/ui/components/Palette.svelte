<script lang="ts">
  import { Search } from '@lucide/svelte';
  import {
    app,
    emergencyStop,
    newConversation,
    openConversation,
    openGoal,
    openProjectDialog,
    savePrefs,
    selectProject,
    stopCurrent,
    togglePanel,
  } from '../lib/app.svelte';
  import { call } from '../lib/api';
  import { keyLabel } from '../lib/format';

  interface Cmd {
    id: string;
    label: string;
    group: string;
    hint?: string;
    run: () => void;
  }

  let q = $state('');
  let sel = $state(0);
  let input: HTMLInputElement | undefined = $state();
  $effect(() => queueMicrotask(() => input?.focus()));

  const close = () => (app.paletteOpen = false);
  const kb = (k: string, d: string) => keyLabel(app.prefs?.keybindings?.[k] ?? d);

  const commands = $derived.by<Cmd[]>(() => {
    const c: Cmd[] = [
      {
        id: 'open',
        label: 'Open project…',
        group: 'Project',
        hint: kb('open_project', 'Mod+O'),
        run: () => void openProjectDialog(),
      },
      { id: 'new-project', label: 'New project…', group: 'Project', run: () => (app.newProjectOpen = true) },
      {
        id: 'new-chat',
        label: 'New chat',
        group: 'Work',
        hint: kb('new_chat', 'Mod+N'),
        run: () => void newConversation('chat'),
      },
      {
        id: 'plan',
        label: 'Plan something',
        group: 'Work',
        run: () => {
          app.draft = '/plan ';
          app.view = 'home';
          app.conversationId = null;
        },
      },
      {
        id: 'goal',
        label: 'Start a Goal',
        group: 'Work',
        run: () => {
          app.draft = '/goal ';
          app.view = 'home';
          app.conversationId = null;
          app.goalId = null;
        },
      },
      {
        id: 'agent',
        label: 'Agent mode',
        group: 'Work',
        run: () => {
          app.mode = 'agent';
        },
      },
      {
        id: 'tests',
        label: 'Run tests and fix failures',
        group: 'Work',
        run: () => {
          app.mode = 'agent';
          app.draft = 'Run the test suite. If anything fails, find the root cause and fix it, then re-run the tests.';
        },
      },
      { id: 'stop', label: 'Stop current work', group: 'Work', hint: 'Esc', run: () => void stopCurrent() },
      {
        id: 'estop',
        label: 'Emergency stop — halt everything',
        group: 'Work',
        hint: keyLabel((app.prefs?.emergency_shortcut ?? 'CmdOrCtrl+Alt+Shift+X').replace('CmdOrCtrl', 'Mod')),
        run: () => void emergencyStop(),
      },
      {
        id: 'inspector',
        label: 'Context Inspector',
        group: 'View',
        hint: kb('inspector', 'Mod+I'),
        run: () => togglePanel('inspector'),
      },
      { id: 'git', label: 'Git, GitHub & checkpoints', group: 'View', run: () => togglePanel('git') },
      { id: 'preview', label: 'Preview (local dev server)', group: 'View', run: () => togglePanel('preview') },
      { id: 'history', label: 'History', group: 'View', run: () => togglePanel('history') },
      { id: 'memory', label: 'Project memory & decisions', group: 'View', run: () => togglePanel('memory') },
      {
        id: 'folder',
        label: 'Reveal project folder',
        group: 'View',
        run: () => app.project && void call('reveal_path', { path: app.project.path }),
      },
      {
        id: 'theme',
        label: 'Toggle light / dark',
        group: 'View',
        run: () =>
          void savePrefs({
            theme_mode:
              document.documentElement.getAttribute('data-theme') === 'dark' ||
              (!document.documentElement.getAttribute('data-theme') &&
                matchMedia('(prefers-color-scheme: dark)').matches)
                ? 'light'
                : 'dark',
          }),
      },
      {
        id: 'settings',
        label: 'Settings',
        group: 'Settings',
        hint: kb('settings', 'Mod+,'),
        run: () => (app.settingsTab = 'general'),
      },
      { id: 'providers', label: 'Providers', group: 'Settings', run: () => (app.settingsTab = 'providers') },
      { id: 'combos', label: 'Combos', group: 'Settings', run: () => (app.settingsTab = 'combos') },
      { id: 'mcp', label: 'MCP Doctor', group: 'Settings', run: () => (app.settingsTab = 'integrations') },
      { id: 'env', label: 'Environment Doctor', group: 'Settings', run: () => (app.settingsTab = 'environment') },
      { id: 'perm', label: 'Permissions', group: 'Settings', run: () => (app.settingsTab = 'permissions') },
      { id: 'remote', label: 'Remote access', group: 'Settings', run: () => (app.settingsTab = 'remote') },
    ];
    for (const combo of app.combos)
      c.push({
        id: `combo-${combo.id}`,
        label: `Use Combo: ${combo.name}`,
        group: 'Models',
        run: () => (app.target = `combo:${combo.id}`),
      });
    for (const m of app.models)
      c.push({
        id: `model-${m.provider}/${m.model}`,
        label: `Use model: ${m.display}`,
        group: 'Models',
        run: () => (app.target = `model:${m.provider}/${m.model}`),
      });
    for (const e of ['auto', 'low', 'medium', 'high'])
      c.push({ id: `effort-${e}`, label: `Effort: ${e}`, group: 'Models', run: () => (app.effort = e) });
    for (const p of app.projects)
      c.push({
        id: `project-${p.id}`,
        label: `Switch to ${p.name}`,
        group: 'Projects',
        run: () => selectProject(p.id),
      });
    for (const g of app.goals.slice(0, 20))
      c.push({ id: `goal-${g.id}`, label: `Goal: ${g.objective}`, group: 'Goals', run: () => openGoal(g.id) });
    for (const cv of app.projectConversations.slice(0, 30))
      c.push({ id: `conv-${cv.id}`, label: cv.title, group: 'Conversations', run: () => openConversation(cv.id) });
    return c;
  });

  function score(label: string, query: string): number {
    if (!query) return 1;
    const l = label.toLowerCase();
    const qq = query.toLowerCase();
    if (l.startsWith(qq)) return 3;
    if (l.includes(qq)) return 2;
    let i = 0;
    for (const ch of l) if (ch === qq[i]) i++;
    return i === qq.length ? 1 : 0;
  }
  const results = $derived(
    commands
      .map((c) => ({ c, s: score(c.label, q.trim()) }))
      .filter((x) => x.s > 0)
      .sort((a, b) => b.s - a.s)
      .slice(0, 40)
      .map((x) => x.c),
  );
  $effect(() => {
    void q;
    sel = 0;
  });

  function key(e: KeyboardEvent) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      sel = Math.min(sel + 1, results.length - 1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      sel = Math.max(sel - 1, 0);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      const c = results[sel];
      if (c) {
        close();
        c.run();
      }
    } else if (e.key === 'Escape') {
      e.preventDefault();
      close();
    }
  }
</script>

<div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && close()}>
  <div class="palette" role="dialog" aria-modal="true" aria-label="Command palette">
    <div class="search">
      <Search size={16} />
      <input
        bind:this={input}
        bind:value={q}
        onkeydown={key}
        placeholder="Type a command, model, project or conversation…"
        aria-label="Command"
        aria-controls="palette-list"
      />
    </div>
    <ul class="list" id="palette-list" role="listbox">
      {#each results as c, i (c.id)}
        {#if i === 0 || results[i - 1].group !== c.group}<li class="group" role="presentation">{c.group}</li>{/if}
        <li role="option" aria-selected={i === sel}>
          <button
            class:sel={i === sel}
            onmouseenter={() => (sel = i)}
            onclick={() => {
              close();
              c.run();
            }}
          >
            <span class="grow">{c.label}</span>
            {#if c.hint}<span class="kbd">{c.hint}</span>{/if}
          </button>
        </li>
      {:else}
        <li class="none muted small">No matches</li>
      {/each}
    </ul>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgb(10 10 14 / 30%);
    z-index: 60;
    display: flex;
    justify-content: center;
    padding-top: 14vh;
    animation: fade var(--duration) var(--ease);
  }
  .palette {
    width: min(620px, calc(100vw - 32px));
    max-height: 60vh;
    display: flex;
    flex-direction: column;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-lg);
    overflow: hidden;
    align-self: flex-start;
  }
  .search {
    display: flex;
    align-items: center;
    gap: var(--s3);
    padding: 0 var(--s4);
    border-bottom: 1px solid var(--border);
    color: var(--text-faint);
  }
  .search input {
    flex: 1;
    height: 50px;
    border: none;
    outline: none;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-lg);
  }
  ul {
    overflow: auto;
    padding: var(--s1);
  }
  .group {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-faint);
    padding: var(--s2) var(--s3) 2px;
  }
  li button {
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--s2);
    height: 36px;
    padding: 0 var(--s3);
    border: none;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-sm);
    border-radius: var(--radius-sm);
    cursor: pointer;
    text-align: left;
  }
  li button.sel {
    background: var(--accent-soft);
  }
  .none {
    padding: var(--s4);
    text-align: center;
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
</style>
