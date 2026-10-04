<script lang="ts">
  import { RefreshCw, ExternalLink, MessageSquarePlus, Share2, X } from '@lucide/svelte';
  import { app, attempt, toast } from '../lib/app.svelte';
  import { call } from '../lib/api';
  import Drawer from './Drawer.svelte';

  interface Port {
    port: number;
    address: string;
    pid: number | null;
    dev: boolean;
  }
  let ports = $state<Port[]>([]);
  let url = $state('');
  let input = $state('');
  let frame: HTMLIFrameElement | undefined = $state();
  let width = $state<'fill' | 1280 | 768 | 390>('fill');
  let commenting = $state(false);
  let drag = $state<{ x0: number; y0: number; x1: number; y1: number } | null>(null);
  let region = $state<{ x: number; y: number; w: number; h: number; vw: number; vh: number } | null>(null);
  let comment = $state('');
  let tunnels = $state<{ available: boolean; tunnels: { id: string; port: number; url: string | null }[] }>({
    available: false,
    tunnels: [],
  });
  let sharing = $state(false);

  async function load() {
    ports = (await call<Port[]>('ports').catch(() => [])).filter((p) => p.dev);
    tunnels = await call<typeof tunnels>('tunnels_list').catch(() => ({ available: false, tunnels: [] }));
    if (!url && ports[0]) open(`http://localhost:${ports[0].port}`);
  }
  $effect(() => {
    void load();
  });

  function open(u: string) {
    const v = u.trim();
    // Previews are limited to local dev servers (also enforced by the CSP).
    if (!/^http:\/\/(localhost|127\.0\.0\.1)(:\d+)?(\/.*)?$/.test(v)) {
      toast('Previews show local dev servers only, e.g. http://localhost:5173');
      return;
    }
    url = v;
    input = v;
  }
  const portOf = (u: string) => Number(new URL(u).port || 80);

  function down(e: PointerEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    drag = { x0: e.clientX - r.left, y0: e.clientY - r.top, x1: e.clientX - r.left, y1: e.clientY - r.top };
    region = null;
  }
  function move(e: PointerEvent) {
    if (!drag) return;
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    drag = { ...drag, x1: e.clientX - r.left, y1: e.clientY - r.top };
  }
  function up(e: PointerEvent) {
    if (!drag) return;
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const x = Math.min(drag.x0, drag.x1);
    const y = Math.min(drag.y0, drag.y1);
    const w = Math.max(8, Math.abs(drag.x1 - drag.x0));
    const h = Math.max(8, Math.abs(drag.y1 - drag.y0));
    region = {
      x: Math.round(x),
      y: Math.round(y),
      w: Math.round(w),
      h: Math.round(h),
      vw: Math.round(r.width),
      vh: Math.round(r.height),
    };
    drag = null;
  }
  function addFeedback() {
    if (!region || !comment.trim()) return;
    const pct = (n: number, d: number) => `${Math.round((n / d) * 100)}%`;
    const where = `region x=${region.x}, y=${region.y}, ${region.w}×${region.h}px of a ${region.vw}×${region.vh} viewport (left ${pct(region.x, region.vw)}, top ${pct(region.y, region.vh)})`;
    app.draft = `${app.draft ? app.draft + '\n\n' : ''}[Preview feedback on ${url} — ${where}]: ${comment.trim()}`;
    if (app.mode === 'chat' || app.mode === 'plan') app.mode = 'agent';
    comment = '';
    region = null;
    commenting = false;
    toast('Added to your prompt');
  }
  async function share() {
    sharing = true;
    const t = await attempt(() => call<{ id: string; url: string | null }>('tunnel_open', { port: portOf(url) }));
    sharing = false;
    if (t?.url) toast(`Shared at ${t.url} — close it when you're done`, 'success');
    await load();
  }
</script>

<Drawer title="Preview" onclose={() => (app.previewOpen = false)} width={620}>
  {#snippet actions()}
    <button class="btn ghost icon sm" aria-label="Reload preview" onclick={() => frame && (frame.src = url)}
      ><RefreshCw size={14} /></button
    >
  {/snippet}
  <div class="row bar">
    <input
      class="input"
      bind:value={input}
      placeholder="http://localhost:5173"
      aria-label="Preview URL"
      onkeydown={(e) => e.key === 'Enter' && open(input)}
    />
    <button class="btn" onclick={() => open(input)}>Go</button>
  </div>
  {#if ports.length}
    <div class="row wrap ports">
      {#each ports as p (p.port)}
        <button
          class="btn sm"
          class:primary={url && portOf(url) === p.port}
          onclick={() => open(`http://localhost:${p.port}`)}>:{p.port}</button
        >
      {/each}
    </div>
  {/if}
  {#if url}
    <div class="row tools">
      <div class="segmented" role="group" aria-label="Viewport">
        {#each [['fill', 'Fit'], [1280, 'Desktop'], [768, 'Tablet'], [390, 'Phone']] as const as [w, l] (w)}
          <button aria-pressed={width === w} onclick={() => (width = w)}>{l}</button>
        {/each}
      </div>
      <span class="spacer"></span>
      <button
        class="btn sm"
        aria-pressed={commenting}
        class:primary={commenting}
        onclick={() => {
          commenting = !commenting;
          region = null;
        }}><MessageSquarePlus size={13} /> Comment</button
      >
      {#if tunnels.available}
        <button class="btn sm" onclick={share} disabled={sharing}
          ><Share2 size={13} /> {sharing ? 'Sharing…' : 'Share'}</button
        >
      {/if}
      <a class="btn sm ghost icon" href={url} target="_blank" rel="noreferrer" aria-label="Open in browser"
        ><ExternalLink size={13} /></a
      >
    </div>
    <div class="viewport" style:max-width={width === 'fill' ? '100%' : `${width}px`}>
      <iframe
        bind:this={frame}
        src={url}
        title="Preview of {url}"
        sandbox="allow-scripts allow-forms allow-same-origin allow-popups"
      ></iframe>
      {#if commenting}
        <div class="overlay" role="presentation" onpointerdown={down} onpointermove={move} onpointerup={up}>
          {#if drag}<div
              class="sel"
              style:left="{Math.min(drag.x0, drag.x1)}px"
              style:top="{Math.min(drag.y0, drag.y1)}px"
              style:width="{Math.abs(drag.x1 - drag.x0)}px"
              style:height="{Math.abs(drag.y1 - drag.y0)}px"
            ></div>{/if}
          {#if region}<div
              class="sel fixed"
              style:left="{region.x}px"
              style:top="{region.y}px"
              style:width="{region.w}px"
              style:height="{region.h}px"
            ></div>{/if}
          {#if !drag && !region}<div class="hint-pill">Drag over the part you want to change</div>{/if}
        </div>
      {/if}
    </div>
    {#if region}
      <div class="feedback card">
        <textarea
          class="textarea"
          rows="2"
          bind:value={comment}
          placeholder="What should change here? e.g. “make this button primary and add more spacing”"
          aria-label="Preview comment"
        ></textarea>
        <div class="row">
          <span class="spacer"></span><button class="btn ghost sm" onclick={() => (region = null)}
            ><X size={12} /> Cancel</button
          ><button class="btn primary sm" onclick={addFeedback} disabled={!comment.trim()}>Add to prompt</button>
        </div>
      </div>
    {/if}
    {#each tunnels.tunnels as t (t.id)}
      <div class="row xsmall tunnel">
        <span class="grow">Shared :{t.port} → {t.url ?? 'starting…'}</span>
        <button
          class="btn sm ghost"
          onclick={async () => {
            await call('tunnel_close', { id: t.id });
            await load();
          }}>Stop sharing</button
        >
      </div>
    {/each}
  {:else}
    <p class="muted small empty-msg">
      Start your project's dev server (for example <span class="mono">npm run dev</span>) and it appears here. Previews
      show local servers only.
    </p>
  {/if}
</Drawer>

<style>
  .bar,
  .ports,
  .tools {
    margin-bottom: var(--s2);
  }
  .viewport {
    position: relative;
    margin: 0 auto;
    height: 60vh;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    overflow: hidden;
    background: #fff;
  }
  iframe {
    width: 100%;
    height: 100%;
    border: 0;
  }
  .overlay {
    position: absolute;
    inset: 0;
    cursor: crosshair;
    background: rgb(75 92 240 / 6%);
  }
  .sel {
    position: absolute;
    border: 2px solid var(--accent);
    background: rgb(75 92 240 / 12%);
    border-radius: 4px;
  }
  .hint-pill {
    position: absolute;
    left: 50%;
    top: 12px;
    transform: translateX(-50%);
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 4px 12px;
    font-size: var(--fs-xs);
    box-shadow: var(--shadow);
  }
  .feedback {
    margin-top: var(--s2);
    padding: var(--s2);
    display: grid;
    gap: var(--s2);
  }
  .tunnel {
    margin-top: var(--s2);
  }
  .empty-msg {
    margin-top: var(--s4);
  }
</style>
