<script lang="ts">
  import { call } from '../../lib/api';
  import Logo from '../Logo.svelte';

  let info = $state<{
    version: string;
    data_dir: string;
    portable: boolean;
    os: string;
    startup: [string, number][];
  } | null>(null);
  $effect(() => {
    call<typeof info>('app_info').then((i) => (info = i));
  });
</script>

<div class="about">
  <Logo size={56} />
  <h2>Conductor</h2>
  <p class="muted">Version {info?.version ?? '…'} · {info?.os}{info?.portable ? ' · portable' : ''}</p>
  <p class="small">Free and open source under the MIT License. Local-first: no account, no Conductor server.</p>
  <p class="small">
    <a href="https://github.com/BenjiSyS/Conductor" target="_blank" rel="noreferrer">github.com/BenjiSyS/Conductor</a>
  </p>
  {#if info?.startup.length}
    <details>
      <summary class="xsmall muted">Startup timing</summary>
      <ul class="list xsmall mono">
        {#each info.startup as [phase, ms] (phase)}<li>{phase}: {ms} ms</li>{/each}
      </ul>
    </details>
  {/if}
  <p class="xsmall faint credits">
    Includes Caveman-compatible response style (Caveman © Julius Brussee, Apache-2.0). Icons by Lucide (ISC).
  </p>
</div>

<style>
  .about {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: var(--s2);
    padding-top: var(--s7);
  }
  .credits {
    margin-top: var(--s6);
    max-width: 420px;
  }
</style>
