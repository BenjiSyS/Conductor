<script lang="ts">
  import { app, dismissToast } from '../lib/app.svelte';
  import { X } from '@lucide/svelte';
</script>

<div class="toasts" role="status" aria-live="polite">
  {#each app.toasts as t (t.id)}
    <div class="toast {t.kind}">
      <span class="grow">{t.text}</span>
      {#if t.action}
        <button
          class="btn sm"
          onclick={() => {
            t.action?.run();
            dismissToast(t.id);
          }}>{t.action.label}</button
        >
      {/if}
      <button class="btn ghost icon sm" aria-label="Dismiss" onclick={() => dismissToast(t.id)}><X size={14} /></button>
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    right: var(--s4);
    top: 60px;
    display: flex;
    flex-direction: column;
    gap: var(--s2);
    z-index: 80;
    max-width: min(420px, calc(100vw - 32px));
  }
  .toast {
    display: flex;
    align-items: center;
    gap: var(--s2);
    padding: var(--s2) var(--s2) var(--s2) var(--s4);
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
    font-size: var(--fs-sm);
    animation: in var(--duration) var(--ease);
  }
  .toast.error {
    border-color: color-mix(in srgb, var(--danger) 45%, var(--border));
  }
  .toast.error span {
    color: var(--danger);
  }
  .toast.success span::before {
    content: '✓ ';
    color: var(--success);
  }
  @keyframes in {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }
</style>
