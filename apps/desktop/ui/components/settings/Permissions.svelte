<script lang="ts">
  import { Shield, ShieldCheck, ShieldAlert, RotateCcw } from '@lucide/svelte';
  import { app, attempt, saveSettings, savePrefs } from '../../lib/app.svelte';
  import Toggle from '../Toggle.svelte';
  import type { Permission } from '../../lib/types';

  const categories: [string, string, string][] = [
    ['filesystem.write', 'Create and edit files', 'Change files in the selected project.'],
    ['filesystem.delete', 'Delete files', 'Remove files in the selected project.'],
    ['terminal.execute', 'Run commands', 'Run programs with your user account.'],
    ['git.write', 'Commit and branch', 'Create commits and branches in the project.'],
    ['network', 'Network access', 'Contact websites and local services.'],
    ['mcp', 'MCP tools', 'Use tools from enabled integrations.'],
    ['secrets.use', 'Use saved integration credentials', 'Pass referenced credentials to the selected integration.'],
    ['install.software', 'Install software', 'Install tools needed by the project.'],
  ];
  const levels: { id: Permission; label: string; desc: string; icon: typeof Shield }[] = [
    { id: 'ask', label: 'Ask', desc: 'Approve every action beyond reading.', icon: Shield },
    {
      id: 'auto_approve',
      label: 'Auto Approve',
      desc: 'Approve the categories you choose automatically.',
      icon: ShieldCheck,
    },
    {
      id: 'full_access',
      label: 'Full Access',
      desc: 'No repeated prompts. Dangerous commands still ask, integrity checks always run, and you can revoke it instantly.',
      icon: ShieldAlert,
    },
  ];

  function toggleCat(cat: string, on: boolean) {
    const s = new Set(app.settings.auto_approve);
    if (on) s.add(cat);
    else s.delete(cat);
    void attempt(() => saveSettings({ auto_approve: [...s] }));
  }
  const maxEntries = $derived(Object.entries(app.prefs?.max_effort ?? {}));
</script>

<h2>Permissions</h2>
<p class="lede">What agents may do without asking. Plan mode never edits files, whatever this is set to.</p>

<div class="levels" role="radiogroup" aria-label="Permission level">
  {#each levels as l (l.id)}
    <button
      class="level {l.id}"
      role="radio"
      aria-checked={app.settings.permission === l.id}
      onclick={() => attempt(() => saveSettings({ permission: l.id }))}
    >
      <l.icon size={18} />
      <span><strong>{l.label}</strong><br /><span class="small muted">{l.desc}</span></span>
    </button>
  {/each}
</div>

{#if app.settings.permission === 'auto_approve'}
  <h3 class="sub">Approve automatically</h3>
  {#each categories as [id, label, description] (id)}
    <div class="setting">
      <div class="text">
        <strong>{label}</strong>
        <p>{description}</p>
      </div>
      <Toggle checked={app.settings.auto_approve.includes(id)} {label} onchange={(v) => toggleCat(id, v)} />
    </div>
  {/each}
{/if}

<h3 class="sub">Highest effort</h3>
<div class="setting">
  <div class="text">
    <strong>Allow automatic maximum effort</strong>
    <p>Off by default: Conductor asks before using Max/Extra-high effort, because it consumes premium usage.</p>
  </div>
  <Toggle
    checked={app.settings.allow_highest_effort}
    label="Allow automatic maximum effort"
    onchange={(v) => attempt(() => saveSettings({ allow_highest_effort: v }))}
  />
</div>
{#if maxEntries.length}
  {#each maxEntries as [model, policy] (model)}
    <div class="setting">
      <div class="text">
        <strong class="mono">{model}</strong>
        <p>{policy === 'always_allow' ? 'Always allowed' : policy === 'never' ? 'Never — and don’t ask' : 'Ask'}</p>
      </div>
      <button
        class="btn sm"
        onclick={() => {
          const m = { ...app.prefs!.max_effort };
          delete m[model];
          void savePrefs({ max_effort: m });
        }}><RotateCcw size={12} /> Reset</button
      >
    </div>
  {/each}
{/if}

<style>
  .levels {
    display: flex;
    flex-direction: column;
    gap: var(--s2);
  }
  .level {
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
  }
  .level :global(svg) {
    margin-top: 2px;
    color: var(--accent);
    flex: none;
  }
  .level[aria-checked='true'] {
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .level.full_access[aria-checked='true'] {
    border-color: var(--danger);
    background: var(--danger-soft);
  }
  .level.full_access :global(svg) {
    color: var(--danger);
  }
</style>
