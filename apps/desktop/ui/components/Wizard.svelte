<script lang="ts">
  import SetupStatus from './SetupStatus.svelte';
  import { call as callSetup } from '../lib/api';
  // Start checking the computer the moment the wizard opens.
  void callSetup('setup_prepare').catch(() => {});
  import { Check, Turtle, Scale, Rocket, Shield, ShieldCheck, ShieldAlert, Layers, Cpu } from '@lucide/svelte';
  import { app, attempt, ensureTarget, openProjectDialog, refresh, saveSettings, savePrefs } from '../lib/app.svelte';
  import { call } from '../lib/api';
  import type { Performance, Permission } from '../lib/types';
  import Logo from './Logo.svelte';
  import ProviderForm from './ProviderForm.svelte';
  import Toggle from './Toggle.svelte';

  const steps = ['Welcome', 'Performance', 'Providers', 'Models', 'Permissions', 'Remote', 'Optimization', 'Ready'];
  let step = $state(0);
  let hw = $state<{ total_memory_gb: number; logical_cpus: number; recommended: Performance; reason: string } | null>(
    null,
  );
  let perf = $state<Performance>(app.settings.performance);
  let perm = $state<Permission>(app.settings.permission);
  let pref = $state<'single' | 'combo'>('single');
  let remote = $state(false);
  let compression = $state(app.settings.compression);
  let caveman = $state(app.settings.caveman);
  let showForm = $state(!app.connected.length);

  $effect(() => {
    call<typeof hw>('hardware').then((h) => {
      hw = h;
      if (h && !app.settings.setup_complete) perf = h.recommended;
    });
  });

  async function finish(openProject = false) {
    await attempt(async () => {
      await saveSettings({
        setup_complete: true,
        performance: perf,
        permission: perm,
        compression,
        caveman,
        auto_approve:
          perm === 'auto_approve' ? ['filesystem.read', 'filesystem.write', 'git.read'] : app.settings.auto_approve,
      });
      const target =
        pref === 'combo' && app.combos.length
          ? `combo:${app.combos[0].id}`
          : app.models[0]
            ? `model:${app.models[0].provider}/${app.models[0].model}`
            : null;
      await savePrefs({ default_target: target });
      if (remote) await call('remote_start');
      await refresh();
      ensureTarget();
    });
    app.wizardOpen = false;
    if (openProject && !app.project) void openProjectDialog();
  }
  const next = () => (step = Math.min(step + 1, steps.length - 1));
  const back = () => (step = Math.max(step - 1, 0));
</script>

<div class="wizard-bg">
  <div class="wizard" role="dialog" aria-modal="true" aria-label="Set up Conductor">
    <div class="progress" aria-label="Step {step + 1} of {steps.length}">
      {#each steps as s, i (s)}<span class="pdot" class:done={i < step} class:on={i === step} title={s}></span>{/each}
    </div>

    <div class="content">
      {#if step === 0}
        <div class="hero">
          <Logo size={72} />
          <h1>Welcome to Conductor</h1>
          <p class="muted lead">
            Let several AI models work together on your goals — while you stay in control. No account needed; everything
            stays on this computer.
          </p>
          <SetupStatus />
        </div>
      {:else if step === 1}
        <h2>How hard should Conductor push this machine?</h2>
        {#if hw}<p class="muted small sub">
            <Cpu size={13} />
            {hw.reason}. We recommend <strong>{hw.recommended}</strong>.
          </p>{/if}
        <div class="choices">
          {#each [['potato', 'Potato', 'Light on memory and CPU. One agent at a time, minimal animation.', Turtle], ['balanced', 'Balanced', 'Two agents in parallel. Good for most laptops.', Scale], ['maximum', 'Maximum', 'Up to four agents and faster indexing.', Rocket]] as const as [id, label, desc, Icon] (id)}
            <button class="choice" aria-pressed={perf === id} onclick={() => (perf = id)}>
              <Icon size={18} />
              <span
                ><strong>{label}</strong>{#if hw?.recommended === id}<span class="badge accent rec">recommended</span
                  >{/if}<br /><span class="small muted">{desc}</span></span
              >
            </button>
          {/each}
        </div>
      {:else if step === 2}
        <h2>Connect a provider</h2>
        <p class="muted small sub">One is enough. You can add more any time in Settings.</p>
        {#if app.connected.length}
          <ul class="list connected">
            {#each app.connected as p (p.id)}<li class="row small">
                <Check size={14} class="ok" />
                {p.name} <span class="faint xsmall">· {p.models.length} models</span>
              </li>{/each}
          </ul>
          {#if !showForm}<button class="btn sm" onclick={() => (showForm = true)}>Connect another</button>{/if}
        {/if}
        {#if showForm}<ProviderForm ondone={() => (showForm = false)} />{/if}
        <p class="xsmall faint note">
          No key? Use an app you're already signed in to (Codex, Claude Code, Antigravity, Grok Build) — Sign in opens
          the provider's own page. API keys go to your OS credential store.
        </p>
      {:else if step === 3}
        <h2>One model or a Combo?</h2>
        <p class="muted small sub">Both use the same picker next to the prompt. You can switch any time.</p>
        <div class="choices two">
          <button class="choice" aria-pressed={pref === 'single'} onclick={() => (pref = 'single')}>
            <span
              ><strong>Single model</strong><br /><span class="small muted"
                >Pick one model and talk to it directly. Simple and predictable.</span
              ></span
            >
          </button>
          <button class="choice" aria-pressed={pref === 'combo'} onclick={() => (pref = 'combo')}>
            <Layers size={18} />
            <span
              ><strong>Combo</strong><br /><span class="small muted"
                >Several models share the work — e.g. one plans, one codes, another reviews. Presets adapt to your
                providers.</span
              ></span
            >
          </button>
        </div>
      {:else if step === 4}
        <h2>How much should agents ask first?</h2>
        <div class="choices">
          <button class="choice" aria-pressed={perm === 'ask'} onclick={() => (perm = 'ask')}
            ><Shield size={18} /><span
              ><strong>Ask</strong><br /><span class="small muted">Approve anything beyond reading. Safest.</span></span
            ></button
          >
          <button class="choice" aria-pressed={perm === 'auto_approve'} onclick={() => (perm = 'auto_approve')}
            ><ShieldCheck size={18} /><span
              ><strong>Auto Approve</strong><br /><span class="small muted"
                >File edits are approved automatically; commands still ask.</span
              ></span
            ></button
          >
          <button class="choice" aria-pressed={perm === 'full_access'} onclick={() => (perm = 'full_access')}
            ><ShieldAlert size={18} /><span
              ><strong>Full Access</strong><br /><span class="small muted"
                >No repeated prompts. Dangerous commands and integrity checks still ask. Revoke any time.</span
              ></span
            ></button
          >
        </div>
      {:else if step === 5}
        <h2>Remote access</h2>
        <p class="muted small sub">
          Reach this machine's projects from another device — directly, encrypted, without any Conductor account. Off by
          default.
        </p>
        <div class="card row">
          <span class="grow">Turn on remote access now</span>
          <Toggle checked={remote} label="Remote access" onchange={(v) => (remote = v)} />
        </div>
        <p class="xsmall faint note">
          Devices pair with a one-time code and can be revoked instantly in Settings → Remote.
        </p>
      {:else if step === 6}
        <h2>Save tokens</h2>
        <p class="muted small sub">
          Conductor sends only the context a task needs. These stay on unless you turn them off.
        </p>
        <div class="card col">
          <div class="row">
            <span class="grow"
              ><strong class="small">Context compression</strong><br /><span class="xsmall muted"
                >Relevant files and symbols only; older chat summarised.</span
              ></span
            ><Toggle checked={compression} label="Context compression" onchange={(v) => (compression = v)} />
          </div>
          <hr class="divider" />
          <div class="row">
            <span class="grow"
              ><strong class="small">Caveman</strong><br /><span class="xsmall muted"
                >Asks models for terse answers while keeping code and errors exact. Based on Caveman by Julius Brussee
                (Apache-2.0).</span
              ></span
            ><Toggle checked={caveman} label="Caveman" onchange={(v) => (caveman = v)} />
          </div>
        </div>
      {:else}
        <div class="hero">
          <div class="ready-ico"><Check size={28} /></div>
          <h1>You're ready</h1>
          <p class="muted lead">
            Open a project folder and say what you want. Press <span class="kbd">Ctrl</span> <span class="kbd">K</span> any
            time for commands.
          </p>
        </div>
      {/if}
    </div>

    <div class="foot">
      {#if step === 0}
        <button class="btn ghost" onclick={() => finish(false)}>Skip setup</button>
        <span class="spacer"></span>
        <button class="btn primary lg" onclick={next}>Get started</button>
      {:else if step === steps.length - 1}
        <button class="btn ghost" onclick={back}>Back</button>
        <span class="spacer"></span>
        <button class="btn" onclick={() => finish(false)}>Done</button>
        <button class="btn primary" onclick={() => finish(true)}>Open a project</button>
      {:else}
        <button class="btn ghost" onclick={back}>Back</button>
        <span class="spacer"></span>
        <button class="btn ghost" onclick={next}>{step === 2 && !app.connected.length ? 'Skip for now' : 'Skip'}</button
        >
        <button class="btn primary" onclick={next}>Continue</button>
      {/if}
    </div>
  </div>
</div>

<style>
  .wizard-bg {
    position: fixed;
    inset: 0;
    z-index: 70;
    background: var(--bg);
    display: grid;
    place-items: center;
    padding: var(--s4);
  }
  .wizard {
    width: min(640px, 100%);
    min-height: min(560px, calc(100vh - 32px));
    max-height: calc(100vh - 32px);
    display: flex;
    flex-direction: column;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-lg);
  }
  .progress {
    display: flex;
    gap: 6px;
    justify-content: center;
    padding: var(--s5) 0 0;
  }
  .pdot {
    width: 6px;
    height: 6px;
    border-radius: 3px;
    background: var(--border-strong);
    transition: all var(--duration) var(--ease);
  }
  .pdot.done {
    background: var(--accent);
    opacity: 0.5;
  }
  .pdot.on {
    width: 20px;
    background: var(--accent);
  }
  .content {
    flex: 1;
    overflow: auto;
    padding: var(--s7) var(--s7) var(--s4);
  }
  .hero {
    text-align: center;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--s3);
    padding-top: var(--s6);
  }
  .lead {
    font-size: var(--fs-lg);
    max-width: 440px;
  }
  .sub {
    margin: var(--s2) 0 var(--s5);
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .choices {
    display: flex;
    flex-direction: column;
    gap: var(--s2);
  }
  .choices.two {
    display: grid;
    grid-template-columns: 1fr 1fr;
  }
  .choice {
    display: flex;
    gap: var(--s3);
    align-items: flex-start;
    padding: var(--s3) var(--s4);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg);
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition: border-color var(--duration) var(--ease);
  }
  .choice:hover {
    border-color: var(--border-strong);
  }
  .choice[aria-pressed='true'] {
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .choice :global(svg) {
    margin-top: 2px;
    color: var(--accent);
    flex: none;
  }
  .rec {
    margin-left: var(--s2);
  }
  .connected {
    margin-bottom: var(--s3);
  }
  .connected :global(.ok) {
    color: var(--success);
  }
  .note {
    margin-top: var(--s4);
  }
  .ready-ico {
    width: 64px;
    height: 64px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: var(--accent-soft);
    color: var(--accent);
  }
  .foot {
    display: flex;
    align-items: center;
    gap: var(--s2);
    padding: var(--s4) var(--s5);
    border-top: 1px solid var(--border);
  }
  @media (max-width: 640px) {
    .content {
      padding: var(--s5) var(--s4) var(--s3);
    }
    .choices.two {
      grid-template-columns: 1fr;
    }
  }
</style>
