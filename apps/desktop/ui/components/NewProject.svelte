<script lang="ts">
  import { FolderOpen } from '@lucide/svelte';
  import { app, attempt, refresh, selectProject } from '../lib/app.svelte';
  import { call, chooseFolder } from '../lib/api';
  import type { Project, Recipe } from '../lib/types';
  import Modal from './Modal.svelte';

  let parent = $state('');
  let name = $state('');
  let git = $state(true);
  let recipe = $state('empty');
  let recipes = $state<Recipe[]>([]);
  let busy = $state(false);

  $effect(() => {
    call<Recipe[]>('recipes').then((r) => (recipes = r));
  });

  async function pick() {
    const p = await chooseFolder('Where should the project go?');
    if (p) parent = p;
  }
  async function create() {
    busy = true;
    const p = await attempt(() =>
      call<Project>('new_project', { parent, name: name.trim(), initializeGit: git, recipe }),
    );
    busy = false;
    if (!p) return;
    await refresh();
    selectProject(p.id);
    app.newProjectOpen = false;
  }
</script>

<Modal title="New project" onclose={() => (app.newProjectOpen = false)} width={520}>
  <div class="field">
    <label class="label" for="np-name">Name</label>
    <input id="np-name" class="input" bind:value={name} placeholder="my-app" />
  </div>
  <div class="field">
    <label class="label" for="np-parent">Location</label>
    <div class="row">
      <input id="np-parent" class="input" bind:value={parent} placeholder="Choose a parent folder" />
      <button class="btn" onclick={pick}><FolderOpen size={14} /> Browse</button>
    </div>
  </div>
  <div class="field">
    <span class="label">Start from</span>
    <div class="recipes">
      {#each recipes as r (r.id)}
        <button class="recipe" aria-pressed={recipe === r.id} onclick={() => (recipe = r.id)}>
          <strong class="small">{r.name}</strong>
          <span class="xsmall muted">{r.description}</span>
        </button>
      {/each}
    </div>
  </div>
  <label class="row small"
    ><input type="checkbox" bind:checked={git} /> Initialise a Git repository (enables checkpoints)</label
  >
  {#snippet footer()}
    <button class="btn ghost" onclick={() => (app.newProjectOpen = false)}>Cancel</button>
    <button class="btn primary" onclick={create} disabled={!name.trim() || !parent.trim() || busy}
      >{busy ? 'Creating…' : 'Create project'}</button
    >
  {/snippet}
</Modal>

<style>
  .recipes {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: var(--s2);
  }
  .recipe {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    padding: var(--s2) var(--s3);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--bg);
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .recipe[aria-pressed='true'] {
    border-color: var(--accent);
    background: var(--accent-soft);
  }
</style>
