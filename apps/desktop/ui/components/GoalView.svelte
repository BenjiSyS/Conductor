<script lang="ts">
  import {
    Play,
    Square,
    Trash2,
    PanelLeft,
    ChevronRight,
    Check,
    X,
    Circle,
    LoaderCircle,
    Pencil,
    RotateCcw,
    Layers,
    GitBranch,
    Monitor,
  } from '@lucide/svelte';
  import { app, attempt, refreshGoals, succeeded, toast, togglePanel } from '../lib/app.svelte';
  import { call } from '../lib/api';
  import { markdown, modelLabel, roleLabels } from '../lib/format';
  import type { GoalContract, GoalRecord, TaskNode } from '../lib/types';

  let rec = $state<GoalRecord | null>(null);
  let expanded = $state<Record<string, boolean>>({});
  let contractOpen = $state(false);
  let editing = $state(false);
  let draft = $state<{
    objective: string;
    requirements: string;
    constraints: string;
    acceptance: string;
    checks: string;
  } | null>(null);
  let now = $state(Date.now() / 1000);

  async function load() {
    if (!app.goalId) return;
    rec = await call<GoalRecord>('goal_get', { id: app.goalId }).catch(() => null);
  }
  $effect(() => {
    void app.goalId;
    void load();
  });
  // Refresh on engine events (debounced).
  $effect(() => {
    void app.goalVersion;
    const t = setTimeout(load, 250);
    return () => clearTimeout(t);
  });
  $effect(() => {
    const i = setInterval(() => (now = Date.now() / 1000), 1000);
    return () => clearInterval(i);
  });

  const g = $derived(rec?.goal);
  const active = $derived(!!g && ['planning', 'running', 'verifying'].includes(g.state));
  const done = $derived(g?.graph.tasks.filter((t) => t.status === 'done').length ?? 0);
  const total = $derived(g?.graph.tasks.length ?? 0);
  const contractRows = $derived<[string, string[]][]>(
    g
      ? [
          ['Requirements', g.contract.requirements],
          ['Constraints', g.contract.constraints],
          ['Target platforms', g.contract.platforms],
          ['Acceptance', g.contract.acceptance],
          ['Decisions', g.contract.decisions],
        ]
      : [],
  );
  const activity = $derived(app.goalId ? (app.activity[app.goalId] ?? []) : []);

  const stateLabel: Record<string, string> = {
    clarifying: 'Clarifying',
    planning: 'Planning',
    running: 'Running',
    verifying: 'Verifying',
    waiting_for_user: 'Needs you',
    paused: 'Paused',
    blocked: 'Blocked',
    complete: 'Complete',
    stopped: 'Stopped',
  };

  async function start() {
    if (!g) return;
    const ok = await succeeded(() => call('goal_start', { id: g.id }));
    if (ok) toast(g.state === 'planning' ? 'Goal started' : 'Goal resumed');
    await refreshGoals();
    await load();
  }
  async function stop() {
    if (!g) return;
    await attempt(() => call('goal_stop', { id: g.id }));
  }
  async function remove() {
    if (!g || !confirm('Delete this Goal? Project files are not affected.')) return;
    await attempt(() => call('goal_delete', { id: g.id }));
    await refreshGoals();
    app.goalId = null;
    app.view = 'home';
  }
  function edit() {
    if (!g) return;
    const c = g.contract;
    draft = {
      objective: c.objective,
      requirements: c.requirements.join('\n'),
      constraints: c.constraints.join('\n'),
      acceptance: c.acceptance.join('\n'),
      checks: c.done_checks.map((d) => d.command ?? d.text ?? '').join('\n'),
    };
    editing = true;
    contractOpen = true;
  }
  async function saveContract() {
    if (!g || !draft) return;
    const lines = (s: string) =>
      s
        .split('\n')
        .map((x) => x.trim())
        .filter(Boolean);
    const contract: GoalContract = {
      ...g.contract,
      objective: draft.objective.trim(),
      requirements: lines(draft.requirements),
      constraints: lines(draft.constraints),
      acceptance: lines(draft.acceptance),
      done_checks: lines(draft.checks).map((c) => ({ kind: 'command', command: c })),
    };
    const r = await attempt(
      () => call<GoalRecord>('goal_update_contract', { id: g.id, contract }),
      'Goal Contract updated',
    );
    if (r) rec = r;
    editing = false;
  }
  async function restoreCheckpoint() {
    if (
      !rec?.checkpoint ||
      !confirm(
        'Restore the project to how it was before this Goal started? The current state is saved as a new checkpoint first, so this can be undone.',
      )
    )
      return;
    await attempt(
      () => call('checkpoint_restore', { projectId: rec!.project_id, id: rec!.checkpoint }),
      'Restored. The previous state was saved as a checkpoint.',
    );
  }
  function elapsed(t: TaskNode) {
    if (!t.started_at) return '';
    const s = Math.max(0, Math.round((t.finished_at ?? now) - t.started_at));
    return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${s % 60}s`;
  }
</script>

<header class="topbar">
  {#if !app.sidebarOpen}<button
      class="btn ghost icon sm"
      aria-label="Show sidebar"
      onclick={() => (app.sidebarOpen = true)}><PanelLeft size={16} /></button
    >{/if}
  <span class="badge state {g?.state}">
    {#if active}<span class="dot running"></span>{/if}
    {g ? stateLabel[g.state] : '…'}
  </span>
  <span class="title">{g?.contract.objective ?? ''}</span>
  <span class="spacer"></span>
  <button class="btn ghost icon sm" aria-label="Context inspector panel" onclick={() => togglePanel('inspector')}
    ><Layers size={15} /></button
  >
  <button class="btn ghost icon sm" aria-label="Preview panel" onclick={() => togglePanel('preview')}
    ><Monitor size={15} /></button
  >
  <button class="btn ghost icon sm" aria-label="Git panel" onclick={() => togglePanel('git')}
    ><GitBranch size={15} /></button
  >
  {#if active}
    <button class="btn" onclick={stop}><Square size={13} fill="currentColor" /> Stop</button>
  {:else if g && g.state !== 'complete'}
    <button class="btn primary" onclick={start}><Play size={13} /> {g.graph.tasks.length ? 'Resume' : 'Start'}</button>
  {/if}
  <button class="btn ghost icon sm" aria-label="Delete Goal" onclick={remove} disabled={active}
    ><Trash2 size={15} /></button
  >
</header>

{#if g}
  <div class="scroll">
    <div class="wrap">
      {#if total}
        <div class="progress" aria-label="Progress {done} of {total}">
          <div class="track">
            <div class="fill" class:ok={g.state === 'complete'} style:width="{(done / total) * 100}%"></div>
          </div>
          <span class="xsmall muted">{done} of {total} tasks</span>
        </div>
      {/if}

      {#if g.notes.length && g.state !== 'complete'}
        <div class="card note">
          {#each g.notes.slice(-3) as n}<p class="small">{n}</p>{/each}
        </div>
      {/if}

      <section class="card contract">
        <div class="row">
          <button class="disclose" aria-expanded={contractOpen} onclick={() => (contractOpen = !contractOpen)}>
            <ChevronRight size={14} class="chev" />
            <h4>Goal Contract</h4>
            <span class="xsmall faint">rev {g.contract.revision} · authoritative for every agent</span>
          </button>
          <span class="spacer"></span>
          {#if !editing}<button class="btn ghost sm" onclick={edit}><Pencil size={12} /> Edit</button>{/if}
        </div>
        {#if contractOpen}
          {#if editing && draft}
            <div class="edit">
              <label class="label" for="c-obj">Objective</label>
              <textarea id="c-obj" class="textarea" rows="2" bind:value={draft.objective}></textarea>
              <label class="label" for="c-req">Requirements</label>
              <textarea id="c-req" class="textarea" rows="2" bind:value={draft.requirements}></textarea>
              <label class="label" for="c-con">Constraints</label>
              <textarea id="c-con" class="textarea" rows="2" bind:value={draft.constraints}></textarea>
              <label class="label" for="c-acc">Acceptance criteria</label>
              <textarea id="c-acc" class="textarea" rows="2" bind:value={draft.acceptance}></textarea>
              <label class="label" for="c-dod">Definition of Done (commands)</label>
              <textarea id="c-dod" class="textarea mono" rows="2" bind:value={draft.checks}></textarea>
              <div class="row">
                <span class="spacer"></span><button class="btn ghost" onclick={() => (editing = false)}>Cancel</button
                ><button class="btn primary" onclick={saveContract}>Save</button>
              </div>
            </div>
          {:else}
            <dl class="dl">
              {#each contractRows as [k, v] (k)}
                {#if v.length}<dt>{k}</dt>
                  <dd>
                    {#each v as x, i (i)}<div>{x}</div>{/each}
                  </dd>{/if}
              {/each}
              <dt>Definition of Done</dt>
              <dd>
                {#if g.contract.done_checks.length}
                  {#each g.contract.done_checks as d}<div class="mono small">{d.command ?? d.text}</div>{/each}
                {:else}<span class="muted small">No checks — completion will rely on task results only.</span>{/if}
              </dd>
            </dl>
          {/if}
        {/if}
      </section>

      <section>
        <h4 class="sec">Tasks</h4>
        {#if !g.graph.tasks.length}
          <p class="muted small">{active ? 'Planning tasks…' : 'Not started yet. Start the Goal to plan tasks.'}</p>
        {:else}
          <ul class="tasks list">
            {#each g.graph.tasks as t (t.id)}
              <li class="task {t.status}">
                <button
                  class="task-head"
                  aria-expanded={!!expanded[t.id]}
                  onclick={() => (expanded = { ...expanded, [t.id]: !expanded[t.id] })}
                >
                  <span class="st" aria-label={t.status}>
                    {#if t.status === 'done'}<Check size={14} />{:else if t.status === 'running'}<LoaderCircle
                        size={14}
                        class="spin"
                      />{:else if t.status === 'failed' || t.status === 'blocked'}<X size={14} />{:else}<Circle
                        size={12}
                      />{/if}
                  </span>
                  <span class="grow tt">{t.title}</span>
                  <span class="badge">{roleLabels[t.role] ?? t.role}</span>
                  {#if t.assigned_model}<span class="xsmall faint model">{modelLabel(t.assigned_model)}</span>{/if}
                  {#if t.started_at}<span class="xsmall faint time">{elapsed(t)}</span>{/if}
                </button>
                {#if expanded[t.id]}
                  <div class="task-body">
                    {#if t.detail}<p class="small muted pre">{t.detail.slice(0, 1200)}</p>{/if}
                    {#if t.files.length}<p class="xsmall">Files: <span class="mono">{t.files.join(', ')}</span></p>{/if}
                    {#if t.attempts}<p class="xsmall muted">
                        Attempts: {t.attempts}{t.failed_models.length
                          ? ` · reassigned from ${t.failed_models.map(modelLabel).join(', ')}`
                          : ''}
                      </p>{/if}
                    {#if t.verification}<p class="xsmall warn">{t.verification}</p>{/if}
                    {#if t.result}<div class="prose small result">{@html markdown(t.result.slice(0, 6000))}</div>{/if}
                  </div>
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
      </section>

      {#if g.checks.length}
        <section>
          <h4 class="sec">Verification</h4>
          <ul class="list checks">
            {#each g.checks.slice(-8) as c, i (i)}
              <li>
                <details>
                  <summary class="row"
                    ><span class:ok={c.passed} class:bad={!c.passed} class="ck">{c.passed ? '✓' : '✗'}</span><span
                      class="mono small grow">{c.check}</span
                    ></summary
                  >
                  <pre class="evidence">{c.evidence}</pre>
                </details>
              </li>
            {/each}
          </ul>
        </section>
      {/if}

      {#if activity.length}
        <section>
          <h4 class="sec">Activity</h4>
          <ol class="activity list" aria-live="polite">
            {#each activity.slice(-40) as a, i (i)}
              <li class="k-{a.kind}" class:fail={a.ok === false}>
                <span class="t faint"
                  >{new Date(a.at).toLocaleTimeString([], {
                    hour: '2-digit',
                    minute: '2-digit',
                    second: '2-digit',
                  })}</span
                >
                <span class="grow">{a.text}</span>
              </li>
            {/each}
          </ol>
        </section>
      {/if}

      {#if rec?.checkpoint}
        <p class="xsmall muted cp">
          A checkpoint was saved before this Goal started.
          <button class="btn ghost sm" onclick={restoreCheckpoint} disabled={active}
            ><RotateCcw size={12} /> Restore</button
          >
        </p>
      {/if}
    </div>
  </div>
{/if}

<style>
  .topbar {
    display: flex;
    align-items: center;
    gap: var(--s2);
    height: 52px;
    padding: 0 var(--s4);
    border-bottom: 1px solid var(--border);
    flex: none;
  }
  .title {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .state.complete {
    color: var(--success);
  }
  .state.blocked {
    background: var(--danger-soft);
    color: var(--danger);
  }
  .state.paused,
  .state.waiting_for_user,
  .state.stopped {
    color: var(--warning);
  }
  .scroll {
    flex: 1;
    overflow: auto;
  }
  .wrap {
    max-width: var(--content-w);
    margin: 0 auto;
    padding: var(--s5);
    display: flex;
    flex-direction: column;
    gap: var(--s5);
  }
  .progress {
    display: flex;
    align-items: center;
    gap: var(--s3);
  }
  .track {
    flex: 1;
    height: 6px;
    border-radius: 3px;
    background: var(--bg-sunken);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    background: var(--accent);
    transition: width 400ms var(--ease);
  }
  .fill.ok {
    background: var(--success);
  }
  .note {
    border-color: color-mix(in srgb, var(--warning) 40%, var(--border));
  }
  .disclose {
    display: flex;
    align-items: center;
    gap: var(--s2);
    border: none;
    background: none;
    color: var(--text);
    font: inherit;
    cursor: pointer;
    padding: 0;
  }
  .disclose :global(.chev) {
    transition: transform var(--duration) var(--ease);
  }
  .disclose[aria-expanded='true'] :global(.chev) {
    transform: rotate(90deg);
  }
  .dl {
    display: grid;
    grid-template-columns: 150px 1fr;
    gap: var(--s2) var(--s4);
    margin: var(--s4) 0 0;
    font-size: var(--fs-sm);
  }
  dt {
    color: var(--text-muted);
  }
  dd {
    margin: 0;
  }
  .edit {
    margin-top: var(--s3);
    display: grid;
    gap: var(--s1);
  }
  .edit .textarea {
    min-height: 44px;
    margin-bottom: var(--s2);
  }
  .sec {
    margin-bottom: var(--s2);
  }
  .tasks {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
    background: var(--surface);
  }
  .task + .task {
    border-top: 1px solid var(--border);
  }
  .task-head {
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--s2);
    min-height: 40px;
    padding: var(--s2) var(--s3);
    border: none;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-sm);
    text-align: left;
    cursor: pointer;
  }
  .task-head:hover {
    background: var(--surface-hover);
  }
  .st {
    display: inline-flex;
    width: 18px;
    justify-content: center;
    color: var(--text-faint);
  }
  .task.done .st {
    color: var(--success);
  }
  .task.running .st {
    color: var(--accent);
  }
  .task.failed .st,
  .task.blocked .st {
    color: var(--danger);
  }
  .task.done .tt {
    color: var(--text-muted);
  }
  .model,
  .time {
    flex: none;
  }
  .task-body {
    padding: 0 var(--s4) var(--s3) 38px;
    display: grid;
    gap: var(--s2);
  }
  .pre {
    white-space: pre-wrap;
  }
  .result {
    border-left: 2px solid var(--border);
    padding-left: var(--s3);
  }
  .warn {
    color: var(--warning);
  }
  :global(.spin) {
    animation: spin 1s linear infinite;
  }
  .checks li + li {
    margin-top: var(--s1);
  }
  .checks summary {
    cursor: pointer;
    list-style: none;
  }
  .ck.ok {
    color: var(--success);
  }
  .ck.bad {
    color: var(--danger);
  }
  .evidence {
    font-size: var(--fs-xs);
    background: var(--code-bg);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: var(--s2);
    max-height: 240px;
    overflow: auto;
    white-space: pre-wrap;
  }
  .activity {
    font-size: var(--fs-xs);
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .activity li {
    display: flex;
    gap: var(--s3);
  }
  .activity .t {
    flex: none;
    font-variant-numeric: tabular-nums;
  }
  .activity .k-notice,
  .activity .k-handoff {
    color: var(--warning);
  }
  .activity .k-done {
    color: var(--success);
  }
  .activity .fail,
  .activity .k-blocked {
    color: var(--danger);
  }
  .activity .k-tool {
    color: var(--text-muted);
  }
  .cp {
    display: flex;
    align-items: center;
    gap: var(--s2);
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
