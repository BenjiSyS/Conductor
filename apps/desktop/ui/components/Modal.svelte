<script lang="ts">
  import type { Snippet } from 'svelte';
  import { X } from '@lucide/svelte';

  let {
    title,
    open = true,
    width = 560,
    onclose,
    children,
    footer,
  }: {
    title: string;
    open?: boolean;
    width?: number;
    onclose: () => void;
    children: Snippet;
    footer?: Snippet;
  } = $props();

  let dialog: HTMLDivElement | undefined = $state();
  let returnFocus: Element | null = null;

  $effect(() => {
    if (open) {
      returnFocus = document.activeElement;
      queueMicrotask(() => {
        const first = dialog?.querySelector<HTMLElement>('input, textarea, select, button:not(.close)');
        (first ?? dialog)?.focus();
      });
      return () => (returnFocus as HTMLElement | null)?.focus?.();
    }
  });

  function key(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.stopPropagation();
      onclose();
    }
    if (e.key === 'Tab' && dialog) {
      const f = [
        ...dialog.querySelectorAll<HTMLElement>(
          'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])',
        ),
      ].filter((x) => !x.hasAttribute('disabled'));
      if (!f.length) return;
      if (e.shiftKey && document.activeElement === f[0]) {
        e.preventDefault();
        f[f.length - 1].focus();
      } else if (!e.shiftKey && document.activeElement === f[f.length - 1]) {
        e.preventDefault();
        f[0].focus();
      }
    }
  }
</script>

{#if open}
  <div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
    <div
      class="modal"
      role="dialog"
      aria-modal="true"
      aria-label={title}
      tabindex="-1"
      bind:this={dialog}
      onkeydown={key}
      style:max-width="{width}px"
    >
      <header>
        <h3>{title}</h3>
        <button class="btn ghost icon sm close" onclick={onclose} aria-label="Close"><X size={16} /></button>
      </header>
      <div class="body">{@render children()}</div>
      {#if footer}<footer>{@render footer()}</footer>{/if}
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgb(10 10 14 / 38%);
    display: grid;
    place-items: center;
    z-index: 50;
    padding: var(--s5);
    animation: fade var(--duration) var(--ease);
  }
  .modal {
    width: 100%;
    max-height: calc(100vh - 64px);
    display: flex;
    flex-direction: column;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-lg);
    animation: rise var(--duration) var(--ease);
    outline: none;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--s4) var(--s5) var(--s2);
  }
  .body {
    padding: var(--s2) var(--s5) var(--s5);
    overflow: auto;
  }
  footer {
    display: flex;
    justify-content: flex-end;
    gap: var(--s2);
    padding: var(--s3) var(--s5) var(--s4);
    border-top: 1px solid var(--border);
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(6px) scale(0.99);
    }
  }
</style>
