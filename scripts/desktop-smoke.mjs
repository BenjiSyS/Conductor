// End-to-end smoke test of the real desktop app (Windows: WebView2).
//
// Launches the built app with an isolated data folder and the WebView2
// DevTools port open, drives the real UI with Playwright over CDP, and
// points it at scripts/mock-llm.mjs (an OpenAI-compatible server), so the
// whole native pipeline runs: keychain-backed provider connect, streaming
// chat, Agent tools with approvals, a Goal with clarifying questions,
// review and Test Gate, checkpoints, Environment Doctor and Conductor Host.
//
// Usage: node scripts/desktop-smoke.mjs [path-to-conductor-app.exe]
import { chromium } from '@playwright/test';
import { spawn, execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import https from 'node:https';
import { ensureDevServer } from './lib/desktop.mjs';

const exe = path.resolve(process.argv[2] ?? 'target/debug/conductor-app.exe');
const shots = path.resolve('target/screenshots');
fs.mkdirSync(shots, { recursive: true });
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'conductor-smoke-'));
const dataDir = path.join(tmp, 'data');
const project = path.join(tmp, 'demo-app');
const results = [];
const step = async (name, fn) => {
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

// Project fixture: a small Git repo.
fs.mkdirSync(path.join(project, 'src'), { recursive: true });
fs.writeFileSync(path.join(project, 'src', 'main.rs'), 'fn main() {\n    app::run();\n}\n');
fs.writeFileSync(path.join(project, 'Cargo.toml'), '[package]\nname = "demo-app"\nversion = "0.1.0"\nedition = "2021"\n');
const git = (...a) => execFileSync('git', a, { cwd: project, stdio: 'pipe' });
git('init');
git('config', 'user.email', 'smoke@example.com');
git('config', 'user.name', 'Smoke');
git('config', 'commit.gpgsign', 'false');
git('add', '-A');
git('commit', '-m', 'init');

const mock = spawn(process.execPath, ['scripts/mock-llm.mjs', '18080'], { stdio: 'pipe' });
await new Promise((r) => mock.stdout.once('data', r));

await ensureDevServer(exe);
const started = Date.now();
const app = spawn(exe, [], {
  env: { ...process.env, CONDUCTOR_DATA_DIR: dataDir, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: '--remote-debugging-port=9333', CONDUCTOR_DEVTOOLS_PORT: '9333' },
  stdio: 'ignore',
});

let browser;
let exitCode = 0;
try {
  // Wait for the WebView2 DevTools endpoint.
  for (let i = 0; i < 120 && !browser; i++) {
    try {
      browser = await chromium.connectOverCDP('http://127.0.0.1:9333');
    } catch {
      await new Promise((r) => setTimeout(r, 250));
    }
  }
  if (!browser) throw new Error('WebView2 DevTools endpoint did not open');
  const ctx = browser.contexts()[0];
  let page = ctx.pages()[0] ?? (await ctx.waitForEvent('page'));
  await page.waitForLoadState('domcontentloaded');
  const invoke = (cmd, args = {}) => page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]);

  await step('app window shows the setup wizard', async () => {
    await page.getByRole('dialog', { name: 'Set up Conductor' }).waitFor({ timeout: 30_000 });
    console.log(`    window interactive after ${Date.now() - started} ms`);
    await page.screenshot({ path: `${shots}/native-01-wizard.png` });
  });

  await step('connect an OpenAI-compatible provider (real HTTP + OS keychain)', async () => {
    await page.getByRole('button', { name: 'Get started' }).click();
    await page.getByRole('button', { name: 'Continue' }).click();
    await page.getByRole('radio', { name: 'Custom (OpenAI-compatible)' }).click();
    await page.getByLabel('Name').fill('Mock');
    await page.getByLabel('Base URL').fill('http://127.0.0.1:18080/v1');
    await page.getByLabel(/API key/).fill('smoke-test-key');
    await page.getByRole('button', { name: 'Test & connect' }).click();
    await page.getByText('Mock connected · 2 models').waitFor({ timeout: 15_000 });
    for (let i = 0; i < 5; i++) await page.getByRole('button', { name: 'Continue' }).click();
    await page.getByRole('button', { name: 'Done' }).click();
  });

  await step('open the project', async () => {
    await invoke('open_project', { path: project });
    await page.reload();
    await page.getByRole('heading', { name: 'demo-app', exact: true }).waitFor({ timeout: 15_000 });
  });

  await step('chat streams a real reply with project context', async () => {
    await page.getByLabel('Prompt').fill('Where is the entry point?');
    await page.getByRole('button', { name: /Context budget/ }).waitFor({ timeout: 15_000 });
    await page.getByLabel('Prompt').press('Enter');
    await page.getByRole('article', { name: 'assistant message' }).getByText('src/main.rs').waitFor({ timeout: 20_000 });
    await page.getByRole('button', { name: 'Send' }).waitFor({ timeout: 20_000 });
    await page.screenshot({ path: `${shots}/native-02-chat.png` });
  });

  await step('Agent mode edits files only after approval (Ask permission)', async () => {
    await page.getByRole('button', { name: 'Agent', exact: true }).click();
    await page.getByLabel('Prompt').fill('Create a greeting file');
    await page.getByLabel('Prompt').press('Enter');
    const bar = page.getByRole('alertdialog', { name: 'Permission request' });
    await bar.getByText('Edit files?').waitFor({ timeout: 20_000 });
    await page.screenshot({ path: `${shots}/native-03-approval.png` });
    if (fs.existsSync(path.join(project, 'greeting.txt'))) throw new Error('file written before approval');
    await bar.getByRole('button', { name: 'Allow' }).click();
    await page.getByRole('alertdialog', { name: 'Permission request' }).getByText('Run a command?').waitFor({ timeout: 20_000 });
    await page.getByRole('alertdialog', { name: 'Permission request' }).getByRole('button', { name: 'Allow' }).click();
    await page.getByRole('article', { name: 'assistant message' }).last().getByText('Created greeting.txt').waitFor({ timeout: 30_000 });
    const content = fs.readFileSync(path.join(project, 'greeting.txt'), 'utf8');
    if (content !== 'Hello from Conductor\n') throw new Error(`unexpected file content ${JSON.stringify(content)}`);
    const cps = await invoke('checkpoints_list', { projectId: (await invoke('snapshot')).projects[0].id });
    if (!cps.some((c) => c.label === 'before agent changes')) throw new Error('no checkpoint before agent changes');
    fs.rmSync(path.join(project, 'greeting.txt'));
  });

  await step('Full Access is set from Settings', async () => {
    await page.getByRole('button', { name: /^Permissions:/ }).click();
    await page.getByRole('radio', { name: /Full Access/ }).click();
    await page.keyboard.press('Escape');
    await page.getByRole('button', { name: 'Permissions: Full Access' }).waitFor();
  });

  await step('Goal: filtered questions, plan, tools, review, Test Gate, verified complete', async () => {
    await page.getByLabel('Prompt').fill('/goal Add a greeting file to the project');
    await page.getByRole('button', { name: /Checks/ }).click();
    await page.getByLabel(/Definition of Done/).fill('git ls-files --others --exclude-standard --error-unmatch greeting.txt');
    await page.getByRole('button', { name: 'Start Goal' }).click();
    const card = page.getByRole('region', { name: 'A few questions before starting' });
    await card.getByText('Which platforms matter?').waitFor({ timeout: 20_000 });
    if (await card.getByText('May I run the tests?').count()) throw new Error('routine permission question was not filtered');
    await card.getByRole('button', { name: 'Decide for me' }).first().click();
    try {
      await page.locator('.badge.state').getByText('Complete').waitFor({ timeout: 90_000 });
    } catch (e) {
      await page.screenshot({ path: `${shots}/native-04-goal-failed.png`, fullPage: true });
      const goals = await invoke('goals_list');
      const rec = goals[0] && (await invoke('goal_get', { id: goals[0].id }));
      console.log('    goal state:', JSON.stringify({ state: rec?.goal.state, notes: rec?.goal.notes, tasks: rec?.goal.graph.tasks.map((t) => [t.id, t.status, t.verification, (t.result ?? '').slice(0, 80)]), checks: rec?.goal.checks }, null, 1));
      console.log('    approvals:', JSON.stringify(await invoke('approvals_pending')));
      throw e;
    }
    await page.screenshot({ path: `${shots}/native-04-goal.png`, fullPage: true });
    if (fs.readFileSync(path.join(project, 'greeting.txt'), 'utf8') !== 'Hello from Conductor\n') throw new Error('goal did not write file');
    const goals = await invoke('goals_list');
    const rec = await invoke('goal_get', { id: goals[0].id });
    if (!rec.checkpoint) throw new Error('no checkpoint before goal');
    if (!rec.goal.checks.some((c) => c.passed)) throw new Error('no passing check evidence');
    const mem = await invoke('memory_get', { projectId: rec.project_id });
    if (!mem.decisions.some((d) => d.topic === 'platforms')) throw new Error('clarification decision not remembered');
  });

  await step('Environment Doctor lists installed tools', async () => {
    await page.keyboard.press('Control+,');
    const s = page.getByRole('dialog', { name: 'Settings' });
    await s.getByRole('navigation').getByRole('button', { name: 'Environment', exact: true }).click();
    await s.getByRole('cell', { name: 'Git', exact: true }).waitFor({ timeout: 30_000 });
  });

  await step('Conductor Host starts over TLS and rejects unpaired clients', async () => {
    const s = page.getByRole('dialog', { name: 'Settings' });
    await s.getByRole('navigation').getByRole('button', { name: 'Remote access', exact: true }).click();
    await s.getByRole('switch', { name: 'Conductor Host' }).click();
    await s.getByText(/Listening on/).waitFor({ timeout: 15_000 });
    await s.getByRole('button', { name: 'Create pairing code' }).click();
    await s.locator('.code').waitFor();
    await page.screenshot({ path: `${shots}/native-05-remote.png` });
    const st = await invoke('remote_status');
    const status = await new Promise((resolve, reject) => {
      https.get(`https://${st.addr}/api/status`, { rejectUnauthorized: false }, (r) => resolve(r.statusCode)).on('error', reject);
    });
    if (status !== 401) throw new Error(`expected 401 for unpaired client, got ${status}`);
    await s.getByRole('switch', { name: 'Conductor Host' }).click();
    await page.keyboard.press('Escape');
  });

  await step('startup timing is recorded', async () => {
    const info = await invoke('app_info');
    console.log('    startup phases:', JSON.stringify(info.startup));
  });

  await step('mock provider saw authenticated, compact requests', async () => {
    const log = await (await fetch('http://127.0.0.1:18080/__log')).json();
    if (!log.length) throw new Error('no provider calls');
    if (log.some((l) => l.auth !== 'present')) throw new Error('missing auth header');
    console.log(`    ${log.length} provider calls; largest request ${Math.max(...log.map((l) => l.chars))} bytes`);
  });

  await invoke('quit_app').catch(() => {});
} catch (e) {
  console.log(`  fatal: ${String(e?.message ?? e).split('\n')[0]}`);
  exitCode = 1;
} finally {
  await browser?.close().catch(() => {});
  app.kill();
  mock.kill();
  fs.writeFileSync(path.join(shots, 'native-smoke.json'), JSON.stringify({ results, at: new Date().toISOString() }, null, 2));
  console.log(results.every((r) => r.ok) && exitCode === 0 ? '\nDesktop smoke: all steps passed' : '\nDesktop smoke: FAILED');
  process.exit(exitCode);
}
