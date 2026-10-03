import DOMPurify from 'dompurify';
import { marked } from 'marked';

marked.setOptions({ gfm: true, breaks: false });

/** Render model Markdown safely (no raw HTML execution). */
export function markdown(text: string): string {
  const html = marked.parse(text, { async: false }) as string;
  return DOMPurify.sanitize(html, {
    USE_PROFILES: { html: true },
    FORBID_TAGS: ['style', 'iframe', 'form', 'input'],
    FORBID_ATTR: ['style', 'onerror', 'onclick'],
  });
}

export function ago(unixSeconds: number): string {
  const d = Date.now() / 1000 - unixSeconds;
  if (d < 60) return 'just now';
  if (d < 3600) return `${Math.floor(d / 60)}m ago`;
  if (d < 86400) return `${Math.floor(d / 3600)}h ago`;
  if (d < 86400 * 7) return `${Math.floor(d / 86400)}d ago`;
  return new Date(unixSeconds * 1000).toLocaleDateString();
}

export function tokens(n: number): string {
  if (n >= 1_000_000) return `~${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1000) return `~${(n / 1000).toFixed(1)}k`;
  return `~${n}`;
}

export function modelLabel(key: string | null | undefined): string {
  if (!key) return 'Choose a model';
  const k = key.replace(/^(model|combo):/, '');
  const [, model] = k.includes('/') ? k.split(/\/(.*)/s) : ['', k];
  return model || k;
}

export const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform);
export function keyLabel(binding: string): string {
  return binding
    .replace('Mod', isMac ? '⌘' : 'Ctrl')
    .replace('Shift', isMac ? '⇧' : 'Shift')
    .replace(/\+/g, isMac ? '' : '+');
}
export function matchesBinding(e: KeyboardEvent, binding: string): boolean {
  const parts = binding.split('+');
  const key = parts.pop()!.toLowerCase();
  const mod = parts.includes('Mod');
  const shift = parts.includes('Shift');
  const alt = parts.includes('Alt');
  const modPressed = isMac ? e.metaKey : e.ctrlKey;
  return e.key.toLowerCase() === key && mod === modPressed && shift === e.shiftKey && alt === e.altKey;
}

export const roleLabels: Record<string, string> = {
  architect: 'Architect',
  planner: 'Planner',
  coder: 'Coder',
  reviewer: 'Reviewer',
  researcher: 'Researcher',
  debugger: 'Debugger',
  tester: 'Tester',
  release_manager: 'Release',
  documentation: 'Docs',
  security_reviewer: 'Security',
};
