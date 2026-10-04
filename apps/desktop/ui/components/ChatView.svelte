<script lang="ts">
  import { tick } from 'svelte';
  import { PanelLeft, GitBranch, History, Layers, Copy, Pencil, Brain, Monitor } from '@lucide/svelte';
  import { app, attempt, refresh, toast, togglePanel } from '../lib/app.svelte';
  import { call } from '../lib/api';
  import { markdown, modelLabel } from '../lib/format';

  let scroller: HTMLDivElement | undefined = $state();
  let stick = $state(true);
  let editing = $state(false);
  let title = $state('');

  const conv = $derived(app.conversation);
  const lastText = $derived(conv?.messages.at(-1)?.text.length ?? 0);

  $effect(() => {
    void conv?.messages.length;
    void lastText;
    if (stick) tick().then(() => scroller?.scrollTo({ top: scroller.scrollHeight }));
  });

  function onScroll() {
    if (!scroller) return;
    stick = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 80;
  }

  async function saveTitle() {
    editing = false;
    if (!conv || !title.trim() || title === conv.title) return;
    await attempt(() => call('rename_conversation', { id: conv.id, title }));
    await refresh();
  }

  function copy(text: string) {
    void navigator.clipboard?.writeText(text).then(() => toast('Copied'));
  }
</script>

<header class="topbar">
  {#if !app.sidebarOpen}
    <button class="btn ghost icon sm" aria-label="Show sidebar" onclick={() => (app.sidebarOpen = true)}
      ><PanelLeft size={16} /></button
    >
  {/if}
  {#if editing}
    <input
      class="input title-input"
      bind:value={title}
      onblur={saveTitle}
      onkeydown={(e) => e.key === 'Enter' && saveTitle()}
      aria-label="Conversation title"
    />
  {:else}
    <button
      class="title"
      onclick={() => {
        title = conv?.title ?? '';
        editing = true;
      }}
      title="Rename"
    >
      <span class="ellipsis">{conv?.title}</span><Pencil size={12} class="pen" />
    </button>
  {/if}
  <span class="badge">{conv?.mode}</span>
  <span class="spacer"></span>
  <button
    class="btn ghost icon sm"
    aria-label="Project memory panel"
    title="Project memory and decisions"
    onclick={() => togglePanel('memory')}><Brain size={15} /></button
  >
  <button
    class="btn ghost icon sm"
    aria-label="Context inspector panel"
    title="Context inspector"
    onclick={() => togglePanel('inspector')}><Layers size={15} /></button
  >
  <button class="btn ghost icon sm" aria-label="Preview panel" title="Preview" onclick={() => togglePanel('preview')}
    ><Monitor size={15} /></button
  >
  <button class="btn ghost icon sm" aria-label="Git panel" title="Git" onclick={() => togglePanel('git')}
    ><GitBranch size={15} /></button
  >
  <button class="btn ghost icon sm" aria-label="History panel" title="History" onclick={() => togglePanel('history')}
    ><History size={15} /></button
  >
</header>

<div class="scroller" bind:this={scroller} onscroll={onScroll}>
  <div class="thread">
    {#each conv?.messages ?? [] as m (m.id)}
      <article class="msg {m.role}" aria-label="{m.role} message">
        {#if m.role === 'user'}
          <div class="bubble">{m.text.replace(/ · paste:[0-9a-f]{32}]/g, ']')}</div>
        {:else}
          <div class="who">
            <span class="model">{m.provider ? modelLabel(m.provider) : 'Assistant'}</span>
            {#if m.status === 'streaming'}<span class="dot running" aria-label="Working"></span>{/if}
            {#if m.status === 'failed'}<span class="badge danger">failed</span>{/if}
            {#if m.status === 'stopped'}<span class="badge">stopped</span>{/if}
            {#if m.status === 'interrupted'}<span class="badge warning">interrupted</span>{/if}
            <span class="spacer"></span>
            {#if m.text && m.status !== 'streaming'}
              <button class="btn ghost icon sm copy" aria-label="Copy message" onclick={() => copy(m.text)}
                ><Copy size={13} /></button
              >
            {/if}
          </div>
          {#if m.text}
            <div class="prose">{@html markdown(m.text)}</div>
          {:else if m.status === 'streaming'}
            <div class="thinking"><span></span><span></span><span></span></div>
          {/if}
        {/if}
      </article>
    {/each}
  </div>
</div>

<style>
  .topbar {
    display: flex;
    align-items: center;
    gap: var(--s2);
    height: 52px;
    padding: 0 var(--s4);
    border-bottom: 1px solid var(--border);
    flex: none;
  }
  .title {
    display: inline-flex;
    align-items: center;
    gap: var(--s2);
    max-width: 50%;
    border: none;
    background: none;
    color: var(--text);
    font: inherit;
    font-weight: 600;
    cursor: text;
    padding: 4px 6px;
    border-radius: var(--radius-sm);
  }
  .title:hover {
    background: var(--surface-hover);
  }
  .title :global(.pen) {
    opacity: 0;
    color: var(--text-faint);
    flex: none;
  }
  .title:hover :global(.pen) {
    opacity: 1;
  }
  .title-input {
    max-width: 420px;
    height: 30px;
  }
  .ellipsis {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .scroller {
    flex: 1;
    overflow: auto;
  }
  .thread {
    max-width: var(--content-w);
    margin: 0 auto;
    padding: var(--s6) var(--s5) var(--s4);
    display: flex;
    flex-direction: column;
    gap: var(--s6);
  }
  .msg.user {
    align-self: flex-end;
    max-width: 85%;
  }
  .bubble {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg) var(--radius-lg) var(--radius-sm) var(--radius-lg);
    padding: var(--s2) var(--s4);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .who {
    display: flex;
    align-items: center;
    gap: var(--s2);
    margin-bottom: var(--s2);
    font-size: var(--fs-xs);
    color: var(--text-muted);
    font-weight: 500;
  }
  .copy {
    opacity: 0;
  }
  .msg:hover .copy,
  .copy:focus-visible {
    opacity: 1;
  }
  .thinking {
    display: flex;
    gap: 5px;
    padding: var(--s2) 0;
  }
  .thinking span {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--text-faint);
    animation: blink 1.2s infinite var(--ease);
  }
  .thinking span:nth-child(2) {
    animation-delay: 0.15s;
  }
  .thinking span:nth-child(3) {
    animation-delay: 0.3s;
  }
  @keyframes blink {
    0%,
    80%,
    100% {
      opacity: 0.25;
    }
    40% {
      opacity: 1;
    }
  }
  @media (max-width: 760px) {
    .thread {
      padding: var(--s4) var(--s3);
    }
  }
</style>
