<script lang="ts">
  import { Wand2 } from '@lucide/svelte';
  import type { Answer, QuestionRound } from '../lib/types';

  let {
    round,
    answers = $bindable(),
    onstart,
    oncancel,
  }: { round: QuestionRound; answers: Record<string, Answer>; onstart: () => void; oncancel: () => void } = $props();

  function setOption(id: string, v: string) {
    answers = { ...answers, [id]: { kind: 'option', value: v } };
  }
  function setText(id: string, v: string) {
    answers = { ...answers, [id]: v.trim() ? { kind: 'text', value: v } : { kind: 'decide_for_me' } };
  }
  function decide(id: string) {
    answers = { ...answers, [id]: { kind: 'decide_for_me' } };
  }
  function decideAll() {
    const next: Record<string, Answer> = { ...answers };
    for (const q of round.questions) if (!next[q.id]) next[q.id] = { kind: 'decide_for_me' };
    answers = next;
    onstart();
  }
  const selected = (id: string) => {
    const a = answers[id];
    return a && a.kind !== 'decide_for_me' ? a.value : null;
  };
</script>

<section class="qcard card" aria-label="A few questions before starting">
  <header class="row between">
    <div>
      <h4>A few questions first</h4>
      <p class="muted xsmall">Only what changes the result. Skip any — Conductor picks a sensible default.</p>
    </div>
    <button class="btn sm" onclick={decideAll}><Wand2 size={13} /> Decide for me</button>
  </header>
  <ol class="qs">
    {#each round.questions as q (q.id)}
      <li>
        <div class="q">
          <span>{q.text}</span>
          {#if q.priority === 'blocking'}<span class="badge accent">important</span>{/if}
        </div>
        <div class="opts">
          {#each q.options as o (o)}
            <button class="opt" aria-pressed={selected(q.id) === o} onclick={() => setOption(q.id, o)}>{o}</button>
          {/each}
          <button
            class="opt ghosty"
            aria-pressed={answers[q.id]?.kind === 'decide_for_me'}
            onclick={() => decide(q.id)}
            title={q.default ? `Default: ${q.default}` : 'Let Conductor choose'}>Decide for me</button
          >
        </div>
        {#if !q.options.length}
          <input
            class="input"
            placeholder="Your answer (optional)"
            oninput={(e) => setText(q.id, (e.target as HTMLInputElement).value)}
            aria-label={q.text}
          />
        {/if}
      </li>
    {/each}
  </ol>
  <footer class="row">
    <span class="spacer"></span>
    <button class="btn ghost" onclick={oncancel}>Cancel</button>
    <button class="btn primary" onclick={onstart}>Start Goal</button>
  </footer>
</section>

<style>
  .qcard {
    box-shadow: var(--shadow);
    animation: rise var(--duration) var(--ease);
  }
  .qs {
    margin: var(--s4) 0;
    padding-left: 1.2em;
    display: flex;
    flex-direction: column;
    gap: var(--s4);
  }
  .q {
    display: flex;
    align-items: center;
    gap: var(--s2);
    font-weight: 500;
    margin-bottom: var(--s2);
  }
  .opts {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s2);
    margin-bottom: var(--s2);
  }
  .opt {
    height: 28px;
    padding: 0 var(--s3);
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .opt:hover {
    border-color: var(--border-strong);
  }
  .opt[aria-pressed='true'] {
    background: var(--accent-soft);
    border-color: var(--accent);
    color: var(--accent);
  }
  .opt.ghosty {
    border-style: dashed;
    color: var(--text-muted);
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }
</style>
