// Live Combo check with the user's own signed-in AI apps (no API keys):
// a Combo that prefers ChatGPT (Codex CLI) with Gemini (Antigravity CLI) as
// fallback. Whatever ChatGPT answers (a reply, or "usage limit reached"),
// the Combo must end with a real reply and record any hand-off.
//
// Skips unless both `codex` and `agy` are installed. Uses real accounts, so
// it is not part of CI.
//
// Usage: node scripts/desktop-real-combo.mjs [path-to-conductor-app.exe]
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { launch, stepper } from './lib/desktop.mjs';

const has = (cmd) => {
  try {
    execFileSync(process.platform === 'win32' ? 'where' : 'which', [cmd], { stdio: 'ignore', timeout: 15_000 });
    return true;
  } catch {
    return false;
  }
};
if (!has('codex') || !has('agy')) {
  console.log('skipped: needs both Codex CLI and Antigravity CLI signed in');
  process.exit(0);
}

const exe = path.resolve(process.argv[2] ?? 'target/debug/conductor-app.exe');
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'conductor-real-combo-'));
const results = [];
const step = stepper(results);
const app = await launch(exe, path.join(tmp, 'data'));
const { page, invoke } = app;
let code = 0;

async function reply(conversationId, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const c = (await invoke('snapshot')).conversations.find((x) => x.id === conversationId);
    const m = c?.messages.at(-1);
    if (m && m.role === 'assistant' && ['complete', 'failed', 'stopped'].includes(m.status)) return m;
    await new Promise((r) => setTimeout(r, 1000));
  }
  throw new Error('no reply in time');
}

try {
  let codexModel;
  let geminiModel;
  await step('connect ChatGPT (Codex CLI) and Gemini (Antigravity CLI) with no keys', async () => {
    await page.getByRole('dialog', { name: 'Set up Conductor' }).waitFor({ timeout: 30_000 });
    const codex = await invoke('cli_bridge_connect', { cli: 'codex' });
    const agy = await invoke('cli_bridge_connect', { cli: 'agy' });
    codexModel = codex.models.find((m) => m.id !== 'default')?.id;
    geminiModel = agy.models.find((m) => /flash.*low/.test(m.id))?.id ?? agy.models[0].id;
    if (!codexModel) throw new Error(`Codex listed no real models: ${codex.models.map((m) => m.id)}`);
    console.log(`    ChatGPT models: ${codex.models.map((m) => m.id).join(', ')}`);
    console.log(`    Gemini model used: ${geminiModel}`);
  });

  await step('a ChatGPT-first Combo answers, handing off to Gemini if ChatGPT is limited', async () => {
    await invoke('combo_save', {
      combo: {
        id: 'real-accounts',
        name: 'Real accounts',
        description: 'ChatGPT first, Gemini fallback',
        members: [
          { model: `cli-codex/${codexModel}`, roles: [], effort: null, enabled: true, weight: 1 },
          { model: `cli-agy/${geminiModel}`, roles: [], effort: null, enabled: true, weight: 1 },
        ],
        strategy: 'prefer_provider',
        strategy_provider: 'cli-codex',
        fallback: true,
      },
    });
    const project = path.join(tmp, 'proj');
    fs.mkdirSync(project);
    await invoke('open_project', { path: project });
    const projectId = (await invoke('snapshot')).projects[0].id;
    const conv = await invoke('create_conversation', { projectId, mode: 'chat' });
    const started = Date.now();
    const sent = await invoke('send_message', { conversationId: conv.id, text: 'Reply with exactly the word: pong', target: 'combo:real-accounts', effort: null, mode: 'chat' }).then(
      () => '',
      (e) => String(e),
    );
    const m = await reply(conv.id, 240_000);
    console.log(`    answered by ${m.provider} in ${Math.round((Date.now() - started) / 1000)} s: "${m.text.trim().slice(0, 60)}"`);
    if (sent) console.log(`    send reported: ${sent.slice(0, 160)}`);
    if (m.status !== 'complete' || !/pong/i.test(m.text)) throw new Error(`${m.status}: ${m.text.slice(0, 200)}`);
    const usage = (await invoke('usage_overview')).providers.filter((p) => p.session.requests > 0).map((p) => `${p.name}=${p.session.requests}`);
    console.log(`    replies counted: ${usage.join(', ')}`);
  });
} catch {
  code = 1;
} finally {
  await invoke('combo_delete', { id: 'real-accounts' }).catch(() => {});
  for (const id of ['cli-codex', 'cli-agy']) await invoke('remove_provider', { id }).catch(() => {});
  await app.browser.close().catch(() => {});
  try {
    execFileSync('taskkill', ['/F', '/T', '/PID', String(app.proc.pid)], { stdio: 'ignore' });
  } catch {}
  console.log(code === 0 ? '\nReal combo: all steps passed' : '\nReal combo: FAILED');
  process.exit(code);
}
