// Native check of zero-setup providers: the real app connects to AI apps the
// user already has signed in (no API key) and chats through them.
//
// * Antigravity CLI (`agy`): real, when installed and signed in on this
//   machine — a real reply from the user's Google account. Skipped otherwise.
// * Claude Code: a stand-in that reports "Not logged in", to check the app
//   explains how to sign in instead of failing silently.
//
// Usage: node scripts/desktop-bridge.mjs [path-to-conductor-app.exe]
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { launch, shots, stepper } from './lib/desktop.mjs';

const exe = path.resolve(process.argv[2] ?? 'target/debug/conductor-app.exe');
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'conductor-bridge-'));
const results = [];
const step = stepper(results);
let hasAgy = false;
try {
  execFileSync('agy', ['--version'], { stdio: 'ignore', timeout: 15_000 });
  hasAgy = true;
} catch {}

const fakeClaude = path.join(tmp, 'fake-claude.mjs');
fs.writeFileSync(
  fakeClaude,
  `process.stdin.resume(); process.stdin.on('end', () => {
  console.log(JSON.stringify({ type: 'system', subtype: 'init' }));
  console.log(JSON.stringify({ type: 'result', subtype: 'success', is_error: true, result: 'Not logged in · Please run /login' }));
});
if (process.argv.includes('--version')) { console.log('2.0.0 (Claude Code stand-in)'); process.exit(0); }`,
);
process.env.CONDUCTOR_CLI_CLAUDE = `${process.execPath}|${fakeClaude}`;

const app = await launch(exe, path.join(tmp, 'data'));
const { page, invoke } = app;
let code = 0;

async function reply(conversationId, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const snap = await invoke('snapshot');
    const c = snap.conversations.find((x) => x.id === conversationId);
    const m = c?.messages.at(-1);
    if (m && m.role === 'assistant' && ['complete', 'failed', 'stopped'].includes(m.status)) return m;
    await new Promise((r) => setTimeout(r, 500));
  }
  throw new Error('no reply in time');
}

try {
  await step('app starts and the bridge is running', async () => {
    await page.getByRole('dialog', { name: 'Set up Conductor' }).waitFor({ timeout: 30_000 });
    const b = await invoke('cli_bridges');
    if (!b.running) throw new Error('bridge not running');
    const claude = b.bridges.find((x) => x.cli === 'claude');
    if (!claude.installed) throw new Error('stand-in Claude not detected');
    console.log(`    detected: ${b.bridges.filter((x) => x.installed).map((x) => `${x.cli} (${x.version ?? '?'})`).join(', ')}`);
  });

  const project = path.join(tmp, 'proj');
  fs.mkdirSync(project);
  await invoke('open_project', { path: project });
  const projectId = (await invoke('snapshot')).projects[0].id;

  if (hasAgy) {
    await step('wizard offers the signed-in Antigravity CLI and connects it with one click', async () => {
      await page.getByRole('button', { name: 'Get started' }).click();
      await page.getByRole('button', { name: 'Continue' }).click();
      const connect = page.getByRole('button', { name: 'Connect Gemini (Antigravity CLI)' });
      await connect.waitFor({ timeout: 30_000 });
      await page.screenshot({ path: `${shots}/native-bridge-wizard.png` });
      await connect.click();
      await page.getByText(/Gemini \(Antigravity CLI\) connected · \d+ models/).waitFor({ timeout: 90_000 });
    });

    await step('real reply from Gemini through the Antigravity CLI (no API key)', async () => {
      const p = (await invoke('snapshot')).providers.find((x) => x.id === 'cli-agy');
      const model = p.models.find((m) => /flash.*low/.test(m.id))?.id ?? 'default';
      const conv = await invoke('create_conversation', { projectId, mode: 'chat' });
      const started = Date.now();
      await invoke('send_message', { conversationId: conv.id, text: 'Reply with exactly the word: pong', target: `model:cli-agy/${model}`, effort: null, mode: 'chat' });
      const m = await reply(conv.id, 120_000);
      if (m.status !== 'complete' || !/pong/i.test(m.text)) throw new Error(`${m.status}: ${m.text.slice(0, 200)}`);
      console.log(`    ${model} answered "${m.text.trim().slice(0, 40)}" in ${Date.now() - started} ms`);
      const o = await invoke('usage_overview');
      const u = o.providers.find((x) => x.id === 'cli-agy');
      if (u.bridge !== 'gemini' || u.session.requests !== 1 || u.session.input === 0) throw new Error(JSON.stringify(u));
    });
  } else {
    console.log('  - Antigravity CLI not installed: real Gemini bridge steps skipped');
  }

  await step('a signed-out app explains how to sign in', async () => {
    const p = await invoke('cli_bridge_connect', { cli: 'claude' });
    if (p.id !== 'cli-claude') throw new Error(JSON.stringify(p));
    const conv = await invoke('create_conversation', { projectId, mode: 'chat' });
    // The send reports the failure; the saved message must explain it too.
    const sent = await invoke('send_message', { conversationId: conv.id, text: 'hello', target: 'model:cli-claude/default', effort: null, mode: 'chat' }).then(
      () => '',
      (e) => String(e),
    );
    if (!/isn't signed in.*\/login/s.test(sent)) throw new Error(`send: ${sent}`);
    const m = await reply(conv.id, 60_000);
    if (m.status !== 'failed' || !/isn't signed in.*\/login/s.test(m.text)) throw new Error(`${m.status}: ${m.text}`);
  });

  await step('bridge rejects requests without its key', async () => {
    const p = (await invoke('snapshot')).providers.find((x) => x.id === 'cli-claude');
    const r = await fetch(`${p.base_url}/models`);
    if (r.status !== 401) throw new Error(`status ${r.status}`);
    const r2 = await fetch(`${p.base_url}/models`, { headers: { authorization: 'Bearer wrong' } });
    if (r2.status !== 401) throw new Error(`status ${r2.status}`);
  });
} catch {
  code = 1;
} finally {
  for (const id of ['cli-agy', 'cli-claude']) await invoke('remove_provider', { id }).catch(() => {});
  await app.browser.close().catch(() => {});
  try {
    execFileSync('taskkill', ['/F', '/T', '/PID', String(app.proc.pid)], { stdio: 'ignore' });
  } catch {}
  console.log(code === 0 ? '\nBridge: all steps passed' : '\nBridge: FAILED');
  process.exit(code);
}
