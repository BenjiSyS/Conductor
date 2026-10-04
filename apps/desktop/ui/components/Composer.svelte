<script lang="ts">
  import { ArrowUp, Square, Target, ChevronDown, Sparkles } from '@lucide/svelte';
  import {
    app,
    attempt,
    openGoal,
    refreshGoals,
    savePrefs,
    send,
    stopCurrent,
    succeeded,
    toast,
  } from '../lib/app.svelte';
  import { call, readable } from '../lib/api';
  import type { Answer, GoalRecord, Mode, QuestionRound } from '../lib/types';
  import ModelPicker from './ModelPicker.svelte';
  import EffortControl from './EffortControl.svelte';
  import ContextPill from './ContextPill.svelte';
  import QuestionCard from './QuestionCard.svelte';

  let textarea: HTMLTextAreaElement | undefined = $state();
  let goalStage = $state<'idle' | 'asking' | 'questions' | 'starting'>('idle');
  let round = $state<QuestionRound | null>(null);
  let answers = $state<Record<string, Answer>>({});
  let showGoalOptions = $state(false);
  let checks = $state('');
  let constraints = $state('');
  let checksPrefilled = $state<string | null>(null);

  const running = $derived(!!(app.conversationId && app.running[app.conversationId]));
  const modes: { id: Mode; label: string; hint: string }[] = [
    { id: 'chat', label: 'Chat', hint: 'Ask and discuss' },
    { id: 'plan', label: 'Plan', hint: 'Design before editing — never changes files' },
    { id: 'goal', label: 'Goal', hint: 'Outcome-driven: plan, delegate, verify, finish' },
    { id: 'agent', label: 'Agent', hint: 'Edit files and run commands under your permission policy' },
  ];

  // Prefill Definition of Done from detected test commands.
  $effect(() => {
    const pid = app.projectId;
    if (app.mode === 'goal' && pid && checksPrefilled !== pid) {
      checksPrefilled = pid;
      call<{ detection: { suggested_test_commands: string[] }; config: { verify?: { commands?: string[] } } | null }>(
        'project_info',
        { projectId: pid },
      )
        .then((i) => {
          // Suggest detected commands, but never overwrite what the user typed.
          if (!checks.trim())
            checks = (
              i.config?.verify?.commands?.length ? i.config.verify.commands : i.detection.suggested_test_commands
            ).join('\n');
        })
        .catch(() => {});
    }
  });

  function autosize() {
    if (!textarea) return;
    textarea.style.height = 'auto';
    textarea.style.height = Math.min(textarea.scrollHeight, 260) + 'px';
  }
  $effect(() => {
    void app.draft;
    queueMicrotask(autosize);
  });

  function key(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      void submit();
    }
  }

  async function submit() {
    if (!app.draft.trim()) return;
    if (!app.target) {
      toast('Connect a provider and choose a model first.', 'info', {
        label: 'Connect',
        run: () => (app.settingsTab = 'providers'),
      });
      return;
    }
    if (app.mode === 'goal') return startGoalFlow();
    await send();
  }

  async function startGoalFlow() {
    goalStage = 'asking';
    try {
      const r = await call<QuestionRound>('goal_clarify', {
        projectId: app.projectId,
        objective: app.draft.trim(),
        target: app.target,
      });
      if (r.questions.length) {
        round = r;
        answers = {};
        goalStage = 'questions';
        return;
      }
    } catch (e) {
      // Clarification is optional; if the planner can't be reached the
      // Goal itself will report the provider problem clearly.
      toast(`Skipped questions: ${readable(e)}`);
    }
    await createAndStart();
  }

  async function createAndStart() {
    goalStage = 'starting';
    const comboId = app.target!.startsWith('combo:') ? app.target!.slice(6) : app.target!;
    const rec = await attempt(() =>
      call<GoalRecord>('goal_create', {
        goal: {
          project_id: app.projectId,
          objective: app.draft.trim(),
          constraints: constraints
            .split('\n')
            .map((s) => s.trim())
            .filter(Boolean),
          checks: checks
            .split('\n')
            .map((s) => s.trim())
            .filter(Boolean),
          combo_id: comboId,
          round,
          answers: Object.entries(answers),
        },
      }),
    );
    if (!rec) {
      goalStage = 'idle';
      return;
    }
    const ok = await succeeded(() => call('goal_start', { id: rec.goal.id }));
    await refreshGoals();
    goalStage = 'idle';
    round = null;
    if (ok) {
      app.draft = '';
      openGoal(rec.goal.id);
    }
  }

  async function effortChoice(choice: 'once' | 'always' | 'never') {
    const r = app.effortRequest;
    if (!r || !app.prefs) return;
    if (choice === 'once') app.effort = r.level;
    else
      await savePrefs({
        max_effort: { ...app.prefs.max_effort, [r.model]: choice === 'always' ? 'always_allow' : 'never' },
      });
    app.effortRequest = null;
  }
</script>

<div class="composer-wrap">
  {#if app.effortRequest}
    <div class="effort-ask card" role="alert">
      <Sparkles size={16} />
      <div class="grow small">
        <strong>Higher effort may help.</strong>
        <span class="muted">{app.effortRequest.why}</span>
      </div>
      <button class="btn sm" onclick={() => effortChoice('once')}>Allow</button>
      <button class="btn sm" onclick={() => effortChoice('always')}>Always for this model</button>
      <button class="btn sm ghost" onclick={() => effortChoice('never')}>Don't ask again</button>
      <button class="btn sm ghost" onclick={() => (app.effortRequest = null)}>Dismiss</button>
    </div>
  {/if}

  {#if goalStage === 'questions' && round}
    <QuestionCard
      {round}
      bind:answers
      onstart={createAndStart}
      oncancel={() => {
        goalStage = 'idle';
        round = null;
      }}
    />
  {/if}

  <div class="composer" class:goal={app.mode === 'goal'}>
    <textarea
      bind:this={textarea}
      bind:value={app.draft}
      onkeydown={key}
      oninput={autosize}
      rows="1"
      placeholder={app.mode === 'goal'
        ? 'Describe the outcome you want…'
        : app.mode === 'plan'
          ? 'What should we plan?'
          : app.mode === 'agent'
            ? 'What should the agent do?'
            : `Ask about ${app.project?.name ?? 'your project'}…`}
      aria-label="Prompt"
      disabled={goalStage === 'asking' || goalStage === 'starting'}
    ></textarea>

    {#if app.mode === 'goal' && showGoalOptions}
      <div class="goal-options">
        <label class="label xsmall" for="dod">Definition of Done — commands that must pass (one per line)</label>
        <textarea id="dod" class="textarea mono" rows="2" bind:value={checks} placeholder="cargo test"></textarea>
        <label class="label xsmall" for="cons">Constraints (one per line)</label>
        <textarea id="cons" class="textarea" rows="2" bind:value={constraints} placeholder="Don't add new dependencies"
        ></textarea>
      </div>
    {/if}

    <div class="bar">
      <div class="segmented" role="group" aria-label="Mode">
        {#each modes as m}
          <button aria-pressed={app.mode === m.id} title={m.hint} onclick={() => (app.mode = m.id)}>{m.label}</button>
        {/each}
      </div>
      <ModelPicker />
      <EffortControl />
      {#if app.mode === 'goal'}
        <button
          class="btn ghost sm"
          aria-expanded={showGoalOptions}
          onclick={() => (showGoalOptions = !showGoalOptions)}>Checks <ChevronDown size={12} /></button
        >
      {/if}
      <span class="spacer"></span>
      <ContextPill />
      {#if running}
        <button class="btn icon stop" aria-label="Stop" title="Stop (Esc)" onclick={stopCurrent}
          ><Square size={14} fill="currentColor" /></button
        >
      {:else if app.mode === 'goal'}
        <button class="btn primary" onclick={submit} disabled={!app.draft.trim() || goalStage !== 'idle'}>
          {#if goalStage === 'asking'}<span class="spinner"></span> Thinking…{:else if goalStage === 'starting'}<span
              class="spinner"
            ></span> Starting…{:else}<Target size={14} /> Start Goal{/if}
        </button>
      {:else}
        <button
          class="btn primary icon"
          aria-label="Send"
          title="Send (Enter)"
          onclick={submit}
          disabled={!app.draft.trim()}><ArrowUp size={16} /></button
        >
      {/if}
    </div>
  </div>
</div>

<style>
  .composer-wrap {
    max-width: var(--content-w);
    margin: 0 auto;
    display: flex;
    flex-direction: column;
    gap: var(--s2);
  }
  .composer {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
    padding: var(--s3) var(--s3) var(--s2);
    transition: border-color var(--duration) var(--ease);
  }
  .composer:focus-within {
    border-color: var(--border-strong);
  }
  .composer.goal {
    border-color: color-mix(in srgb, var(--accent) 35%, var(--border));
  }
  textarea:not(.textarea) {
    width: 100%;
    border: none;
    outline: none;
    resize: none;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-md);
    line-height: 1.5;
    padding: var(--s1) var(--s1) var(--s2);
    min-height: 28px;
    max-height: 260px;
  }
  textarea::placeholder {
    color: var(--text-faint);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: var(--s2);
    flex-wrap: wrap;
  }
  .stop {
    background: var(--text);
    color: var(--bg);
    border-color: var(--text);
  }
  .goal-options {
    padding: var(--s2) var(--s1) var(--s3);
    display: grid;
    gap: var(--s1);
  }
  .goal-options .textarea {
    min-height: 48px;
  }
  .effort-ask {
    display: flex;
    align-items: center;
    gap: var(--s2);
    flex-wrap: wrap;
    padding: var(--s2) var(--s3);
    color: var(--accent);
  }
  .effort-ask .grow {
    color: var(--text);
  }
</style>
