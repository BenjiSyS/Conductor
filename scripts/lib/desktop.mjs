// Shared helpers for native desktop tests (WebView2 over CDP).
import { chromium } from '@playwright/test';
import { spawn, execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

export const shots = path.resolve('target/screenshots');
fs.mkdirSync(shots, { recursive: true });

export function makeProject(dir) {
  fs.mkdirSync(path.join(dir, 'src'), { recursive: true });
  fs.writeFileSync(path.join(dir, 'src', 'main.rs'), 'fn main() {\n    app::run();\n}\n');
  const git = (...a) => execFileSync('git', a, { cwd: dir, stdio: 'pipe' });
  git('init');
  git('config', 'user.email', 'smoke@example.com');
  git('config', 'user.name', 'Smoke');
  git('config', 'commit.gpgsign', 'false');
  git('add', '-A');
  git('commit', '-m', 'init');
}

export async function startMock(port, env = {}) {
  const p = spawn(process.execPath, ['scripts/mock-llm.mjs', String(port)], { stdio: 'pipe', env: { ...process.env, ...env } });
  await new Promise((r) => p.stdout.once('data', r));
  return p;
}

// Debug builds load the UI from the Vite dev server on port 1420. Start it
// when nothing is serving there, so tests work without manual setup.
let devServer = null;
export async function ensureDevServer(exe) {
  if (!/[\\/]debug[\\/]/.test(exe)) return; // release builds bundle the UI
  const up = () => fetch('http://localhost:1420').then((r) => r.ok, () => false);
  if (await up()) return;
  devServer = spawn(process.execPath, ['node_modules/vite/bin/vite.js', '--host', '127.0.0.1', '--port', '1420', '--strictPort'], { stdio: 'ignore' });
  process.on('exit', () => devServer?.kill());
  for (let i = 0; i < 60 && !(await up()); i++) await new Promise((r) => setTimeout(r, 500));
}

let cdpPort = 9400;
/** Launch the app with an isolated data dir; returns { proc, browser, page, invoke }. */
export async function launch(exe, dataDir) {
  await ensureDevServer(exe);
  const port = cdpPort++;
  const proc = spawn(exe, [], {
    env: { ...process.env, CONDUCTOR_DATA_DIR: dataDir, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`, CONDUCTOR_DEVTOOLS_PORT: String(port) },
    stdio: 'ignore',
  });
  let browser;
  for (let i = 0; i < 160 && !browser; i++) {
    try {
      browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
    } catch {
      await new Promise((r) => setTimeout(r, 250));
    }
  }
  if (!browser) throw new Error('WebView2 DevTools endpoint did not open');
  const ctx = browser.contexts()[0];
  const page = ctx.pages()[0] ?? (await ctx.waitForEvent('page'));
  await page.waitForLoadState('domcontentloaded');
  const invoke = (cmd, args = {}) => page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]);
  return { proc, browser, page, invoke };
}

export function stepper(results) {
  return async (name, fn) => {
    const t = Date.now();
    try {
      await fn();
      results.push({ name, ok: true, ms: Date.now() - t });
      console.log(`  ✓ ${name} (${Date.now() - t} ms)`);
    } catch (e) {
      results.push({ name, ok: false, error: String(e?.message ?? e).split('\n')[0] });
      console.log(`  ✗ ${name}: ${String(e?.message ?? e).split('\n').slice(0, 3).join(' | ')}`);
      throw e;
    }
  };
}

export async function connectProvider(invoke, id, name, url) {
  return invoke('save_provider', { config: { id, name, kind: 'openai_compatible', base_url: url, models: [], enabled: true }, apiKey: 'smoke-key' });
}

/** Post WM_CLOSE to the app's top-level window — exactly what clicking ✕ does. */
export function closeWindow(pid) {
  const ps = `
Add-Type @"
using System; using System.Runtime.InteropServices;
public class W { [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l); }
"@
$p = Get-Process -Id ${pid}
[void][W]::PostMessage($p.MainWindowHandle, 0x10, [IntPtr]::Zero, [IntPtr]::Zero)
$p.MainWindowHandle`;
  return execFileSync('powershell', ['-NoProfile', '-Command', ps], { encoding: 'utf8' }).trim();
}

/** Whether a specific top-level window handle is visible. */
export function windowVisible(handle) {
  const ps = `
Add-Type @"
using System; using System.Runtime.InteropServices;
public class V { [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h); }
"@
[V]::IsWindowVisible([IntPtr]${handle})`;
  return execFileSync('powershell', ['-NoProfile', '-Command', ps], { encoding: 'utf8' }).trim() === 'True';
}

export function alive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
