<script lang="ts">
  import { Gauge, Plug, Settings, Palette, Download, Info, Bell, BellOff } from '@lucide/svelte';
  import { app, attempt, savePrefs, type SettingsTab } from '../lib/app.svelte';
  import { call } from '../lib/api';
  import { keyLabel } from '../lib/format';
  import type { SubscriptionUsage, UsageOverview } from '../lib/types';

  let open = $state(false);
  let user = $state('');
  let summary = $state<{ label: string; used: number }[]>([]);
  let root: HTMLDivElement | undefined = $state();

  $effect(() => {
    call<{ user: string }>('profile_info')
      .then((p) => (user = p.user))
      .catch(() => {});
  });

  async function loadSummary() {
    const o = await call<UsageOverview>('usage_overview').catch(() => null);
    if (!o) return;
    const rows: { label: string; used: number }[] = [];
    for (const s of o.subscriptions.filter((s) => s.signed_in)) {
      const u = await call<SubscriptionUsage>('subscription_usage', { service: s.service }).catch(() => null);
      const top = u?.windows.reduce((a, w) => (w.used_percent > a ? w.used_percent : a), 0);
      if (top !== undefined) rows.push({ label: s.label, used: Math.round(top) });
    }
    for (const p of o.providers) {
      const w = p.limits?.tokens.limit ? p.limits.tokens : p.limits?.requests;
      if (w?.limit && w.remaining !== null)
        rows.push({ label: p.name, used: Math.round(100 - (w.remaining / w.limit) * 100) });
    }
    summary = rows.slice(0, 4);
  }

  function toggle() {
    open = !open;
    if (open) void loadSummary();
  }
  function go(tab: SettingsTab) {
    open = false;
    app.settingsTab = tab;
  }
  const initial = $derived((user.trim()[0] ?? 'Y').toUpperCase());
  const items: { tab: SettingsTab; label: string; icon: typeof Gauge; hint?: string }[] = [
    { tab: 'usage', label: 'Usage', icon: Gauge },
    { tab: 'providers', label: 'Providers & accounts', icon: Plug },
    { tab: 'general', label: 'Settings', icon: Settings, hint: keyLabel('Mod+,') },
    { tab: 'appearance', label: 'Appearance', icon: Palette },
    { tab: 'updates', label: 'Updates', icon: Download },
    { tab: 'about', label: 'About Conductor', icon: Info },
  ];
</script>

<svelte:window
  onkeydown={(e) => e.key === 'Escape' && open && (open = false)}
  onpointerdown={(e) => open && root && !root.contains(e.target as Node) && (open = false)}
/>

<div class="profile" bind:this={root}>
  {#if open}
    <div class="menu" role="menu" aria-label="Profile">
      <div class="who">
        <span class="avatar lg" aria-hidden="true">{initial}</span>
        <div class="grow">
          <strong>{user || 'You'}</strong>
          <p class="xsmall muted">Local profile · no Conductor account</p>
        </div>
      </div>
      {#if summary.length}
        <div class="usage" aria-label="Usage at a glance">
          {#each summary as r (r.label)}
            <div class="urow xsmall">
              <span class="grow">{r.label}</span><span class="num">{r.used}% used</span>
            </div>
            <div class="bar"><span class:hot={r.used >= 90} style:width="{r.used}%"></span></div>
          {/each}
        </div>
      {/if}
      {#each items as it (it.tab)}
        <button role="menuitem" class="item" onclick={() => go(it.tab)}>
          <it.icon size={15} />
          <span class="grow">{it.label}</span>
          {#if it.hint}<span class="kbd">{it.hint}</span>{/if}
        </button>
      {/each}
      <button
        role="menuitemcheckbox"
        aria-checked={app.prefs?.notify_done !== false}
        class="item"
        onclick={() => attempt(() => savePrefs({ notify_done: app.prefs?.notify_done === false }))}
      >
        {#if app.prefs?.notify_done !== false}<Bell size={15} />{:else}<BellOff size={15} />{/if}
        <span class="grow">Notifications</span>
        <span class="xsmall muted">{app.prefs?.notify_done !== false ? 'On' : 'Off'}</span>
      </button>
    </div>
  {/if}
  <button class="trigger" aria-haspopup="menu" aria-expanded={open} aria-label="Profile and settings" onclick={toggle}>
    <span class="avatar" aria-hidden="true">{initial}</span>
  </button>
</div>

<style>
  .profile {
    position: relative;
  }
  .trigger {
    border: none;
    background: none;
    padding: 2px;
    border-radius: 50%;
    cursor: pointer;
    display: grid;
  }
  .trigger:hover .avatar,
  .trigger[aria-expanded='true'] .avatar {
    box-shadow: 0 0 0 2px var(--accent-soft);
  }
  .avatar {
    width: 26px;
    height: 26px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: var(--accent);
    color: var(--accent-text);
    font-size: 13px;
    font-weight: 600;
  }
  .avatar.lg {
    width: 34px;
    height: 34px;
    font-size: 15px;
  }
  .menu {
    position: absolute;
    bottom: calc(100% + 8px);
    right: 0;
    width: calc(var(--sidebar-w) - 24px);
    z-index: 40;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow-lg);
    padding: var(--s1);
    display: grid;
  }
  .who {
    display: flex;
    gap: var(--s3);
    align-items: center;
    padding: var(--s2) var(--s2) var(--s3);
    border-bottom: 1px solid var(--border);
    margin-bottom: var(--s1);
  }
  .who p {
    margin: 0;
  }
  .usage {
    display: grid;
    gap: 3px;
    padding: var(--s1) var(--s2) var(--s3);
    border-bottom: 1px solid var(--border);
    margin-bottom: var(--s1);
  }
  .urow {
    display: flex;
  }
  .num {
    font-variant-numeric: tabular-nums;
    color: var(--text-muted);
  }
  .bar {
    height: 4px;
    border-radius: 99px;
    background: var(--border);
    overflow: hidden;
    margin-bottom: 4px;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
  }
  .bar span.hot {
    background: var(--warning);
  }
  .item {
    display: flex;
    align-items: center;
    gap: var(--s2);
    border: none;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-sm);
    text-align: left;
    padding: 7px var(--s2);
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  .item:hover,
  .item:focus-visible {
    background: var(--surface-hover);
  }
</style>
