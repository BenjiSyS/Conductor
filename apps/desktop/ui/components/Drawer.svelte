<script lang="ts">
  import type { Snippet } from 'svelte';
  import { X } from '@lucide/svelte';

  let {
    title,
    onclose,
    children,
    actions,
    width = 440,
  }: { title: string; onclose: () => void; children: Snippet; actions?: Snippet; width?: number } = $props();
</script>

<aside class="drawer" aria-label={title} style:width="{width}px">
  <header>
    <h3>{title}</h3>
    <div class="row">
      {#if actions}{@render actions()}{/if}
      <button class="btn ghost icon sm" onclick={onclose} aria-label="Close panel"><X size={16} /></button>
    </div>
  </header>
  <div class="content">{@render children()}</div>
</aside>

<style>
  .drawer {
    flex: none;
    max-width: 50vw;
    height: 100%;
    border-left: 1px solid var(--border);
    background: var(--bg-elevated);
    display: flex;
    flex-direction: column;
    animation: slide var(--duration) var(--ease);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 52px;
    padding: 0 var(--s3) 0 var(--s4);
    border-bottom: 1px solid var(--border);
  }
  .content {
    flex: 1;
    overflow: auto;
    padding: var(--s4);
  }
  @keyframes slide {
    from {
      transform: translateX(12px);
      opacity: 0;
    }
  }
  @media (max-width: 900px) {
    .drawer {
      position: fixed;
      right: 0;
      top: 0;
      bottom: 0;
      max-width: 92vw;
      z-index: 40;
      box-shadow: var(--shadow-lg);
    }
  }
</style>
