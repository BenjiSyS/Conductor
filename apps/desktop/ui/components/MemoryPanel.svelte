<script lang="ts">
  import { Plus, Trash2 } from '@lucide/svelte';
  import { app, attempt } from '../lib/app.svelte';
  import { call } from '../lib/api';
  import type { ProjectMemory } from '../lib/types';
  import Drawer from './Drawer.svelte';

  let mem = $state<ProjectMemory | null>(null);
  let topic = $state('');
  let value = $state('');
  let fact = $state('');

  $effect(() => {
    const pid = app.projectId;
    if (pid) call<ProjectMemory>('memory_get', { projectId: pid }).then((m) => (mem = m));
  });

  async function save(next: ProjectMemory) {
    mem = next;
    await attempt(() => call('memory_save', { projectId: app.projectId, memory: next }));
  }
  function addDecision() {
    if (!mem || !topic.trim() || !value.trim()) return;
    const decisions = mem.decisions.filter((d) => d.topic.toLowerCase() !== topic.trim().toLowerCase());
    decisions.push({
      id: crypto.randomUUID(),
      topic: topic.trim(),
      value: value.trim(),
      at: Math.floor(Date.now() / 1000),
      source: 'user',
    });
    void save({ ...mem, decisions });
    topic = value = '';
  }
  function addFact() {
    if (!mem || !fact.trim()) return;
    void save({ ...mem, facts: [...mem.facts, fact.trim()] });
    fact = '';
  }
</script>

<Drawer title="Project memory" onclose={() => (app.memoryOpen = false)} width={420}>
  {#if mem}
    <p class="xsmall muted">
      Durable facts and decisions for {app.project?.name}. Conductor includes them when relevant and won't ask about
      them again. Stored locally.
    </p>
    <h4 class="sec">Decisions</h4>
    <ul class="list">
      {#each mem.decisions as d (d.id)}
        <li class="row">
          <span class="grow small"
            ><strong>{d.topic}</strong>: {d.value} <span class="xsmall faint">· {d.source}</span></span
          >
          <button
            class="btn ghost icon sm"
            aria-label="Remove decision {d.topic}"
            onclick={() => mem && save({ ...mem, decisions: mem.decisions.filter((x) => x.id !== d.id) })}
            ><Trash2 size={13} /></button
          >
        </li>
      {:else}
        <li class="muted small">No decisions yet. Answers to Goal questions are saved here.</li>
      {/each}
    </ul>
    <div class="row add">
      <input class="input" placeholder="Topic (e.g. engine)" bind:value={topic} aria-label="Decision topic" />
      <input
        class="input"
        placeholder="Decision (e.g. Godot)"
        bind:value
        aria-label="Decision value"
        onkeydown={(e) => e.key === 'Enter' && addDecision()}
      />
      <button class="btn icon" aria-label="Add decision" onclick={addDecision}><Plus size={14} /></button>
    </div>

    <h4 class="sec">Facts</h4>
    <ul class="list">
      {#each mem.facts as f, i (i)}
        <li class="row">
          <span class="grow small">{f}</span>
          <button
            class="btn ghost icon sm"
            aria-label="Remove fact"
            onclick={() => mem && save({ ...mem, facts: mem.facts.filter((_, j) => j !== i) })}
            ><Trash2 size={13} /></button
          >
        </li>
      {/each}
    </ul>
    <div class="row add">
      <input
        class="input"
        placeholder="e.g. Must support Windows 10"
        bind:value={fact}
        aria-label="New fact"
        onkeydown={(e) => e.key === 'Enter' && addFact()}
      />
      <button class="btn icon" aria-label="Add fact" onclick={addFact}><Plus size={14} /></button>
    </div>

    <h4 class="sec">Project instructions</h4>
    <textarea
      class="textarea"
      rows="5"
      value={mem.instructions}
      onchange={(e) => mem && save({ ...mem, instructions: (e.target as HTMLTextAreaElement).value })}
      placeholder="Conventions every model should follow in this project."
    ></textarea>
    <p class="hint">
      A <span class="mono">conductor.toml</span> in the project can also hold shared instructions for your team.
    </p>
  {/if}
</Drawer>

<style>
  .sec {
    margin: var(--s4) 0 var(--s2);
  }
  li {
    padding: 4px 0;
    border-bottom: 1px solid var(--border);
  }
  .add {
    margin-top: var(--s2);
  }
</style>
