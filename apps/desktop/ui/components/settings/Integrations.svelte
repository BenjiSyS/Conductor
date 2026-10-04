<script lang="ts">
  import { Search, Plus, Stethoscope, Trash2, Download, FolderOpen } from '@lucide/svelte';
  import { attempt, toast } from '../../lib/app.svelte';
  import { call, chooseFolder } from '../../lib/api';
  import type { CatalogEntry, Diagnosis, McpServer, SkillInfo } from '../../lib/types';
  import Toggle from '../Toggle.svelte';
  import Modal from '../Modal.svelte';

  let tab = $state<'mcp' | 'skills' | 'plugins'>('mcp');
  let q = $state('');
  let catalog = $state<CatalogEntry[]>([]);
  let servers = $state<McpServer[]>([]);
  let diag = $state<Record<string, Diagnosis>>({});
  let values = $state<Record<string, Record<string, string>>>({});
  let busy = $state<string | null>(null);
  let secretFor = $state<string | null>(null);
  let secretValue = $state('');
  let exported = $state<string | null>(null);
  let skills = $state<SkillInfo[]>([]);
  let plugins = $state<
    {
      manifest: { name: string; version: string; description: string; permissions: string[] };
      health: string;
      detail: string;
    }[]
  >([]);
  let source = $state('');
  let remoteName = $state('');
  let remoteUrl = $state('');

  // Remote connectors: add by URL; Conductor opens the service's sign-in page
  // when it needs one (no app registration or tokens to copy).
  async function connectRemote() {
    const url = remoteUrl.trim();
    const name =
      remoteName.trim() ||
      (() => {
        try {
          return new URL(url).hostname.replace(/^(www|mcp|api)./, '').split('.')[0];
        } catch {
          return '';
        }
      })();
    if (!url || !name) return;
    busy = 'remote';
    toast('Finish signing in in your browser if it opens…');
    const d = await attempt(() =>
      call<Diagnosis>('mcp_connect_remote', { name: name.replace(/[^A-Za-z0-9_-]/g, '-'), url }),
    );
    busy = null;
    if (d) {
      diag = { ...diag, [d.server]: d };
      toast(
        d.status === 'connected' ? `${d.server} connected` : `${d.server}: ${d.summary}`,
        d.status === 'connected' ? 'success' : 'info',
      );
      remoteName = '';
      remoteUrl = '';
      await loadMcp();
    }
  }
  async function signIn(name: string) {
    busy = name;
    const d = await attempt(() => call<Diagnosis>('mcp_sign_in', { name }));
    busy = null;
    if (d) diag = { ...diag, [d.server]: d };
  }

  async function loadMcp() {
    catalog = await call<CatalogEntry[]>('mcp_catalog', { query: q || null });
    servers = (await call<{ servers: McpServer[] }>('mcp_list')).servers;
  }
  async function loadPkgs() {
    skills = await call<SkillInfo[]>('skills_list').catch(() => []);
    plugins = await call<typeof plugins>('plugins_list').catch(() => []);
  }
  $effect(() => {
    void q;
    const t = setTimeout(loadMcp, 200);
    return () => clearTimeout(t);
  });
  $effect(() => {
    void loadPkgs();
  });

  async function add(e: CatalogEntry) {
    busy = e.id;
    const d = await attempt(() => call<Diagnosis>('mcp_add', { id: e.id, values: values[e.id] ?? {} }));
    busy = null;
    if (d) {
      diag = { ...diag, [d.server]: d };
      toast(
        d.status === 'connected' ? `${e.name} installed and connected` : `${e.name} added — ${d.summary}`,
        d.status === 'connected' ? 'success' : 'info',
      );
      await loadMcp();
    }
  }
  async function doctor() {
    busy = 'doctor';
    const r = await attempt(() => call<Diagnosis[]>('mcp_doctor', { name: null }));
    busy = null;
    if (r) diag = Object.fromEntries(r.map((d) => [d.server, d]));
  }
  async function saveSecret() {
    if (!secretFor) return;
    await attempt(() => call('secret_set', { name: secretFor, value: secretValue }), 'Saved to the OS keychain');
    secretFor = null;
    secretValue = '';
    void doctor();
  }
  async function installPkg(kind: 'skills' | 'plugins') {
    const src = source.trim();
    if (!src) return;
    busy = 'pkg';
    const r = await attempt(() =>
      call<{ name: string; version: string }>(kind === 'skills' ? 'skills_install' : 'plugins_install', {
        source: src,
        rev: null,
      }),
    );
    busy = null;
    if (r) {
      toast(`Installed ${r.name} ${r.version}`, 'success');
      source = '';
      await loadPkgs();
    }
  }
  async function browse() {
    const p = await chooseFolder('Choose a skill or plugin folder');
    if (p) source = p;
  }
  const statusClass = (s: string) =>
    s === 'connected'
      ? 'ok'
      : s === 'disabled' || s === 'not_checked'
        ? ''
        : s === 'missing_dependency' || s === 'authentication_required'
          ? 'warn'
          : 'bad';
</script>

<h2>MCP, skills & plugins</h2>
<p class="lede">Install once — Conductor exposes them to every compatible provider.</p>

<div class="segmented tabs" role="tablist">
  <button aria-pressed={tab === 'mcp'} onclick={() => (tab = 'mcp')}>MCP servers</button>
  <button aria-pressed={tab === 'skills'} onclick={() => (tab = 'skills')}>Skills</button>
  <button aria-pressed={tab === 'plugins'} onclick={() => (tab = 'plugins')}>Plugins</button>
</div>

{#if tab === 'mcp'}
  <div class="row head">
    <h3 class="sub inline">Installed</h3>
    <span class="spacer"></span>
    <button class="btn sm" onclick={doctor} disabled={!servers.length || busy === 'doctor'}
      ><Stethoscope size={13} /> {busy === 'doctor' ? 'Checking…' : 'Run MCP Doctor'}</button
    >
    <select
      class="select exp"
      onchange={async (e) => {
        const v = (e.target as HTMLSelectElement).value;
        (e.target as HTMLSelectElement).value = '';
        if (v) exported = await call<string>('mcp_export', { target: v });
      }}
      aria-label="Export MCP config"
    >
      <option value="">Export for…</option><option value="claude">Claude</option><option value="codex">Codex</option
      ><option value="gemini">Gemini CLI</option><option value="grok">Grok Build</option>
    </select>
  </div>
  {#each servers as s (s.name)}
    {@const d = diag[s.name]}
    <div class="srv card">
      <div class="row">
        <span class="dot {d ? statusClass(d.status) : ''}"></span>
        <strong class="small">{s.name}</strong>
        <span class="xsmall faint grow">{d ? d.summary : s.source}</span>
        {#if s.transport.type === 'http'}
          <button class="btn sm ghost" onclick={() => signIn(s.name)} disabled={busy !== null}>Sign in</button>
        {/if}
        <Toggle
          checked={s.enabled}
          label="Enable {s.name}"
          onchange={async (v) => {
            await call('mcp_set_enabled', { name: s.name, enabled: v });
            await loadMcp();
          }}
        />
        <button
          class="btn ghost icon sm"
          aria-label="Remove {s.name}"
          onclick={async () => {
            await attempt(() => call('mcp_remove', { name: s.name }));
            await loadMcp();
          }}><Trash2 size={13} /></button
        >
      </div>
      {#if d && d.fix.kind !== 'none'}
        <div class="fix small">
          {#if d.fix.kind === 'install_dependency'}Fix: {d.fix.hint}
          {:else if d.fix.kind === 'set_secret'}Needs a credential. <button
              class="btn sm"
              onclick={() => (secretFor = d.fix.name ?? null)}>Add {d.fix.name}</button
            >
          {:else if d.fix.kind === 'reconnect'}<button class="btn sm" onclick={doctor}>Reconnect</button>
          {:else if d.fix.kind === 'enable'}Enable it to use it.
          {:else if d.fix.kind === 'edit_config'}{d.fix.hint}{/if}
          {#if d.detail}<details>
              <summary class="xsmall muted">Details</summary>
              <pre class="detail">{d.detail}</pre>
            </details>{/if}
        </div>
      {/if}
    </div>
  {:else}
    <p class="muted small">No MCP servers yet.</p>
  {/each}

  <h3 class="sub">Connect a remote connector</h3>
  <p class="xsmall muted">
    Paste the connector's URL (for example from GitHub, Linear, Notion or Sentry). If it needs an account, its sign-in
    page opens — no keys to copy.
  </p>
  <div class="row remote">
    <input class="input" bind:value={remoteUrl} placeholder="https://mcp.example.com/mcp" aria-label="Connector URL" />
    <input class="input name" bind:value={remoteName} placeholder="Name (optional)" aria-label="Connector name" />
    <button class="btn primary" onclick={connectRemote} disabled={!remoteUrl.trim() || busy !== null}
      >{busy === 'remote' ? 'Connecting…' : 'Connect'}</button
    >
  </div>

  <h3 class="sub">Add an MCP server</h3>
  <div class="searchbox">
    <Search size={14} />
    <input
      class="input"
      bind:value={q}
      placeholder="Try “Set up Blender MCP” or “GitHub”"
      aria-label="Search MCP catalog"
    />
  </div>
  {#each catalog as e (e.id)}
    <div class="cat">
      <div class="grow">
        <strong class="small">{e.name}</strong>
        <p class="xsmall muted">{e.description}</p>
        <p class="xsmall faint">
          Source: <a href={e.source} target="_blank" rel="noreferrer">{e.source.replace('https://', '')}</a> · needs {e.dependencies.join(
            ', ',
          )}
        </p>
        {#each e.needs as n (n)}
          <input
            class="input need"
            placeholder={n === 'path' ? 'Folder the server may access' : n}
            aria-label="{e.name} {n}"
            value={values[e.id]?.[n] ?? ''}
            oninput={(ev) =>
              (values = { ...values, [e.id]: { ...(values[e.id] ?? {}), [n]: (ev.target as HTMLInputElement).value } })}
          />
        {/each}
      </div>
      <button class="btn sm" onclick={() => add(e)} disabled={busy === e.id || servers.some((s) => s.name === e.id)}>
        {#if busy === e.id}<span class="spinner"></span> Installing…{:else if servers.some((s) => s.name === e.id)}Installed{:else}<Plus
            size={13}
          /> Add{/if}
      </button>
    </div>
  {:else}
    <p class="muted small">Nothing in the catalog matches.</p>
  {/each}
  <p class="xsmall faint">
    Packages run from their official sources via npx, uvx or Docker. Conductor tests each server after adding it.
  </p>
{:else}
  <div class="row install">
    <input class="input" bind:value={source} placeholder="https://github.com/… or a local folder" aria-label="Source" />
    <button class="btn" onclick={browse}><FolderOpen size={14} /></button>
    <button
      class="btn primary"
      onclick={() => installPkg(tab as 'skills' | 'plugins')}
      disabled={!source.trim() || busy === 'pkg'}>{busy === 'pkg' ? 'Installing…' : 'Install'}</button
    >
  </div>
  <p class="xsmall faint">
    Only HTTPS Git URLs or local folders. Packages are checked (manifest, permissions, no symlinks), hashed, and the
    previous version is kept for rollback.
  </p>
  {#if tab === 'skills'}
    {#each skills as s (s.manifest.name)}
      <div class="srv card row">
        <div class="grow">
          <strong class="small">{s.manifest.name}</strong> <span class="xsmall faint">{s.manifest.version}</span>
          <p class="xsmall muted">{s.manifest.description}</p>
          {#if s.sensitive.length}<p class="xsmall warn">Can: {s.sensitive.join(', ')}</p>{/if}
        </div>
        <button
          class="btn ghost icon sm"
          aria-label="Remove {s.manifest.name}"
          onclick={async () => {
            await attempt(() => call('skills_remove', { name: s.manifest.name }));
            await loadPkgs();
          }}><Trash2 size={13} /></button
        >
      </div>
    {:else}
      <p class="muted small">No skills installed. Skills in the common SKILL.md format work too.</p>
    {/each}
  {:else}
    {#each plugins as p (p.manifest.name)}
      <div class="srv card row">
        <div class="grow">
          <strong class="small">{p.manifest.name}</strong>
          <span class="xsmall faint">{p.manifest.version} · {p.health}</span>
          <p class="xsmall muted">{p.manifest.description}</p>
          <p class="xsmall faint">Permissions: {p.manifest.permissions.join(', ')}</p>
        </div>
        <button
          class="btn ghost icon sm"
          aria-label="Remove {p.manifest.name}"
          onclick={async () => {
            await attempt(() => call('plugins_remove', { name: p.manifest.name }));
            await loadPkgs();
          }}><Trash2 size={13} /></button
        >
      </div>
    {:else}
      <p class="muted small">
        No plugins installed. Plugins are declarative: they add tools that run under your permission policy.
      </p>
    {/each}
  {/if}
{/if}

{#if secretFor}
  <Modal title="Add credential" onclose={() => (secretFor = null)} width={440}>
    <label class="label" for="sec-v">{secretFor}</label>
    <input id="sec-v" class="input" type="password" bind:value={secretValue} autocomplete="off" />
    <p class="hint">
      Stored in your OS credential store and injected only into this server's process. Never sent to a model.
    </p>
    {#snippet footer()}
      <button class="btn ghost" onclick={() => (secretFor = null)}>Cancel</button>
      <button class="btn primary" onclick={saveSecret} disabled={!secretValue}>Save</button>
    {/snippet}
  </Modal>
{/if}

{#if exported}
  <Modal title="MCP configuration" onclose={() => (exported = null)}>
    <textarea class="textarea mono" rows="12" readonly value={exported} aria-label="Exported configuration"></textarea>
    <p class="hint">Secrets appear as ${'{'}placeholders{'}'} — never as values.</p>
    {#snippet footer()}<button
        class="btn primary"
        onclick={() => {
          void navigator.clipboard?.writeText(exported ?? '');
          toast('Copied');
        }}><Download size={13} /> Copy</button
      >{/snippet}
  </Modal>
{/if}

<style>
  .remote {
    gap: var(--s2);
    margin-bottom: var(--s3);
  }
  .remote .name {
    max-width: 170px;
  }
  .tabs {
    margin-bottom: var(--s4);
  }
  .head {
    margin-bottom: var(--s2);
  }
  .inline {
    margin: 0 !important;
  }
  .exp {
    width: auto !important;
    height: 26px;
    font-size: var(--fs-xs);
  }
  .srv {
    padding: var(--s2) var(--s3);
    margin-bottom: var(--s2);
  }
  .fix {
    margin: var(--s2) 0 0 15px;
    color: var(--text-muted);
  }
  .detail {
    font-size: 11px;
    white-space: pre-wrap;
    max-height: 160px;
    overflow: auto;
  }
  .searchbox {
    position: relative;
    margin-bottom: var(--s3);
  }
  .searchbox :global(svg) {
    position: absolute;
    left: 10px;
    top: 10px;
    color: var(--text-faint);
  }
  .searchbox .input {
    padding-left: 32px;
  }
  .cat {
    display: flex;
    gap: var(--s3);
    align-items: flex-start;
    padding: var(--s3) 0;
    border-bottom: 1px solid var(--border);
  }
  .need {
    margin-top: var(--s2);
    max-width: 360px;
  }
  .install {
    margin-bottom: var(--s1);
  }
  .warn {
    color: var(--warning);
  }
</style>
