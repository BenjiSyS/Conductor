<script lang="ts">
  import { Smartphone, Trash2 } from '@lucide/svelte';
  import { app, attempt, savePrefs } from '../../lib/app.svelte';
  import { call } from '../../lib/api';
  import { ago } from '../../lib/format';
  import type { HostStatus } from '../../lib/types';
  import Toggle from '../Toggle.svelte';

  let st = $state<HostStatus | null>(null);
  let code = $state<{ code: string; expires_at: number; fingerprint: string; addr: string } | null>(null);
  let control = $state(false);
  let scope = $state<string[]>(app.projectId ? [app.projectId] : []);
  let busy = $state(false);

  const load = async () => (st = await call<HostStatus>('remote_status').catch(() => null));
  $effect(() => {
    void load();
    const t = setInterval(load, 4000);
    return () => clearInterval(t);
  });

  async function toggle(on: boolean) {
    busy = true;
    if (on) await attempt(() => call('remote_start'));
    else await attempt(() => call('remote_stop'));
    code = null;
    busy = false;
    await load();
  }
  async function pair() {
    if (!scope.length) return;
    code =
      (await attempt(() => call<typeof code>('remote_pair_code', { projects: scope, canControl: control }))) ?? null;
  }
</script>

<h2>Remote access</h2>
<p class="lede">
  Reach this computer's projects from another device — a direct, encrypted connection. No Conductor account, no cloud
  relay.
</p>

<div class="setting">
  <div class="text">
    <strong>Conductor Host</strong>
    <p>{st?.running ? `Listening on ${st.addr} · ${st.connected} connected` : 'Off. Turn on to pair a device.'}</p>
  </div>
  <Toggle checked={!!st?.running} label="Conductor Host" disabled={busy} onchange={toggle} />
</div>

<div class="setting">
  <div class="text">
    <strong>Network</strong>
    <p>“This computer only” is safest. Choose “Local network” to connect from other devices on your Wi-Fi.</p>
  </div>
  <select
    class="select"
    value={app.prefs?.remote_bind}
    disabled={st?.running}
    onchange={(e) => attempt(() => savePrefs({ remote_bind: (e.target as HTMLSelectElement).value }))}
    aria-label="Listen on"
  >
    <option value="127.0.0.1">This computer only</option>
    <option value="0.0.0.0">Local network</option>
  </select>
</div>

<div class="setting">
  <div class="text">
    <strong>Keep Goals running when a device disconnects</strong>
    <p>Approved work continues; anything needing a decision waits for you.</p>
  </div>
  <Toggle
    checked={!!app.prefs?.remote_continue_on_disconnect}
    label="Continue on disconnect"
    onchange={(v) => attempt(() => savePrefs({ remote_continue_on_disconnect: v }))}
  />
</div>

{#if st?.running}
  <h3 class="sub">Pair a device</h3>
  <div class="card pair">
    <span class="label">Projects this device may see</span>
    <div class="row wrap">
      {#each app.projects as p (p.id)}
        <label class="row small chk"
          ><input
            type="checkbox"
            checked={scope.includes(p.id)}
            onchange={(e) =>
              (scope = (e.target as HTMLInputElement).checked ? [...scope, p.id] : scope.filter((x) => x !== p.id))}
          />
          {p.name}</label
        >
      {/each}
    </div>
    <label class="row small chk"
      ><input type="checkbox" bind:checked={control} /> Allow prompts, approvals and file edits (otherwise view only)</label
    >
    <div class="row">
      <span class="spacer"></span><button class="btn primary" onclick={pair} disabled={!scope.length}
        ><Smartphone size={14} /> Create pairing code</button
      >
    </div>
    {#if code}
      <div class="codebox">
        <div class="code mono">{code.code}</div>
        <p class="small">
          On the other device open <span class="mono">https://{code.addr}</span> (or Conductor → Connect to host) and enter
          the code. Single use, expires in 5 minutes.
        </p>
        <p class="xsmall muted">Check that the certificate fingerprint matches:</p>
        <p class="xsmall mono fp">{code.fingerprint}</p>
      </div>
    {/if}
  </div>

  <h3 class="sub">Trusted devices</h3>
  {#each st.devices as d (d.id)}
    <div class="setting">
      <div class="text">
        <strong>{d.name}</strong>
        <p>
          {d.can_control ? 'Can control' : 'View only'} · paired {ago(d.created)}{d.last_seen
            ? ` · seen ${ago(d.last_seen)}`
            : ''}
        </p>
      </div>
      <button
        class="btn sm danger"
        onclick={async () => {
          await attempt(() => call('remote_revoke', { deviceId: d.id }), `${d.name} revoked`);
          await load();
        }}><Trash2 size={12} /> Revoke</button
      >
    </div>
  {:else}
    <p class="muted small">No paired devices.</p>
  {/each}
{/if}

<style>
  .pair {
    display: flex;
    flex-direction: column;
    gap: var(--s3);
  }
  .chk {
    gap: 6px;
  }
  .codebox {
    border-top: 1px solid var(--border);
    padding-top: var(--s3);
    display: grid;
    gap: var(--s2);
  }
  .code {
    font-size: 32px;
    letter-spacing: 0.12em;
    font-weight: 600;
  }
  .fp {
    word-break: break-all;
  }
</style>
