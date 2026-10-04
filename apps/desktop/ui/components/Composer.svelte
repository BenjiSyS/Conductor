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
    draftMode,
    slashCommand,
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
  // Chat and Agent are modes; Goal and Plan are started per message.
  const modes: { id: Mode; label: string; hint: string }[] = [
    { id: 'chat', label: 'Chat', hint: 'Ask and discuss' },
    { id: 'agent', label: 'Agent', hint: 'Edit files and run commands under your permission policy' },
  ];
  const commands = [
    { cmd: 'goal', hint: 'Outcome-driven: plan, delegate, verify, finish' },
    { cmd: 'plan', hint: 'Design before editing — never changes files' },
  ] as const;
  const mode = $derived(draftMode());
  // Typing "/" shows the commands until a space is typed.
  const menu = $derived(
    /^\/[a-z]*$/i.test(app.draft) ? commands.filter((c) => c.cmd.startsWith(app.draft.slice(1).toLowerCase())) : [],
  );
  function complete(cmd: string) {
    app.draft = `/${cmd} `;
    queueMicrotask(() => textarea?.focus());
  }

  // Prefill Definition of Done from detected test commands.
  $effect(() => {
    const pid = app.projectId;
    if (mode === 'goal' && pid && checksPrefilled !== pid) {
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

  // Large pastes are saved by the app and shown as a short chip, so the box
  // never holds megabytes of text (5 MB+ pastes stay responsive).
  const PASTE_CHIP_CHARS = 8_000;
  const MAX_DRAFT_CHARS = 60_000;
  let pasteCount = 0;
  let pasting = $state(false);
  async function chipFor(text: string): Promise<string | null> {
    pasting = true;
    try {
      const info = await call<{ id: string; chars: number; lines: number }>('save_paste', { text });
      pasteCount += 1;
      return `[Pasted text #${pasteCount} · ${info.chars.toLocaleString()} chars · ${info.lines.toLocaleString()} lines · paste:${info.id}]`;
    } catch (e) {
      toast(readable(e), 'error');
      return null;
    } finally {
      pasting = false;
    }
  }
  async function paste(e: ClipboardEvent) {
    const text = e.clipboardData?.getData('text/plain') ?? '';
    if (text.length < PASTE_CHIP_CHARS) return;
    e.preventDefault();
    const el = textarea;
    const start = el?.selectionStart ?? app.draft.length;
    const end = el?.selectionEnd ?? app.draft.length;
    const chip = await chipFor(text);
    if (!chip) return;
    app.draft = app.draft.slice(0, start) + chip + app.draft.slice(end);
    queueMicrotask(() => {
      autosize();
      const at = start + chip.length;
      textarea?.setSelectionRange(at, at);
    });
  }

  function key(e: KeyboardEvent) {
    if (menu.length && (e.key === 'Tab' || (e.key === 'Enter' && !e.shiftKey))) {
      e.preventDefault();
      complete(menu[0].cmd);
      return;
    }
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      void submit();
    }
  }

  async function submit() {
    if (!app.draft.trim() || pasting) return;
    // A very long typed draft is sent as a saved paste too.
    if (app.draft.length > MAX_DRAFT_CHARS) {
      const chip = await chipFor(app.draft);
      if (!chip) return;
      app.draft = chip;
    }
    if (!app.target) {
      toast('Connect a provider and choose a model first.', 'info', {
        label: 'Connect',
        run: () => (app.settingsTab = 'providers'),
      });
      return;
    }
    const cmd = slashCommand(app.draft);
    const body = cmd ? app.draft.replace(/^\/(goal|plan)\s*/i, '') : app.draft;
    if (cmd && !body.trim()) {
      toast(cmd === 'goal' ? 'Describe the outcome after /goal' : 'Describe what to plan after /plan');
      return;
    }
    if (cmd === 'goal') {
      app.forcedMode = 'goal';
      app.draft = body;
      return startGoalFlow();
    }
    const base = app.mode === 'agent' ? 'agent' : 'chat';
    app.mode = cmd === 'plan' ? 'plan' : base;
    if (cmd) app.draft = body;
    try {
      await send();
    } finally {
      app.mode = base;
    }
  }

  /** Leave Goal setup; keep the text as a /goal prompt so nothing is lost. */
  function endGoalSetup(keepDraft: boolean) {
    app.forcedMode = null;
    if (keepDraft && app.draft.trim()) app.draft = `/goal ${app.draft}`;
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
      endGoalSetup(true);
      return;
    }
    const ok = await succeeded(() => call('goal_start', { id: rec.goal.id }));
    await refreshGoals();
    goalStage = 'idle';
    round = null;
    if (ok) {
      app.draft = '';
      endGoalSetup(false);
      openGoal(rec.goal.id);
    } else {
      endGoalSetup(true);
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
        endGoalSetup(true);
      }}
    />
  {/if}

  <div class="composer" class:goal={mode === 'goal'}>
    {#if menu.length}
      <div class="slash" role="listbox" aria-label="Commands">
        {#each menu as c, i (c.cmd)}
          <button role="option" aria-selected={i === 0} onclick={() => complete(c.cmd)}
            ><span class="mono">/{c.cmd}</span><span class="xsmall muted">{c.hint}</span></button
          >
        {/each}
      </div>
    {/if}
    {#if mode === 'goal' || mode === 'plan'}
      <span class="badge mode-badge">{mode === 'goal' ? 'Goal' : 'Plan'}</span>
    {/if}
    <textarea
      bind:this={textarea}
      bind:value={app.draft}
      onkeydown={key}
      oninput={autosize}
      onpaste={paste}
      rows="1"
      placeholder={mode === 'goal'
        ? 'Describe the outcome you want…'
        : mode === 'plan'
          ? 'What should we plan?'
          : mode === 'agent'
            ? 'What should the agent do? (/goal or /plan for more)'
            : `Ask about ${app.project?.name ?? 'your project'}… (/goal, /plan)`}
      aria-label="Prompt"
      disabled={goalStage === 'asking' || goalStage === 'starting'}
    ></textarea>

    {#if mode === 'goal' && showGoalOptions}
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
      {#if mode === 'goal'}
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
      {:else if mode === 'goal'}
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
  .composer {
    position: relative;
  }
  .slash {
    position: absolute;
    left: var(--s2);
    bottom: calc(100% + 6px);
    z-index: 20;
    display: grid;
    min-width: 320px;
    padding: var(--s1);
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
  }
  .slash button {
    display: flex;
    gap: var(--s3);
    align-items: baseline;
    border: none;
    background: none;
    color: var(--text);
    font: inherit;
    text-align: left;
    padding: 6px var(--s2);
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  .slash button[aria-selected='true'],
  .slash button:hover {
    background: var(--surface-hover);
  }
  .mode-badge {
    position: absolute;
    top: 8px;
    right: 10px;
    font-size: var(--fs-xs);
    color: var(--accent);
    background: var(--accent-soft);
    border-radius: 999px;
    padding: 1px 8px;
  }
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
