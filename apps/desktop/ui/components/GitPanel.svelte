<script lang="ts">
  import { RefreshCw, Save, RotateCcw } from '@lucide/svelte';
  import { app, attempt } from '../lib/app.svelte';
  import { call } from '../lib/api';
  import type { Checkpoint } from '../lib/types';
  import Drawer from './Drawer.svelte';

  interface Overview {
    repo: boolean;
    summary?: string;
    status?: { branch: string | null; staged: string[]; modified: string[]; untracked: string[]; conflicted: string[] };
    log?: { hash: string; author: string; date: string; subject: string }[];
    branches?: string[];
    diff?: string;
  }
  let ov = $state<Overview | null>(null);
  let cps = $state<Checkpoint[]>([]);
  const fileGroups = $derived<[string, string[]][]>([
    ['Conflicted', ov?.status?.conflicted ?? []],
    ['Staged', ov?.status?.staged ?? []],
    ['Modified', ov?.status?.modified ?? []],
    ['Untracked', ov?.status?.untracked ?? []],
  ]);
  let tab = $state<'changes' | 'history' | 'checkpoints' | 'github'>('changes');
  interface GhItem {
    number?: number;
    title?: string;
    name?: string;
    state?: string;
    status?: string;
    conclusion?: string;
    url: string;
    headRefName?: string;
    headBranch?: string;
    isDraft?: boolean;
  }
  let gh = $state<{ status: string; message?: string; prs?: GhItem[]; issues?: GhItem[]; runs?: GhItem[] } | null>(
    null,
  );
  async function loadGh() {
    gh = null;
    gh = await call<typeof gh>('github_overview', { projectId: app.projectId }).catch((e) => ({
      status: 'error',
      message: String(e),
    }));
  }
  $effect(() => {
    if (tab === 'github' && !gh) void loadGh();
  });

  async function load() {
    if (!app.projectId) return;
    ov = await call<Overview>('git_overview', { projectId: app.projectId }).catch(() => null);
    cps = await call<Checkpoint[]>('checkpoints_list', { projectId: app.projectId }).catch(() => []);
  }
  $effect(() => {
    void app.projectId;
    void load();
  });

  async function checkpoint() {
    const label = prompt('Checkpoint name', 'Before my change');
    if (!label) return;
    await attempt(() => call('checkpoint_create', { projectId: app.projectId, label }), 'Checkpoint saved');
    await load();
  }
  async function restore(c: Checkpoint) {
    if (!confirm(`Restore "${c.label}"? Your current files are saved as a new checkpoint first.`)) return;
    await attempt(
      () => call('checkpoint_restore', { projectId: app.projectId, id: c.id }),
      'Restored. Previous state saved as a checkpoint.',
    );
    await load();
  }
</script>

<Drawer title="Git" onclose={() => (app.gitOpen = false)} width={480}>
  {#snippet actions()}
    <button class="btn ghost icon sm" aria-label="Refresh" onclick={load}><RefreshCw size={14} /></button>
  {/snippet}
  {#if ov && !ov.repo}
    <p class="muted small">
      This project isn't a Git repository. Initialise Git to get diffs, branches and checkpoints.
    </p>
  {:else if ov}
    <p class="small">{ov.summary}</p>
    <div class="segmented tabs" role="tablist">
      <button aria-pressed={tab === 'changes'} onclick={() => (tab = 'changes')}>Changes</button>
      <button aria-pressed={tab === 'history'} onclick={() => (tab = 'history')}>History</button>
      <button aria-pressed={tab === 'checkpoints'} onclick={() => (tab = 'checkpoints')}>Checkpoints</button>
      <button aria-pressed={tab === 'github'} onclick={() => (tab = 'github')}>GitHub</button>
    </div>
    {#if tab === 'changes'}
      {#each fileGroups as [k, files] (k)}
        {#if files.length}
          <h4 class="sec xsmall">{k}</h4>
          <ul class="list files">
            {#each files as f (f)}<li class="mono xsmall">{f}</li>{/each}
          </ul>
        {/if}
      {/each}
      {#if ov.diff}<pre class="diff">{#each ov.diff.split('\n').slice(0, 1500) as line, i (i)}<span
              class:add={line.startsWith('+') && !line.startsWith('+++')}
              class:del={line.startsWith('-') && !line.startsWith('---')}
              class:hunk={line.startsWith('@@')}
              >{line}
</span>{/each}</pre>{:else}<p class="muted small">No uncommitted changes.</p>{/if}
      <p class="xsmall faint">Conductor never resets or discards your work.</p>
    {:else if tab === 'history'}
      <ul class="list log">
        {#each ov.log ?? [] as c (c.hash)}
          <li>
            <span class="mono xsmall faint">{c.hash.slice(0, 7)}</span> <span class="small">{c.subject}</span>
            <span class="xsmall faint">· {c.author}</span>
          </li>
        {/each}
      </ul>
    {:else if tab === 'github'}
      {#if !gh}
        <div class="row muted small"><span class="spinner"></span> Asking GitHub…</div>
      {:else if gh.status !== 'ok'}
        <p class="small muted">{gh.message}</p>
        <button class="btn sm" onclick={loadGh}>Try again</button>
      {:else}
        {#each [['Pull requests', gh.prs ?? []], ['Issues', gh.issues ?? []], ['CI runs', gh.runs ?? []]] as const as [label, items] (label)}
          <h4 class="sec xsmall">{label}</h4>
          <ul class="list log">
            {#each items as it (it.url)}
              <li>
                <a href={it.url} target="_blank" rel="noreferrer" class="small"
                  >{it.number ? `#${it.number} ` : ''}{it.title ?? it.name}</a
                >
                <span class="xsmall faint"
                  >· {it.conclusion || it.status || it.state}{it.isDraft ? ' · draft' : ''}{it.headRefName ||
                  it.headBranch
                    ? ` · ${it.headRefName ?? it.headBranch}`
                    : ''}</span
                >
              </li>
            {:else}
              <li class="muted small">None</li>
            {/each}
          </ul>
        {/each}
        <p class="xsmall faint">
          Uses the GitHub CLI's own sign-in. Ask an agent to “fix the failing CI run” or “open a PR for this branch” —
          publishing still asks first.
        </p>
      {/if}
    {:else}
      <div class="row">
        <p class="small muted grow">Restorable snapshots. Creating one never touches your branches or staging.</p>
        <button class="btn sm" onclick={checkpoint}><Save size={13} /> Save</button>
      </div>
      <ul class="list cps">
        {#each cps as c (c.id)}
          <li class="row">
            <span class="grow small">{c.label}<br /><span class="xsmall faint">{c.created}</span></span><button
              class="btn sm ghost"
              onclick={() => restore(c)}
              disabled={app.anyRunning}><RotateCcw size={12} /> Restore</button
            >
          </li>
        {:else}
          <li class="muted small">No checkpoints yet. Goals save one automatically before they start.</li>
        {/each}
      </ul>
    {/if}
  {:else}
    <div class="row muted small"><span class="spinner"></span> Reading repository…</div>
  {/if}
</Drawer>

<style>
  .tabs {
    margin: var(--s3) 0;
  }
  .sec {
    color: var(--text-muted);
    margin: var(--s3) 0 var(--s1);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .files li {
    padding: 1px 0;
  }
  .diff {
    font-size: 11px;
    line-height: 1.45;
    background: var(--code-bg);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: var(--s2);
    overflow: auto;
    max-height: 50vh;
    margin-top: var(--s3);
  }
  .add {
    color: var(--success);
  }
  .del {
    color: var(--danger);
  }
  .hunk {
    color: var(--accent);
  }
  .log li,
  .cps li {
    padding: var(--s2) 0;
    border-bottom: 1px solid var(--border);
  }
</style>
