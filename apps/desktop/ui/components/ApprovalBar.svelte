<script lang="ts">
  import { ShieldQuestion, TriangleAlert } from '@lucide/svelte';
  import { app, attempt } from '../lib/app.svelte';
  import { call } from '../lib/api';

  const labels: Record<string, string> = {
    'filesystem.write': 'Edit files',
    'filesystem.delete': 'Delete a file',
    'terminal.execute': 'Run a command',
    'git.write': 'Change Git history',
    network: 'Use the network',
    'install.software': 'Install software',
    'computer.control': 'Control the computer',
    browser: 'Use the browser',
    'remote.host': 'Remote access',
  };

  async function resolve(id: string, allow: boolean) {
    await attempt(() => call('approval_resolve', { id, allow }));
    app.approvals = app.approvals.filter((a) => a.id !== id);
  }
</script>

{#each app.approvals.slice(0, 3) as a (a.id)}
  <div class="approval card" class:danger={a.risk === 'dangerous'} role="alertdialog" aria-label="Permission request">
    <div class="ico">
      {#if a.risk === 'dangerous'}<TriangleAlert size={18} />{:else}<ShieldQuestion size={18} />{/if}
    </div>
    <div class="grow body">
      <div class="title">
        {labels[a.capability] ?? a.capability}?
        {#if a.risk === 'dangerous'}<span class="badge danger">needs your approval even with Full Access</span>{/if}
      </div>
      <pre class="detail">{a.detail}</pre>
    </div>
    <div class="col actions">
      <button class="btn primary sm" onclick={() => resolve(a.id, true)}>Allow</button>
      <button class="btn sm" onclick={() => resolve(a.id, false)}>Deny</button>
    </div>
  </div>
{/each}
{#if app.approvals.length > 3}
  <p class="more xsmall muted">+{app.approvals.length - 3} more waiting</p>
{/if}

<style>
  .approval {
    max-width: var(--content-w);
    margin: 0 auto var(--s2);
    display: flex;
    gap: var(--s3);
    align-items: flex-start;
    padding: var(--s3);
    box-shadow: var(--shadow);
    border-color: color-mix(in srgb, var(--accent) 40%, var(--border));
    animation: rise var(--duration) var(--ease);
  }
  .approval.danger {
    border-color: color-mix(in srgb, var(--danger) 55%, var(--border));
  }
  .ico {
    color: var(--accent);
    padding-top: 2px;
  }
  .danger .ico {
    color: var(--danger);
  }
  .title {
    font-weight: 600;
    display: flex;
    align-items: center;
    gap: var(--s2);
    flex-wrap: wrap;
  }
  .detail {
    margin: var(--s1) 0 0;
    font-size: var(--fs-xs);
    color: var(--text-muted);
    white-space: pre-wrap;
    max-height: 120px;
    overflow: auto;
  }
  .actions {
    gap: var(--s1);
  }
  .more {
    max-width: var(--content-w);
    margin: 0 auto var(--s2);
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }
</style>
