import { describe, expect, it } from 'vitest';
import { ago, keyLabel, matchesBinding, modelLabel, tokens } from '../../apps/desktop/ui/lib/format';

const key = (k: string, mods: Partial<KeyboardEvent> = {}) =>
  ({ key: k, ctrlKey: false, metaKey: false, shiftKey: false, altKey: false, ...mods }) as KeyboardEvent;

describe('keyboard bindings', () => {
  it('matches Mod shortcuts with exact modifiers', () => {
    expect(matchesBinding(key('k', { ctrlKey: true }), 'Mod+K')).toBe(true);
    expect(matchesBinding(key('K', { ctrlKey: true }), 'Mod+K')).toBe(true);
    expect(matchesBinding(key('k'), 'Mod+K')).toBe(false);
    expect(matchesBinding(key('k', { ctrlKey: true, shiftKey: true }), 'Mod+K')).toBe(false);
    expect(matchesBinding(key(',', { ctrlKey: true }), 'Mod+,')).toBe(true);
    expect(matchesBinding(key('Escape', { ctrlKey: true, shiftKey: true }), 'Mod+Shift+Escape')).toBe(true);
  });

  it('labels shortcuts for display', () => {
    expect(keyLabel('Mod+K')).toBe('Ctrl+K');
    expect(keyLabel('Mod+Shift+Escape')).toBe('Ctrl+Shift+Escape');
  });
});

describe('display formatting', () => {
  it('formats token estimates with a tilde (always estimates)', () => {
    expect(tokens(12)).toBe('~12');
    expect(tokens(2414)).toBe('~2.4k');
    expect(tokens(3_200_000)).toBe('~3.2M');
  });

  it('extracts model names from targets', () => {
    expect(modelLabel('model:openai/gpt-x')).toBe('gpt-x');
    expect(modelLabel('model:custom/org/model')).toBe('org/model');
    expect(modelLabel(null)).toBe('Choose a model');
  });

  it('formats relative times', () => {
    const now = Date.now() / 1000;
    expect(ago(now - 5)).toBe('just now');
    expect(ago(now - 120)).toBe('2m ago');
    expect(ago(now - 7200)).toBe('2h ago');
    expect(ago(now - 3 * 86400)).toBe('3d ago');
  });
});
