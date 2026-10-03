<script lang="ts">
  import { Plus, Copy, Trash2, Download, Upload, Star, Layers } from '@lucide/svelte';
  import { app, attempt, refresh, succeeded, toast } from '../../lib/app.svelte';
  import { call } from '../../lib/api';
  import { roleLabels } from '../../lib/format';
  import type { AgentRole, Combo } from '../../lib/types';
  import Toggle from '../Toggle.svelte';
  import Modal from '../Modal.svelte';

  let editing = $state<Combo | null>(null);
  let importing = $state(false);
  let importText = $state('');
  let exported = $state<string | null>(null);

  const roles = Object.keys(roleLabels) as AgentRole[];
  const strategies: [string, string][] = [
    ['balanced', 'Balanced'],
    ['maximum_quality', 'Maximum quality'],
    ['lowest_cost', 'Lowest cost'],
    ['fastest', 'Fastest'],
    ['subscription_first', 'Subscription first'],
    ['prefer_provider', 'Prefer a provider'],
    ['preserve_provider', 'Preserve a provider'],
    ['manual_weights', 'Manual weights'],
  ];
  const providers = $derived([...new Set(app.models.map((m) => m.provider))]);

  function blank(): Combo {
    const first = app.models[0];
    return {
      id: crypto.randomUUID().replace(/-/g, ''),
      name: 'My Combo',
      description: '',
      version: 1,
      members: first
        ? [{ model: `${first.provider}/${first.model}`, roles: [], effort: null, enabled: true, weight: 1 }]
        : [],
      strategy: 'balanced',
      strategy_provider: null,
      reserves: [],
      fallback: true,
      review: 'important',
      max_parallel: 2,
      budget: { max_tokens: null, max_cost_usd: null, ask_at_fraction: null },
      instructions: '',
      phases: [],
      builtin: false,
      history: [],
    };
  }
  function effortsFor(model: string) {
    return app.models.find((m) => `${m.provider}/${m.model}` === model)?.efforts ?? [];
  }
  async function save() {
    if (!editing) return;
    if (!editing.members.some((m) => m.enabled)) {
      toast('A Combo needs at least one enabled model.', 'error');
      return;
    }
    const ok = await succeeded(() => call('combo_save', { combo: editing }), 'Combo saved');
    if (ok) {
      editing = null;
      await refresh();
    }
  }
  async function dup(c: Combo) {
    const r = await attempt(() => call<Combo>('combo_duplicate', { id: c.id, name: `${c.name} copy` }));
    if (r) {
      await refresh();
      editing = structuredClone(r);
    }
  }
  async function del(c: Combo) {
    if (!confirm(`Delete ${c.name}?`)) return;
    await attempt(() => call('combo_delete', { id: c.id }));
    await refresh();
  }
  async function setDefault(c: Combo) {
    await attempt(() => call('combo_set_default', { id: app.defaultCombo === c.id ? null : c.id }));
    await refresh();
  }
  async function exp(c: Combo) {
    exported = await call<string>('combo_export', { id: c.id });
  }
  async function imp() {
    const r = await attempt(() => call<{ combo: Combo; warnings: string[] }>('combo_import', { json: importText }));
    if (r) {
      importing = false;
      importText = '';
      toast(
        r.warnings.length ? `Imported with notes: ${r.warnings.join('; ')}` : `Imported ${r.combo.name}`,
        'success',
      );
      await refresh();
    }
  }
</script>

<h2>Combos</h2>
<p class="lede">
  A Combo lets several models share work — by role, effort and usage rules. Presets adapt to the models you've
  connected.
</p>

<div class="row toolbar">
  <button class="btn primary" onclick={() => (editing = blank())} disabled={!app.models.length}
    ><Plus size={14} /> New Combo</button
  >
  <button class="btn" onclick={() => (importing = true)}><Upload size={14} /> Import</button>
</div>

{#if !app.models.length}
  <p class="muted small">Connect a provider to get Combo presets.</p>
{/if}

<div class="combos">
  {#each app.combos as c (c.id)}
    <div class="combo card">
      <div class="row">
        <Layers size={15} class="ic" />
        <strong class="grow">{c.name}</strong>
        {#if c.builtin}<span class="badge">preset</span>{:else}<span class="badge">v{c.version}</span>{/if}
        <button
          class="btn ghost icon sm"
          class:on={app.defaultCombo === c.id}
          aria-label="Use as default"
          title="Default Combo"
          onclick={() => setDefault(c)}><Star size={13} /></button
        >
      </div>
      {#if c.description}<p class="xsmall muted desc">{c.description}</p>{/if}
      <ul class="list members">
        {#each c.members as m, mi (mi)}
          <li class:off={!m.enabled}>
            <span class="mono xsmall grow">{m.model}</span>
            {#each m.roles as r (r)}<span class="role">{roleLabels[r]}</span>{/each}
          </li>
        {/each}
      </ul>
      <div class="row actions">
        <button class="btn sm" onclick={() => (editing = structuredClone($state.snapshot(c)) as Combo)}
          >{c.builtin ? 'Customize' : 'Edit'}</button
        >
        <button class="btn sm ghost icon" aria-label="Duplicate" title="Duplicate" onclick={() => dup(c)}
          ><Copy size={13} /></button
        >
        <button class="btn sm ghost icon" aria-label="Export" title="Export / share" onclick={() => exp(c)}
          ><Download size={13} /></button
        >
        {#if !c.builtin}<button class="btn sm ghost icon" aria-label="Delete" onclick={() => del(c)}
            ><Trash2 size={13} /></button
          >{/if}
      </div>
    </div>
  {/each}
</div>

{#if editing}
  <Modal
    title={editing.builtin ? `Customize ${editing.name}` : 'Edit Combo'}
    onclose={() => (editing = null)}
    width={760}
  >
    <div class="field">
      <label class="label" for="cb-name">Name</label>
      <input id="cb-name" class="input" bind:value={editing.name} />
    </div>
    <span class="label">Models</span>
    <table class="table members-edit">
      <thead><tr><th>Model</th><th>Roles</th><th>Effort</th><th>On</th><th></th></tr></thead>
      <tbody>
        {#each editing.members as m, i (i)}
          <tr>
            <td>
              <select class="select" bind:value={m.model} aria-label="Model">
                {#each app.models as x (x.provider + x.model)}<option value="{x.provider}/{x.model}">{x.display}</option
                  >{/each}
              </select>
            </td>
            <td class="roles-cell">
              {#each roles as r (r)}
                <button
                  class="rchip"
                  aria-pressed={m.roles.includes(r)}
                  onclick={() => (m.roles = m.roles.includes(r) ? m.roles.filter((x) => x !== r) : [...m.roles, r])}
                  >{roleLabels[r]}</button
                >
              {/each}
            </td>
            <td>
              <select class="select" bind:value={m.effort} aria-label="Effort">
                <option value={null}>Auto</option>
                {#each effortsFor(m.model) as e (e)}<option value={e}>{e}</option>{/each}
              </select>
            </td>
            <td><Toggle checked={m.enabled} label="Enabled" onchange={(v) => (m.enabled = v)} /></td>
            <td
              ><button
                class="btn ghost icon sm"
                aria-label="Remove member"
                onclick={() => editing && (editing.members = editing.members.filter((_, j) => j !== i))}
                ><Trash2 size={13} /></button
              ></td
            >
          </tr>
        {/each}
      </tbody>
    </table>
    <button
      class="btn sm add-m"
      onclick={() =>
        editing &&
        app.models[0] &&
        (editing.members = [
          ...editing.members,
          {
            model: `${app.models[0].provider}/${app.models[0].model}`,
            roles: [],
            effort: null,
            enabled: true,
            weight: 1,
          },
        ])}><Plus size={13} /> Add model</button
    >
    <p class="hint">
      Roles are preferences: if a model runs out of usage or fails, another member takes over its role with a compact
      handoff.
    </p>

    <div class="grid2">
      <div class="field">
        <label class="label" for="cb-strat">Routing</label>
        <select id="cb-strat" class="select" bind:value={editing.strategy}>
          {#each strategies as [v, l] (v)}<option value={v}>{l}</option>{/each}
        </select>
      </div>
      {#if editing.strategy === 'prefer_provider' || editing.strategy === 'preserve_provider'}
        <div class="field">
          <label class="label" for="cb-prov">Provider</label>
          <select id="cb-prov" class="select" bind:value={editing.strategy_provider}>
            {#each providers as p (p)}<option value={p}>{p}</option>{/each}
          </select>
        </div>
      {/if}
      <div class="field">
        <label class="label" for="cb-review">Independent review</label>
        <select id="cb-review" class="select" bind:value={editing.review}>
          <option value="off">Off</option>
          <option value="important">Important tasks</option>
          <option value="always">Always</option>
        </select>
      </div>
      <div class="field">
        <label class="label" for="cb-par">Parallel agents</label>
        <input id="cb-par" class="input" type="number" min="1" max="4" bind:value={editing.max_parallel} />
      </div>
    </div>

    <span class="label">Usage Reserve</span>
    {#each editing.reserves as r, i (i)}
      <div class="row reserve">
        <span class="small">Keep</span>
        <input
          class="input pct"
          type="number"
          min="0"
          max="90"
          value={Math.round(r.fraction * 100)}
          oninput={(e) => (r.fraction = Math.min(0.9, Math.max(0, Number((e.target as HTMLInputElement).value) / 100)))}
          aria-label="Reserve percent"
        />
        <span class="small">% of</span>
        <select class="select prov" bind:value={r.provider} aria-label="Provider"
          >{#each providers as p (p)}<option value={p}>{p}</option>{/each}</select
        >
        <span class="small">for</span>
        <select
          class="select prov"
          value={r.for_roles[0] ?? 'reviewer'}
          onchange={(e) => (r.for_roles = [(e.target as HTMLSelectElement).value as AgentRole])}
          aria-label="Role"
          >{#each roles as x (x)}<option value={x}>{roleLabels[x]}</option>{/each}</select
        >
        <button
          class="btn ghost icon sm"
          aria-label="Remove reserve"
          onclick={() => editing && (editing.reserves = editing.reserves.filter((_, j) => j !== i))}
          ><Trash2 size={13} /></button
        >
      </div>
    {/each}
    <button
      class="btn sm"
      onclick={() =>
        editing &&
        providers[0] &&
        (editing.reserves = [...editing.reserves, { provider: providers[0], fraction: 0.3, for_roles: ['reviewer'] }])}
      ><Plus size={13} /> Add reserve</button
    >
    <p class="hint">Remaining capacity is estimated unless a provider reports it.</p>

    <div class="setting tight">
      <div class="text">
        <strong>Automatic fallback</strong>
        <p>Continue with another member if a provider is unavailable.</p>
      </div>
      <Toggle
        checked={editing.fallback}
        label="Automatic fallback"
        onchange={(v) => editing && (editing.fallback = v)}
      />
    </div>
    <div class="field">
      <label class="label" for="cb-instr">Combo instructions</label>
      <textarea
        id="cb-instr"
        class="textarea"
        rows="2"
        bind:value={editing.instructions}
        placeholder="Shared guidance for every member of this Combo"
      ></textarea>
    </div>
    {#snippet footer()}
      <button class="btn ghost" onclick={() => (editing = null)}>Cancel</button>
      <button class="btn primary" onclick={save}>Save Combo</button>
    {/snippet}
  </Modal>
{/if}

{#if importing}
  <Modal title="Import Combo" onclose={() => (importing = false)}>
    <textarea
      class="textarea mono"
      rows="10"
      bind:value={importText}
      placeholder="Paste a shared Combo (.json)"
      aria-label="Combo JSON"
    ></textarea>
    <p class="hint">Models you don't have are kept but disabled. Combos never contain secrets.</p>
    {#snippet footer()}
      <button class="btn ghost" onclick={() => (importing = false)}>Cancel</button>
      <button class="btn primary" onclick={imp} disabled={!importText.trim()}>Import</button>
    {/snippet}
  </Modal>
{/if}

{#if exported}
  <Modal title="Share Combo" onclose={() => (exported = null)}>
    <textarea class="textarea mono" rows="12" readonly value={exported} aria-label="Exported Combo"></textarea>
    {#snippet footer()}
      <button
        class="btn primary"
        onclick={() => {
          void navigator.clipboard?.writeText(exported ?? '');
          toast('Copied');
        }}>Copy</button
      >
    {/snippet}
  </Modal>
{/if}

<style>
  .toolbar {
    margin-bottom: var(--s4);
  }
  .combos {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
    gap: var(--s3);
  }
  .combo {
    display: flex;
    flex-direction: column;
    gap: var(--s2);
    padding: var(--s3) var(--s4);
  }
  .combo :global(.ic) {
    color: var(--accent);
  }
  .on {
    color: var(--warning);
  }
  .desc {
    min-height: 2.6em;
  }
  .members li {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 2px 0;
    flex-wrap: wrap;
  }
  .members li.off {
    opacity: 0.45;
  }
  .role {
    font-size: 10px;
    padding: 1px 6px;
    border-radius: 999px;
    background: var(--bg-sunken);
    color: var(--text-muted);
  }
  .actions {
    margin-top: auto;
  }
  .members-edit td {
    vertical-align: top;
  }
  .members-edit .select {
    min-width: 0;
  }
  .roles-cell {
    display: flex;
    flex-wrap: wrap;
    gap: 3px;
    max-width: 300px;
  }
  .rchip {
    font-size: 11px;
    height: 22px;
    padding: 0 7px;
    border-radius: 999px;
    border: 1px solid var(--border);
    background: var(--bg);
    color: var(--text-muted);
    cursor: pointer;
    font-family: inherit;
  }
  .rchip[aria-pressed='true'] {
    background: var(--accent-soft);
    border-color: var(--accent);
    color: var(--accent);
  }
  .add-m {
    margin-top: var(--s2);
  }
  .grid2 {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0 var(--s4);
    margin-top: var(--s4);
  }
  .reserve {
    margin-bottom: var(--s2);
  }
  .pct {
    width: 70px;
  }
  .prov {
    width: auto;
  }
  .tight {
    margin: var(--s3) 0;
  }
</style>
