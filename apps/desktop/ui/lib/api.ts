// Native bridge. Inside the desktop app every call goes to Rust over Tauri
// IPC. In a plain browser (UI development, automated UI tests) an in-memory
// mock backend answers instead, and the UI says so clearly.

import type { EngineEvent, RunUpdate } from './types';

type Handler<T> = (payload: T) => void;

interface Bridge {
  call<T>(cmd: string, args?: Record<string, unknown>): Promise<T>;
  listen<T>(event: string, cb: Handler<T>): Promise<() => void>;
  chooseFolder(title?: string): Promise<string | null>;
}

export const native = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

let bridge: Promise<Bridge> | null = null;

function load(): Promise<Bridge> {
  if (bridge) return bridge;
  bridge = native
    ? (async () => {
        const core = await import('@tauri-apps/api/core');
        const ev = await import('@tauri-apps/api/event');
        const dialog = await import('@tauri-apps/plugin-dialog');
        return {
          call: <T>(cmd: string, args?: Record<string, unknown>) => core.invoke<T>(cmd, args),
          listen: async <T>(event: string, cb: Handler<T>) => ev.listen<T>(event, (e) => cb(e.payload)),
          chooseFolder: async (title = 'Choose a folder') => {
            const r = await dialog.open({ directory: true, multiple: false, title });
            return typeof r === 'string' ? r : null;
          },
        };
      })()
    : import('./mock').then((m) => m.mockBridge);
  return bridge;
}

export async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const b = await load();
  return b.call<T>(cmd, args);
}

export async function listen<T>(event: string, cb: Handler<T>): Promise<() => void> {
  const b = await load();
  return b.listen<T>(event, cb);
}

export async function chooseFolder(title?: string): Promise<string | null> {
  const b = await load();
  return b.chooseFolder(title);
}

export const onRunUpdate = (cb: Handler<RunUpdate>) => listen<RunUpdate>('run-update', cb);
export const onEngine = (cb: Handler<EngineEvent>) => listen<EngineEvent>('engine', cb);

/** Turn backend errors into short, human sentences. */
export function readable(e: unknown): string {
  const raw = typeof e === 'string' ? e : e instanceof Error ? e.message : JSON.stringify(e);
  const m = raw.replace(/^Error:\s*/, '');
  if (/401|invalid_grant|unauthori[sz]ed/i.test(m) && m.length > 120)
    return 'Provider signed out. Reconnect to continue.';
  return m.length > 400 ? m.slice(0, 400) + '…' : m;
}
