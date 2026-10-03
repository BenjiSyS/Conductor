<script lang="ts">
  import { FolderOpen, FolderPlus, GitBranch, PanelLeft, Plug, Target, Brain } from '@lucide/svelte';
  import { app, openGoal, openProjectDialog, openProjectPath } from '../lib/app.svelte';
  import { call } from '../lib/api';
  import { ago } from '../lib/format';
  import Logo from './Logo.svelte';

  interface ProjectInfo {
    detection: {
      kinds: string[];
      is_git: boolean;
      languages: string[];
      suggested_test_commands: string[];
      has_conductor_toml: boolean;
    };
    config_error: string | null;
    branch: string | null;
    resume: {
      last_goal: { id: string; objective: string; state: string } | null;
      unfinished_goals: number;
      last_decision: string | null;
      attention: string | null;
    } | null;
  }
  let info = $state<ProjectInfo | null>(null);

  $effect(() => {
    const pid = app.projectId;
    info = null;
    if (pid)
      call<ProjectInfo>('project_info', { projectId: pid })
        .then((i) => (info = i))
        .catch(() => {});
  });

  const suggestions: Record<string, string[]> = {
    chat: ['Explain how this project is structured', 'Where is the entry point?', 'What would you improve first?'],
    plan: [
      'Plan adding a settings page',
      'Plan a migration to the latest framework version',
      'Plan test coverage for the core module',
    ],
    agent: ['Fix the failing tests', 'Add input validation to the API', 'Write a README quick start'],
    goal: ['Build a small multiplayer lobby', 'Make all tests pass and add CI', 'Prepare a release'],
  };
</script>

<section class="home">
  {#if !app.sidebarOpen}
    <button class="btn ghost icon sidebar-toggle" aria-label="Show sidebar" onclick={() => (app.sidebarOpen = true)}
      ><PanelLeft size={16} /></button
    >
  {/if}
  {#if !app.project}
    <div class="welcome">
      <Logo size={56} />
      <h1>Conductor</h1>
      <p class="muted lead">Open a project, choose a model or Combo, and say what you want.</p>
      <div class="row cta">
        <button class="btn primary lg" onclick={openProjectDialog}><FolderOpen size={16} /> Open folder</button>
        <button class="btn lg" onclick={() => (app.newProjectOpen = true)}><FolderPlus size={16} /> New project</button>
      </div>
      {#if app.projects.length}
        <div class="recent">
          <div class="section-title">Recent</div>
          {#each app.projects.slice(0, 5) as p (p.id)}
            <button class="recent-item" onclick={() => openProjectPath(p.path)}>
              <span class="grow">{p.name}</span><span class="faint xsmall mono">{p.path}</span>
            </button>
          {/each}
        </div>
      {/if}
    </div>
  {:else}
    <div class="project-home">
      <header>
        <h2>{app.project.name}</h2>
        <div class="row wrap meta">
          <span class="mono faint xsmall">{app.project.path}</span>
          {#if info?.branch}<span class="badge"><GitBranch size={12} />{info.branch}</span>{/if}
          {#each info?.detection.languages ?? [] as l}<span class="badge">{l}</span>{/each}
          {#if info?.detection.has_conductor_toml}<span class="badge accent">conductor.toml</span>{/if}
        </div>
        {#if info?.config_error}<p class="warn small">conductor.toml: {info.config_error}</p>{/if}
      </header>

      {#if !app.connected.length}
        <div class="card notice">
          <Plug size={18} />
          <div class="grow">
            <h4>Connect a provider</h4>
            <p class="muted small">Add an OpenAI, Anthropic, Gemini or local (OpenAI-compatible) provider to start.</p>
          </div>
          <button class="btn primary" onclick={() => (app.settingsTab = 'providers')}>Connect</button>
        </div>
      {/if}

      {#if info?.resume && (info.resume.last_goal || info.resume.last_decision)}
        <div class="card resume">
          <div class="section-title">Where you left off</div>
          {#if info.resume.last_goal}
            <button class="resume-goal" onclick={() => info?.resume?.last_goal && openGoal(info.resume.last_goal.id)}>
              <Target size={14} />
              <span class="grow">{info.resume.last_goal.objective}</span>
              <span class="badge">{info.resume.last_goal.state.replace(/_/g, ' ')}</span>
            </button>
          {/if}
          {#if info.resume.last_decision}<p class="small muted">
              <Brain size={13} /> Last decision: {info.resume.last_decision}
            </p>{/if}
          {#if info.resume.attention}<p class="small">{info.resume.attention}</p>{/if}
        </div>
      {/if}

      <div class="intro">
        {#if app.mode === 'goal'}
          <h3>Start a Goal</h3>
          <p class="muted small">
            Describe the outcome. Conductor asks only what matters, plans tasks, assigns models, verifies with your
            checks, and keeps going until it's done — even in the background.
          </p>
        {:else if app.mode === 'agent'}
          <h3>Agent</h3>
          <p class="muted small">The model can read and edit files and run commands under your permission policy.</p>
        {:else if app.mode === 'plan'}
          <h3>Plan</h3>
          <p class="muted small">Understand and design before changing anything. Plan mode never edits files.</p>
        {:else}
          <h3>Ask anything about {app.project.name}</h3>
          <p class="muted small">Only the relevant files are sent — see the context budget under the prompt.</p>
        {/if}
        <div class="chips">
          {#each suggestions[app.mode] as s}
            <button class="chip" onclick={() => (app.draft = s)}>{s}</button>
          {/each}
        </div>
      </div>

      {#if app.history.filter((h) => h.project_id === app.projectId).length}
        <div class="recent-activity">
          <div class="section-title">Recent activity</div>
          {#each app.history.filter((h) => h.project_id === app.projectId).slice(0, 4) as h (h.id)}
            <div class="row small">
              <span class="grow muted">{h.summary}</span><span class="faint xsmall">{ago(h.created_at)}</span>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</section>

<style>
  .home {
    flex: 1;
    overflow: auto;
    position: relative;
  }
  .sidebar-toggle {
    position: absolute;
    left: var(--s3);
    top: var(--s3);
  }
  .welcome {
    max-width: 460px;
    margin: 12vh auto 0;
    text-align: center;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--s3);
    padding: 0 var(--s4);
  }
  .lead {
    font-size: var(--fs-lg);
    max-width: 380px;
  }
  .cta {
    margin-top: var(--s3);
  }
  .recent {
    width: 100%;
    margin-top: var(--s7);
    text-align: left;
  }
  .recent-item {
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--s3);
    padding: var(--s2) var(--s3);
    border: none;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-sm);
    border-radius: var(--radius-sm);
    cursor: pointer;
    text-align: left;
  }
  .recent-item:hover {
    background: var(--surface-hover);
  }
  .section-title {
    font-size: var(--fs-xs);
    font-weight: 600;
    color: var(--text-faint);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin-bottom: var(--s2);
  }
  .project-home {
    max-width: var(--content-w);
    margin: 0 auto;
    padding: var(--s8) var(--s5) var(--s5);
    display: flex;
    flex-direction: column;
    gap: var(--s5);
  }
  .meta {
    margin-top: var(--s2);
  }
  .warn {
    color: var(--warning);
    margin-top: var(--s2);
  }
  .notice {
    display: flex;
    align-items: center;
    gap: var(--s3);
  }
  .resume-goal {
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--s2);
    padding: var(--s2);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-sm);
    cursor: pointer;
    text-align: left;
    margin-bottom: var(--s2);
  }
  .intro {
    margin-top: var(--s4);
  }
  .intro h3 {
    margin-bottom: var(--s1);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s2);
    margin-top: var(--s4);
  }
  .chip {
    border: 1px solid var(--border);
    background: var(--surface);
    color: var(--text-muted);
    border-radius: 999px;
    padding: 6px var(--s3);
    font: inherit;
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .chip:hover {
    color: var(--text);
    border-color: var(--border-strong);
  }
  .recent-activity .row {
    padding: 3px 0;
  }
  @media (max-width: 760px) {
    .project-home {
      padding: var(--s7) var(--s3) var(--s3);
    }
  }
</style>
