<script lang="ts">
  import { ChevronDown, Star, Layers, Search, Check } from '@lucide/svelte';
  import { app, savePrefs } from '../lib/app.svelte';
  import { modelLabel } from '../lib/format';

  let open = $state(false);
  let q = $state('');
  let input: HTMLInputElement | undefined = $state();

  const label = $derived.by(() => {
    const t = app.target;
    if (!t) return 'Choose a model';
    if (t.startsWith('combo:')) return app.combos.find((c) => `combo:${c.id}` === t)?.name ?? 'Combo';
    const m = app.models.find((x) => `model:${x.provider}/${x.model}` === t);
    return m ? (m.display.split(' · ').pop() ?? m.model) : modelLabel(t);
  });

  const filter = (s: string) => !q.trim() || s.toLowerCase().includes(q.trim().toLowerCase());
  const combos = $derived(app.combos.filter((c) => c.members.some((m) => m.enabled) && filter(c.name)));
  const groups = $derived.by(() => {
    const g = new Map<string, typeof app.models>();
    for (const m of app.models) {
      if (!filter(m.display)) continue;
      const k = m.display.split(' · ')[0];
      g.set(k, [...(g.get(k) ?? []), m]);
    }
    return [...g.entries()];
  });
  const favorites = $derived(
    (app.prefs?.favorites ?? []).filter(
      (f) =>
        app.models.some((m) => `model:${m.provider}/${m.model}` === f) || app.combos.some((c) => `combo:${c.id}` === f),
    ),
  );

  function pick(t: string) {
    app.target = t;
    open = false;
    q = '';
  }
  function fav(t: string, e: Event) {
    e.stopPropagation();
    const f = app.prefs?.favorites ?? [];
    void savePrefs({ favorites: f.includes(t) ? f.filter((x) => x !== t) : [...f, t] });
  }
  function nameOf(t: string) {
    if (t.startsWith('combo:')) return app.combos.find((c) => `combo:${c.id}` === t)?.name ?? t;
    return app.models.find((m) => `model:${m.provider}/${m.model}` === t)?.display ?? t;
  }
  function firstMatch(): string | null {
    if (combos[0]) return `combo:${combos[0].id}`;
    const m = groups[0]?.[1][0];
    return m ? `model:${m.provider}/${m.model}` : null;
  }
  $effect(() => {
    if (open) queueMicrotask(() => input?.focus());
  });
</script>

<div class="picker">
  <button
    class="btn ghost sm trigger"
    aria-haspopup="listbox"
    aria-expanded={open}
    onclick={() => (open = !open)}
    title="Model or Combo"
  >
    {#if app.target?.startsWith('combo:')}<Layers size={13} />{/if}
    <span class="lbl">{label}</span>
    <ChevronDown size={12} />
  </button>
  {#if open}
    <div class="backdrop" role="presentation" onclick={() => (open = false)}></div>
    <div
      class="pop"
      role="listbox"
      aria-label="Choose a model or Combo"
      tabindex="-1"
      onkeydown={(e) => e.key === 'Escape' && (open = false)}
    >
      <div class="search">
        <Search size={14} />
        <input
          bind:this={input}
          bind:value={q}
          placeholder="Search models and Combos"
          aria-label="Search models"
          onkeydown={(e) => {
            if (e.key === 'Enter') {
              const first = firstMatch();
              if (first) pick(first);
            }
          }}
        />
      </div>
      <div class="scroll">
        {#if !app.models.length}
          <div class="empty-pick">
            <p class="small muted">No models yet.</p>
            <button
              class="btn sm primary"
              onclick={() => {
                open = false;
                app.settingsTab = 'providers';
              }}>Connect a provider</button
            >
          </div>
        {/if}
        {#if favorites.length && !q}
          <div class="group">Favorites</div>
          {#each favorites as t (t)}
            <button class="opt" role="option" aria-selected={app.target === t} onclick={() => pick(t)}>
              <span class="grow">{nameOf(t)}</span>
              {#if app.target === t}<Check size={14} />{/if}
            </button>
          {/each}
        {/if}
        {#if combos.length}
          <div class="group">Combos</div>
          {#each combos as c (c.id)}
            {@const t = `combo:${c.id}`}
            <div class="opt-row">
              <button class="opt" role="option" aria-selected={app.target === t} onclick={() => pick(t)}>
                <Layers size={13} class="ic" />
                <span class="grow"
                  ><span>{c.name}</span><span class="desc"
                    >{c.members.filter((m) => m.enabled).length} models{c.builtin ? ' · preset' : ''}</span
                  ></span
                >
                {#if app.target === t}<Check size={14} />{/if}
              </button>
              <button
                class="star"
                class:on={app.prefs?.favorites.includes(t)}
                aria-label="Favorite {c.name}"
                onclick={(e) => fav(t, e)}><Star size={13} /></button
              >
            </div>
          {/each}
        {/if}
        {#each groups as [provider, models] (provider)}
          <div class="group">{provider}</div>
          {#each models as m (m.provider + m.model)}
            {@const t = `model:${m.provider}/${m.model}`}
            <div class="opt-row">
              <button class="opt" role="option" aria-selected={app.target === t} onclick={() => pick(t)}>
                <span class="grow"
                  ><span>{m.display.split(' · ').pop()}</span>{#if m.efforts.length}<span class="desc"
                      >effort: {m.efforts.join(' · ')}</span
                    >{/if}</span
                >
                {#if app.target === t}<Check size={14} />{/if}
              </button>
              <button
                class="star"
                class:on={app.prefs?.favorites.includes(t)}
                aria-label="Favorite {m.model}"
                onclick={(e) => fav(t, e)}><Star size={13} /></button
              >
            </div>
          {/each}
        {/each}
      </div>
      <div class="foot">
        <button
          class="btn ghost sm"
          onclick={() => {
            open = false;
            app.settingsTab = 'combos';
          }}>Manage Combos</button
        >
        <button
          class="btn ghost sm"
          onclick={() => {
            open = false;
            app.settingsTab = 'providers';
          }}>Providers</button
        >
      </div>
    </div>
  {/if}
</div>

<style>
  .picker {
    position: relative;
  }
  .trigger {
    max-width: 220px;
    font-size: var(--fs-sm);
  }
  .lbl {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 29;
  }
  .pop {
    position: absolute;
    bottom: calc(100% + 8px);
    left: 0;
    width: 340px;
    max-width: calc(100vw - 32px);
    max-height: 440px;
    z-index: 30;
    display: flex;
    flex-direction: column;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow-lg);
    animation: pop var(--duration) var(--ease);
  }
  .search {
    display: flex;
    align-items: center;
    gap: var(--s2);
    padding: var(--s2) var(--s3);
    border-bottom: 1px solid var(--border);
    color: var(--text-faint);
  }
  .search input {
    flex: 1;
    border: none;
    outline: none;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-sm);
    height: 28px;
  }
  .scroll {
    overflow: auto;
    padding: var(--s1);
  }
  .group {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-faint);
    padding: var(--s2) var(--s2) 2px;
  }
  .opt-row {
    position: relative;
  }
  .opt {
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--s2);
    padding: 6px 32px 6px var(--s2);
    border: none;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-sm);
    border-radius: var(--radius-sm);
    text-align: left;
    cursor: pointer;
  }
  .opt:hover,
  .opt[aria-selected='true'] {
    background: var(--surface-hover);
  }
  .opt .grow {
    display: flex;
    flex-direction: column;
  }
  .desc {
    font-size: 11px;
    color: var(--text-faint);
  }
  .star {
    position: absolute;
    right: 6px;
    top: 50%;
    transform: translateY(-50%);
    border: none;
    background: none;
    color: var(--text-faint);
    opacity: 0;
    display: inline-flex;
    cursor: pointer;
    padding: 4px;
    border-radius: 4px;
  }
  .opt-row:hover .star,
  .star.on,
  .star:focus-visible {
    opacity: 1;
  }
  .star.on {
    color: var(--warning);
  }
  .opt :global(.ic) {
    color: var(--accent);
    flex: none;
  }
  .foot {
    display: flex;
    justify-content: space-between;
    padding: var(--s1);
    border-top: 1px solid var(--border);
  }
  .empty-pick {
    padding: var(--s4);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--s2);
  }
  @keyframes pop {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
  }
</style>
