<script lang="ts">
  import { RefreshCw, LogIn, LogOut, TriangleAlert } from '@lucide/svelte';
  import { app, attempt, savePrefs } from '../../lib/app.svelte';
  import { call } from '../../lib/api';
  import type { RateWindow, SubscriptionUsage, UsageOverview } from '../../lib/types';
  import Toggle from '../Toggle.svelte';

  type Brand = 'claude' | 'chatgpt' | 'gemini' | 'other';
  const brands: { id: Brand; label: string; kind: string | null; vendor: string }[] = [
    { id: 'claude', label: 'Claude', kind: 'anthropic', vendor: 'Anthropic' },
    { id: 'chatgpt', label: 'ChatGPT', kind: 'openai', vendor: 'OpenAI' },
    { id: 'gemini', label: 'Gemini', kind: 'gemini', vendor: 'Google' },
    { id: 'other', label: 'Other', kind: 'openai_compatible', vendor: 'OpenAI-compatible' },
  ];

  let overview = $state<UsageOverview | null>(null);
  let brand = $state<Brand>('claude');
  let subUsage = $state<Record<string, SubscriptionUsage | string>>({});
  let busy = $state<string | null>(null);

  const current = $derived(brands.find((b) => b.id === brand)!);
  const apiProviders = $derived(overview?.providers.filter((p) => p.kind === current.kind) ?? []);
  const sub = $derived(overview?.subscriptions.find((s) => s.service === brand) ?? null);

  async function load() {
    overview = await call<UsageOverview>('usage_overview').catch(() => null);
    for (const s of overview?.subscriptions ?? [])
      if (s.signed_in && !(s.service in subUsage)) void fetchSub(s.service);
  }
  $effect(() => {
    void load();
  });

  async function fetchSub(service: string) {
    busy = service;
    try {
      subUsage[service] = await call<SubscriptionUsage>('subscription_usage', { service });
    } catch (e) {
      subUsage[service] = String(e instanceof Error ? e.message : e);
    }
    busy = null;
  }
  async function signIn(service: string, label: string) {
    busy = service;
    const r = await attempt(() => call('subscription_sign_in', { service }), `Signed in to ${label}`);
    busy = null;
    if (r !== undefined) {
      delete subUsage[service];
      await load();
    }
  }
  async function signOut(service: string, label: string) {
    await attempt(() => call('subscription_sign_out', { service }), `Signed out of ${label}`);
    delete subUsage[service];
    await load();
  }
  async function setOptIn(on: boolean) {
    if (
      on &&
      !confirm(
        'Subscription sign-in is not supported by Anthropic, OpenAI or Google. It reuses the sign-in of their command-line tools and reads private usage pages, so it can stop working at any time. Using it from another app may break their terms, and your account could be flagged.\n\nTurn it on anyway?',
      )
    )
      return;
    await attempt(() => savePrefs({ subscription_signin: on }));
    subUsage = {};
    await load();
  }

  // ---------- formatting ----------
  function planOf(s: UsageOverview['subscriptions'][number]): string | null {
    const u = subUsage[s.service];
    return (typeof u === 'object' ? u.plan : null) ?? s.plan;
  }
  function resetText(w: { resets_at?: string | null; resets_at_unix?: number | null }): string {
    const t = w.resets_at ? Date.parse(w.resets_at) : w.resets_at_unix ? w.resets_at_unix * 1000 : NaN;
    if (Number.isNaN(t)) return '';
    const mins = Math.max(0, Math.round((t - Date.now()) / 60000));
    if (mins < 60) return `resets in ${mins} min`;
    if (mins < 48 * 60) return `resets in ${Math.floor(mins / 60)} h ${mins % 60} min`;
    return `resets ${new Date(t).toLocaleDateString(undefined, { weekday: 'short', month: 'short', day: 'numeric' })}`;
  }
  function rateReset(r: string | null): string {
    if (!r) return '';
    const t = Date.parse(r);
    if (!Number.isNaN(t) && /\d{4}-/.test(r)) return resetText({ resets_at: r });
    return `resets in ${r}`;
  }
  const fmt = (n: number) => (n >= 10000 ? `${(n / 1000).toFixed(n >= 100000 ? 0 : 1)}k` : n.toLocaleString());
  const usedOf = (w: RateWindow) =>
    w.limit && w.remaining !== null ? Math.min(100, Math.max(0, 100 - (w.remaining / w.limit) * 100)) : null;
  function rateRows(l: NonNullable<UsageOverview['providers'][number]['limits']>) {
    return (
      [
        ['Requests', l.requests],
        ['Tokens', l.tokens],
        ['Input tokens', l.input_tokens],
        ['Output tokens', l.output_tokens],
      ] as const
    ).filter(([, w]) => w && (w.limit !== null || w.remaining !== null));
  }
</script>

<h2>Usage</h2>
<p class="lede">
  How much you have left with each AI. Numbers come straight from the provider — nothing here is estimated.
</p>

<div class="brands" role="tablist" aria-label="AI provider">
  {#each brands as b (b.id)}
    <button role="tab" class="brand-tab brand-{b.id}" aria-selected={brand === b.id} onclick={() => (brand = b.id)}
      >{b.label}</button
    >
  {/each}
</div>

<div class="themed brand-{brand}" data-testid="usage-panel" data-brand={brand}>
  <header class="hero">
    <div class="glyph" aria-hidden="true">{current.label[0]}</div>
    <div class="grow">
      <h3>{current.label}</h3>
      <p class="xsmall sub-muted">{current.vendor}</p>
    </div>
    <button class="btn sm ghost" onclick={load} aria-label="Refresh usage"><RefreshCw size={13} /> Refresh</button>
  </header>

  {#if brand !== 'other' && overview?.subscriptions_enabled && sub}
    <section class="block" aria-label="{current.label} subscription">
      <div class="row">
        <strong class="grow">{current.label} subscription</strong>
        {#if sub.signed_in}
          <button class="btn sm ghost" onclick={() => fetchSub(sub.service)} disabled={busy === sub.service}
            ><RefreshCw size={12} /> Update</button
          >
          <button class="btn sm ghost" onclick={() => signOut(sub.service, sub.label)}
            ><LogOut size={12} /> Sign out</button
          >
        {/if}
      </div>
      {#if !sub.signed_in}
        <p class="small sub-muted">Sign in with your {current.label} account to see your plan's limits.</p>
        <button class="btn brand-btn" onclick={() => signIn(sub.service, sub.label)} disabled={busy === sub.service}
          ><LogIn size={14} /> {busy === sub.service ? 'Waiting for browser…' : `Sign in with ${current.label}`}</button
        >
      {:else}
        <p class="xsmall sub-muted">{[sub.account ?? 'Signed in', planOf(sub)].filter(Boolean).join(' · ')}</p>
        {#if busy === sub.service && !subUsage[sub.service]}
          <p class="small sub-muted">Loading…</p>
        {:else if typeof subUsage[sub.service] === 'string'}
          <p class="small warn">{subUsage[sub.service]}</p>
        {:else if subUsage[sub.service]}
          {@const u = subUsage[sub.service] as SubscriptionUsage}
          {#each u.windows as w (w.label)}
            <div class="meter-row">
              <div class="row xsmall">
                <span class="grow">{w.label}</span>
                <span class="num">{Math.round(w.used_percent)}% used</span>
              </div>
              <div
                class="meter"
                role="meter"
                aria-label="{w.label} used"
                aria-valuemin="0"
                aria-valuemax="100"
                aria-valuenow={Math.round(w.used_percent)}
              >
                <span class:hot={w.used_percent >= 80} style:width="{w.used_percent}%"></span>
              </div>
              <div class="xsmall sub-muted">{resetText(w)}</div>
            </div>
          {:else}
            <p class="small sub-muted">{current.label} reported no limits for this plan.</p>
          {/each}
        {/if}
      {/if}
    </section>
  {/if}

  {#each apiProviders as p (p.id)}
    <section class="block" aria-label="{p.name} API usage">
      <div class="row">
        <strong class="grow">{p.name}</strong>
        <span class="xsmall sub-muted">API key{p.enabled ? '' : ' · signed out'}</span>
      </div>
      {#if p.limits}
        {#each rateRows(p.limits) as [label, w] (label)}
          {@const used = usedOf(w)}
          <div class="meter-row">
            <div class="row xsmall">
              <span class="grow">{label}</span>
              <span class="num"
                >{w.remaining !== null ? fmt(w.remaining) : '?'}{w.limit !== null ? ` of ${fmt(w.limit)}` : ''} left</span
              >
            </div>
            {#if used !== null}
              <div
                class="meter"
                role="meter"
                aria-label="{p.name} {label} used"
                aria-valuemin="0"
                aria-valuemax="100"
                aria-valuenow={Math.round(used)}
              >
                <span class:hot={used >= 80} style:width="{used}%"></span>
              </div>
            {/if}
            <div class="xsmall sub-muted">{rateReset(w.reset)}</div>
          </div>
        {/each}
        <p class="xsmall sub-muted">Rate limits per minute, as of your last request.</p>
      {:else if p.kind === 'gemini'}
        <p class="small sub-muted">
          The Gemini API doesn't report remaining limits. Check Google AI Studio for quotas.
        </p>
      {:else}
        <p class="small sub-muted">Send a message with {p.name} and its rate limits show up here.</p>
      {/if}
      <p class="xsmall sub-muted session">
        This session: {fmt(p.session.requests)} replies · {fmt(p.session.input)} in · {fmt(p.session.output)} out tokens
      </p>
    </section>
  {:else}
    {#if brand === 'other' || !overview?.subscriptions_enabled}
      <section class="block empty">
        <p class="small sub-muted">
          No {current.vendor} API key connected.
          <button class="link" onclick={() => (app.settingsTab = 'providers')}>Add one in Providers</button>
        </p>
      </section>
    {/if}
  {/each}
</div>

<h3 class="sub">Subscription sign-in</h3>
<div class="row optin">
  <div class="grow">
    <div class="row">
      <TriangleAlert size={14} class="warn" /> <strong>Sign in with Claude, ChatGPT or Gemini</strong>
    </div>
    <p class="xsmall muted">
      Not supported by the providers, so it's off by default. It reuses each vendor's command-line sign-in to show your
      plan's limits. You sign in in your own browser; Conductor never sees your password. Only a refresh token is kept,
      in your OS keychain.
    </p>
  </div>
  <Toggle checked={!!app.prefs?.subscription_signin} label="Subscription sign-in" onchange={(v) => setOptIn(v)} />
</div>

<style>
  .brands {
    display: flex;
    gap: var(--s1);
    margin-bottom: var(--s3);
    flex-wrap: wrap;
  }
  .brand-tab {
    border: 1px solid var(--border);
    background: var(--surface);
    color: var(--text-muted);
    border-radius: 999px;
    padding: 5px 14px;
    font: inherit;
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .brand-tab[aria-selected='true'] {
    color: var(--b-on);
    background: var(--b-fill);
    border-color: transparent;
  }

  /* Each provider gets its own look. Colors only — no logos. */
  .brand-claude {
    --b-accent: #d97757;
    --b-fill: #d97757;
    --b-on: #fff;
    --b-bg: #faf9f5;
    --b-card: #ffffff;
    --b-border: #e8e2d6;
    --b-text: #3d3929;
    --b-muted: #7a7466;
    --b-track: #efe9df;
    --b-font: 'Tiempos Headline', Georgia, 'Times New Roman', serif;
  }
  .brand-chatgpt {
    --b-accent: #10a37f;
    --b-fill: #0d0d0d;
    --b-on: #fff;
    --b-bg: #ffffff;
    --b-card: #f9f9f9;
    --b-border: #e5e5e5;
    --b-text: #0d0d0d;
    --b-muted: #6b6b6b;
    --b-track: #ececec;
    --b-font: 'Söhne', ui-sans-serif, system-ui, 'Segoe UI', sans-serif;
  }
  .brand-gemini {
    --b-accent: #4285f4;
    --b-fill: linear-gradient(90deg, #4285f4, #9b72cb, #d96570);
    --b-on: #fff;
    --b-bg: #f8fafd;
    --b-card: #ffffff;
    --b-border: #dde3ea;
    --b-text: #1f1f1f;
    --b-muted: #5f6368;
    --b-track: #e9eef6;
    --b-font: 'Google Sans', 'Product Sans', 'Segoe UI', system-ui, sans-serif;
  }
  .brand-other {
    --b-accent: var(--accent);
    --b-fill: var(--accent);
    --b-on: var(--accent-text);
    --b-bg: var(--bg-sunken);
    --b-card: var(--surface);
    --b-border: var(--border);
    --b-text: var(--text);
    --b-muted: var(--text-muted);
    --b-track: var(--border);
    --b-font: var(--font-ui);
  }
  @media (prefers-color-scheme: dark) {
    :global(:root:not([data-theme='light'])) .brand-claude {
      --b-bg: #262624;
      --b-card: #30302e;
      --b-border: #3e3e3a;
      --b-text: #f5f4ee;
      --b-muted: #a6a39a;
      --b-track: #3e3e3a;
    }
    :global(:root:not([data-theme='light'])) .brand-chatgpt {
      --b-fill: #ffffff;
      --b-on: #0d0d0d;
      --b-bg: #212121;
      --b-card: #2f2f2f;
      --b-border: #3d3d3d;
      --b-text: #ececec;
      --b-muted: #a3a3a3;
      --b-track: #424242;
    }
    :global(:root:not([data-theme='light'])) .brand-gemini {
      --b-bg: #131314;
      --b-card: #1e1f20;
      --b-border: #333537;
      --b-text: #e3e3e3;
      --b-muted: #9aa0a6;
      --b-track: #333537;
    }
  }
  :global(:root[data-theme='dark']) .brand-claude {
    --b-bg: #262624;
    --b-card: #30302e;
    --b-border: #3e3e3a;
    --b-text: #f5f4ee;
    --b-muted: #a6a39a;
    --b-track: #3e3e3a;
  }
  :global(:root[data-theme='dark']) .brand-chatgpt {
    --b-fill: #ffffff;
    --b-on: #0d0d0d;
    --b-bg: #212121;
    --b-card: #2f2f2f;
    --b-border: #3d3d3d;
    --b-text: #ececec;
    --b-muted: #a3a3a3;
    --b-track: #424242;
  }
  :global(:root[data-theme='dark']) .brand-gemini {
    --b-bg: #131314;
    --b-card: #1e1f20;
    --b-border: #333537;
    --b-text: #e3e3e3;
    --b-muted: #9aa0a6;
    --b-track: #333537;
  }

  .themed {
    background: var(--b-bg);
    color: var(--b-text);
    border: 1px solid var(--b-border);
    border-radius: var(--radius-lg);
    padding: var(--s4);
    display: grid;
    gap: var(--s3);
    transition:
      background var(--duration) var(--ease),
      color var(--duration) var(--ease);
  }
  .hero {
    display: flex;
    align-items: center;
    gap: var(--s3);
  }
  .hero h3 {
    margin: 0;
    font-family: var(--b-font);
    font-size: var(--fs-xl);
    font-weight: 500;
  }
  .glyph {
    width: 36px;
    height: 36px;
    border-radius: 10px;
    display: grid;
    place-items: center;
    background: var(--b-fill);
    color: var(--b-on);
    font-family: var(--b-font);
    font-weight: 600;
    font-size: 18px;
  }
  .block {
    background: var(--b-card);
    border: 1px solid var(--b-border);
    border-radius: var(--radius);
    padding: var(--s3) var(--s4);
    display: grid;
    gap: var(--s2);
  }
  .sub-muted {
    color: var(--b-muted);
    margin: 0;
  }
  .themed :global(.btn.ghost) {
    color: var(--b-text);
  }
  .brand-btn {
    justify-self: start;
    background: var(--b-fill);
    color: var(--b-on);
    border: none;
  }
  .brand-btn:hover:not(:disabled) {
    filter: brightness(1.08);
  }
  .meter-row {
    display: grid;
    gap: 4px;
  }
  .meter {
    height: 8px;
    border-radius: 999px;
    background: var(--b-track);
    overflow: hidden;
  }
  .meter span {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--b-fill);
    transition: width 300ms var(--ease);
  }
  .meter span.hot {
    background: var(--warning);
  }
  .num {
    font-variant-numeric: tabular-nums;
  }
  .session {
    border-top: 1px dashed var(--b-border);
    padding-top: var(--s2);
  }
  .warn {
    color: var(--warning);
    margin: 0;
  }
  .link {
    background: none;
    border: none;
    padding: 0;
    color: var(--b-accent);
    text-decoration: underline;
    cursor: pointer;
    font: inherit;
  }
  .optin {
    align-items: flex-start;
    gap: var(--s4);
  }
  .optin p {
    margin: var(--s1) 0 0;
  }
</style>
