<script lang="ts">
  import { app } from '../lib/app.svelte';
  import { ago } from '../lib/format';
  import Drawer from './Drawer.svelte';

  let filter = $state('all');
  const kinds = ['all', 'goal', 'chat', 'checkpoint', 'install', 'provider', 'project'];
  const items = $derived(
    app.history
      .filter(
        (h) =>
          (!app.projectId || !h.project_id || h.project_id === app.projectId) &&
          (filter === 'all' || h.kind === filter),
      )
      .slice(0, 300),
  );
</script>

<Drawer title="History" onclose={() => (app.historyOpen = false)} width={400}>
  <div class="row wrap filters">
    {#each kinds as k (k)}
      <button class="btn sm" class:primary={filter === k} onclick={() => (filter = k)}>{k}</button>
    {/each}
  </div>
  <ol class="list">
    {#each items as h (h.id)}
      <li>
        <span class="badge">{h.kind}</span>
        <span class="grow small">{h.summary}</span>
        <span class="xsmall faint">{ago(h.created_at)}</span>
      </li>
    {:else}
      <li class="muted small">Meaningful project activity — Goals, decisions, checkpoints, installs — appears here.</li>
    {/each}
  </ol>
</Drawer>

<style>
  .filters {
    margin-bottom: var(--s3);
    gap: var(--s1);
  }
  li {
    display: flex;
    align-items: center;
    gap: var(--s2);
    padding: var(--s2) 0;
    border-bottom: 1px solid var(--border);
  }
</style>
