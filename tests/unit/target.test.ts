import { beforeEach, describe, expect, it } from 'vitest';
import { app, ensureTarget, togglePanel } from '../../apps/desktop/ui/lib/app.svelte';
import type { Combo, ModelProfile, Prefs } from '../../apps/desktop/ui/lib/types';

const model = (provider: string, m: string, local = false): ModelProfile => ({
  provider,
  model: m,
  display: `${provider} · ${m}`,
  tier: local ? 'local' : 'strong',
  capabilities: [],
  efforts: [],
  context_window: 100_000,
  local,
});

const combo = (id: string): Combo => ({
  id,
  name: id,
  description: '',
  version: 1,
  members: [{ model: 'openai/a', roles: [], effort: null, enabled: true, weight: 1 }],
  strategy: 'balanced',
  strategy_provider: null,
  reserves: [],
  fallback: true,
  review: 'important',
  max_parallel: 2,
  budget: { max_tokens: null, max_cost_usd: null, ask_at_fraction: null },
  instructions: '',
  phases: [],
  builtin: true,
  history: [],
});

describe('default model / Combo selection', () => {
  beforeEach(() => {
    app.models = [model('openai', 'a'), model('ollama', 'llama', true)];
    app.combos = [combo('balanced')];
    app.prefs = { default_target: null, recents: [], privacy: 'standard' } as unknown as Prefs;
    app.target = null;
  });

  it('keeps a valid current target', () => {
    app.target = 'combo:balanced';
    ensureTarget();
    expect(app.target).toBe('combo:balanced');
  });

  it('replaces a stale target with the configured default, then recents', () => {
    app.target = 'model:gone/x';
    app.prefs = { ...app.prefs!, default_target: 'model:openai/a' };
    ensureTarget();
    expect(app.target).toBe('model:openai/a');
    app.target = 'model:gone/x';
    app.prefs = { ...app.prefs!, default_target: null, recents: ['model:gone/y', 'combo:balanced'] };
    ensureTarget();
    expect(app.target).toBe('combo:balanced');
  });

  it('Local First prefers a local model when nothing is chosen', () => {
    app.prefs = { ...app.prefs!, privacy: 'local_first' };
    ensureTarget();
    expect(app.target).toBe('model:ollama/llama');
  });

  it('no models means no target', () => {
    app.models = [];
    app.combos = [];
    ensureTarget();
    expect(app.target).toBeNull();
  });
});

describe('side panels', () => {
  it('are mutually exclusive and toggle', () => {
    togglePanel('git');
    expect(app.gitOpen).toBe(true);
    togglePanel('inspector');
    expect(app.inspectorOpen).toBe(true);
    expect(app.gitOpen).toBe(false);
    togglePanel('inspector');
    expect(app.inspectorOpen).toBe(false);
  });
});
