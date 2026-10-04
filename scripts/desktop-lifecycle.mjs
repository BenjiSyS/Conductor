// Native lifecycle, failover and integration checks for the real desktop app.
//
// Covers: Emergency Stop shortcut registration, Caveman on/off changing real
// requests, skill install, MCP Doctor (healthy + broken), remote pairing and
// revocation from a real TLS client with fingerprint pinning, Fast Stop
// killing a running command's process tree, provider failover with handoff,
// Goals continuing after the window is closed, and crash → restart → resume.
//
// Usage: node scripts/desktop-lifecycle.mjs [path-to-conductor-app.exe]
import { execFileSync } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import https from 'node:https';
import os from 'node:os';
import path from 'node:path';
import tls from 'node:tls';
import { alive, closeWindow, connectProvider, launch, makeProject, shots, sleep, startMock, stepper, windowVisible } from './lib/desktop.mjs';

const exe = path.resolve(process.argv[2] ?? 'target/debug/conductor-app.exe');
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'conductor-life-'));
const dataDir = path.join(tmp, 'data');
const project = path.join(tmp, 'demo-app');
makeProject(project);
const results = [];
const step = stepper(results);
const good = await startMock(18090);
const bad = await startMock(18091, { MOCK_FAIL: '429' });
let app;
let code = 0;

const pings = () => {
  try {
    return execFileSync('tasklist', ['/FI', 'IMAGENAME eq PING.EXE', '/NH'], { encoding: 'utf8' }).split('\n').filter((l) => /ping/i.test(l)).length;
  } catch {
    return 0;
  }
};

try {
  app = await launch(exe, dataDir);
  let { page, invoke } = app;
  await page.getByRole('dialog', { name: 'Set up Conductor' }).waitFor({ timeout: 30_000 });
  await page.getByRole('button', { name: 'Skip setup' }).click();
  const bk = await connectProvider(invoke, 'backup', 'Backup', 'http://127.0.0.1:18090/v1');
  await connectProvider(invoke, 'flaky', 'Flaky', 'http://127.0.0.1:18091/v1');
  const proj = await invoke('open_project', { path: project });
  await invoke('save_settings', { settings: { ...(await invoke('snapshot')).settings, permission: 'full_access', setup_complete: true } });
  await page.reload();
  await page.getByRole('heading', { name: 'demo-app', exact: true }).waitFor({ timeout: 15_000 });

  await step('Emergency Stop shortcut is registered with the OS', async () => {
    const info = await invoke('app_info');
    if (!info.emergency_shortcut_registered) throw new Error('shortcut not registered');
  });

  await step('Caveman on/off changes what is actually sent', async () => {
    const send = async (text) => {
      await page.getByRole('button', { name: /Backup|mock-coder|Flaky/ }).first().click();
      await page.getByRole('listbox', { name: 'Choose a model or Combo' }).getByRole('option', { name: /mock-coder/ }).first().click();
      await page.getByLabel('Prompt').fill(text);
      await page.getByLabel('Prompt').press('Enter');
      await page.getByRole('button', { name: 'Send' }).waitFor({ timeout: 20_000 });
    };
    // Make sure the Backup (working) provider's model is chosen.
    await invoke('prefs_save', { prefs: { ...(await invoke('prefs_get')), default_target: `model:${bk.id}/mock-coder` } });
    await page.reload();
    await page.getByRole('heading', { name: 'demo-app', exact: true }).waitFor();
    const replies = () => page.getByRole('article', { name: 'assistant message' });
    const settle = async (n) => {
      for (let i = 0; i < 80; i++) {
        if ((await replies().count()) >= n && !(await page.locator('.who .dot.running').count())) return;
        await sleep(250);
      }
      throw new Error('reply did not finish');
    };
    await page.getByLabel('Prompt').fill('hello with caveman');
    await page.getByLabel('Prompt').press('Enter');
    await settle(1);
    await page.keyboard.press('Control+,');
    const s = page.getByRole('dialog', { name: 'Settings' });
    await s.getByRole('navigation').getByRole('button', { name: 'Optimization' }).click();
    await s.getByRole('switch', { name: 'Caveman' }).click();
    await page.keyboard.press('Escape');
    await page.getByLabel('Prompt').fill('hello without caveman');
    await page.getByLabel('Prompt').press('Enter');
    await settle(2);
    let log = [];
    for (let i = 0; i < 40; i++) {
      log = (await (await fetch('http://127.0.0.1:18090/__log')).json()).filter((l) => !l.failed);
      if (log.length >= 2) break;
      await sleep(250);
    }
    await page.getByRole('button', { name: 'Send' }).waitFor({ timeout: 20_000 });
    const last2 = log.slice(-2).map((l) => l.caveman);
    if (JSON.stringify(last2) !== '[true,false]') {
      await page.screenshot({ path: `${shots}/native-debug-caveman.png` });
      throw new Error(`caveman flags ${JSON.stringify(last2)}`);
    }
    void send;
  });

  await step('install a skill from a local folder', async () => {
    const sk = path.join(tmp, 'steam-skill');
    fs.mkdirSync(sk);
    fs.writeFileSync(path.join(sk, 'SKILL.md'), '---\nname: steam-publish\ndescription: Publish builds to Steam\n---\nUse steamcmd.');
    await page.keyboard.press('Control+,');
    const s = page.getByRole('dialog', { name: 'Settings' });
    await s.getByRole('navigation').getByRole('button', { name: 'MCP, skills & plugins' }).click();
    await s.getByRole('button', { name: 'Skills', exact: true }).click();
    await s.getByLabel('Source').fill(sk);
    await s.getByRole('button', { name: 'Install', exact: true }).click();
    await s.getByText('steam-publish').waitFor({ timeout: 15_000 });
    await page.keyboard.press('Escape');
  });

  await step('MCP Doctor: healthy server connects, broken one gets a fix', async () => {
    const fake = path.join(tmp, 'fake-mcp.js');
    fs.writeFileSync(
      fake,
      "const rl=require('readline').createInterface({input:process.stdin});const s=o=>process.stdout.write(JSON.stringify(o)+'\\n');rl.on('line',l=>{const m=JSON.parse(l);if(m.method==='initialize')s({jsonrpc:'2.0',id:m.id,result:{protocolVersion:m.params.protocolVersion,capabilities:{tools:{}},serverInfo:{name:'fake',version:'1'}}});else if(m.method==='tools/list')s({jsonrpc:'2.0',id:m.id,result:{tools:[{name:'echo',description:'Echo',inputSchema:{type:'object'}}]}});});",
    );
    const mk = (name, command, args) => ({ name, transport: { type: 'stdio', command, args, env: {} }, enabled: true, description: '', source: 'manual', version: null, project: null, providers: [] });
    const d1 = await invoke('mcp_add_custom', { server: mk('local-echo', 'node', [fake]) });
    if (d1.status !== 'connected') throw new Error(`healthy server: ${d1.status} ${d1.summary}`);
    await invoke('mcp_add_custom', { server: mk('broken', 'uvx-not-installed-xyz', ['thing']) });
    await page.keyboard.press('Control+,');
    const s = page.getByRole('dialog', { name: 'Settings' });
    await s.getByRole('navigation').getByRole('button', { name: 'MCP, skills & plugins' }).click();
    await s.getByRole('button', { name: 'Run MCP Doctor' }).click();
    await s.getByText('local-echo connected · 1 tool(s)').waitFor({ timeout: 30_000 });
    await s.getByText(/needs 'uvx-not-installed-xyz'/).waitFor({ timeout: 30_000 });
    await s.getByText(/Fix: Install 'uvx-not-installed-xyz'/).waitFor();
    await page.screenshot({ path: `${shots}/native-06-mcp-doctor.png` });
    await page.keyboard.press('Escape');
  });

  await step('remote: pin certificate, pair, access scoped project, revoke', async () => {
    const st = await invoke('remote_start');
    const pc = await invoke('remote_pair_code', { projects: [proj.id], canControl: true });
    const [host, port] = st.addr.split(':');
    const fp = await new Promise((resolve, reject) => {
      const sock = tls.connect({ host, port: Number(port), rejectUnauthorized: false, servername: 'conductor-host' }, () => {
        const der = sock.getPeerCertificate(true).raw;
        sock.end();
        resolve(crypto.createHash('sha256').update(der).digest('hex').toUpperCase().match(/../g).join(':'));
      });
      sock.on('error', reject);
    });
    if (fp !== st.fingerprint) throw new Error('certificate fingerprint does not match the one shown in the app');
    const req = (method, p, body, token) =>
      new Promise((resolve, reject) => {
        const r = https.request({ host, port, path: p, method, rejectUnauthorized: false, headers: { 'content-type': 'application/json', ...(token ? { authorization: `Bearer ${token}` } : {}) } }, (res) => {
          let d = '';
          res.on('data', (c) => (d += c));
          res.on('end', () => resolve({ status: res.statusCode, body: d ? JSON.parse(d) : null }));
        });
        r.on('error', reject);
        if (body) r.write(JSON.stringify(body));
        r.end();
      });
    const paired = await req('POST', '/api/pair', { code: pc.code, name: 'Smoke phone' });
    if (paired.status !== 200) throw new Error(`pair ${paired.status}`);
    const token = paired.body.token;
    const ok = await req('GET', '/api/status', null, token);
    if (ok.status !== 200 || ok.body.projects[0]?.name !== 'demo-app') throw new Error('status/scope mismatch');
    const f = await req('GET', `/api/projects/${proj.id}/file?path=src/main.rs`, null, token);
    if (!String(f.body.content).includes('app::run')) throw new Error('could not read project file remotely');
    await page.keyboard.press('Control+,');
    const s = page.getByRole('dialog', { name: 'Settings' });
    await s.getByRole('navigation').getByRole('button', { name: 'Remote access', exact: true }).click();
    await s.getByText('Smoke phone').waitFor({ timeout: 10_000 });
    await s.getByRole('button', { name: 'Revoke' }).click();
    await sleep(500);
    const after = await req('GET', '/api/status', null, token);
    if (after.status !== 401) throw new Error(`revoked device still allowed (${after.status})`);
    await invoke('remote_stop');
    await page.keyboard.press('Escape');
  });

  await step('Fast Stop cancels an agent and kills the running command', async () => {
    const before = pings();
    await page.getByRole('button', { name: 'Agent', exact: true }).click();
    await page.getByLabel('Prompt').fill('Run the long command please');
    await page.getByLabel('Prompt').press('Enter');
    for (let i = 0; i < 40 && pings() <= before; i++) await sleep(250);
    if (pings() <= before) throw new Error('command never started');
    await page.getByRole('button', { name: 'Stop' }).click();
    await page.getByRole('article', { name: 'assistant message' }).last().getByText('stopped').waitFor({ timeout: 15_000 });
    await sleep(1500);
    if (pings() > before) throw new Error('process tree still running after Stop');
    await page.getByRole('button', { name: 'Chat', exact: true }).click();
  });

  await step('provider failover hands off without restarting', async () => {
    const combo = {
      id: 'failover',
      name: 'Failover',
      description: '',
      version: 1,
      members: [
        { model: 'flaky/mock-coder', roles: ['coder'], effort: null, enabled: true, weight: 1 },
        { model: `${bk.id}/mock-coder`, roles: ['reviewer', 'planner'], effort: null, enabled: true, weight: 1 },
      ],
      strategy: 'balanced',
      strategy_provider: null,
      reserves: [],
      fallback: true,
      review: 'important',
      max_parallel: 1,
      budget: { max_tokens: null, max_cost_usd: null, ask_at_fraction: null },
      instructions: '',
      phases: [],
      builtin: false,
      history: [],
    };
    await invoke('combo_save', { combo });
    const rec = await invoke('goal_create', { goal: { project_id: proj.id, objective: 'Add a greeting file', checks: ['git ls-files --others --exclude-standard --error-unmatch greeting.txt'], combo_id: 'failover' } });
    await invoke('goal_start', { id: rec.goal.id });
    let g;
    for (let i = 0; i < 120; i++) {
      g = await invoke('goal_get', { id: rec.goal.id });
      if (['complete', 'blocked', 'stopped'].includes(g.goal.state)) break;
      await sleep(500);
    }
    if (g.goal.state !== 'complete') throw new Error(`goal ${g.goal.state}: ${g.goal.notes.join('; ')}`);
    const t = g.goal.graph.tasks.find((x) => x.id === 'write');
    if (!t.failed_models.includes('flaky/mock-coder') || t.assigned_model !== `${bk.id}/mock-coder`) throw new Error(`no reassignment: ${JSON.stringify(t)}`);
    fs.rmSync(path.join(project, 'greeting.txt'), { force: true });
  });

  await step('closing the window keeps a running Goal going in the background', async () => {
    const rec = await invoke('goal_create', { goal: { project_id: proj.id, objective: 'SLOW Add a greeting file', checks: [], combo_id: `model:${bk.id}/mock-coder` } });
    await invoke('goal_start', { id: rec.goal.id });
    await sleep(1500);
    const handle = closeWindow(app.proc.pid);
    await sleep(1500);
    if (!alive(app.proc.pid)) throw new Error('app exited on close');
    if (windowVisible(handle)) throw new Error('window still visible after close');
    let g;
    for (let i = 0; i < 240; i++) {
      g = await invoke('goal_get', { id: rec.goal.id });
      if (['complete', 'blocked'].includes(g.goal.state)) break;
      await sleep(500);
    }
    if (g.goal.state !== 'complete') throw new Error(`background goal ended ${g.goal.state}`);
    fs.rmSync(path.join(project, 'greeting.txt'), { force: true });
  });

  await step('crash mid-Goal → restart → Goal is resumable and finishes', async () => {
    const rec = await invoke('goal_create', { goal: { project_id: proj.id, objective: 'SLOW Add a greeting file again', checks: [], combo_id: `model:${bk.id}/mock-coder` } });
    await invoke('goal_start', { id: rec.goal.id });
    await sleep(2500);
    execFileSync('taskkill', ['/F', '/T', '/PID', String(app.proc.pid)], { stdio: 'ignore' });
    await app.browser.close().catch(() => {});
    await sleep(1500);
    app = await launch(exe, dataDir);
    ({ page, invoke } = app);
    await page.getByText(/interrupted\. Resume from Goals/).waitFor({ timeout: 30_000 });
    const g1 = await invoke('goal_get', { id: rec.goal.id });
    if (g1.goal.state !== 'paused') throw new Error(`expected paused, got ${g1.goal.state}`);
    // Conversations from before the crash are still there.
    if (!(await invoke('snapshot')).conversations.length) throw new Error('conversations lost');
    await page.getByRole('button', { name: 'Open', exact: true }).click();
    await page.getByRole('button', { name: 'Resume' }).click();
    await page.locator('.badge.state').getByText('Complete').waitFor({ timeout: 120_000 });
    await page.screenshot({ path: `${shots}/native-07-resumed.png` });
  });

  await invoke('quit_app').catch(() => {});
} catch (e) {
  console.log(`  fatal: ${String(e?.message ?? e).split('\n')[0]}`);
  code = 1;
} finally {
  await app?.browser?.close().catch(() => {});
  try {
    execFileSync('taskkill', ['/F', '/T', '/PID', String(app?.proc?.pid)], { stdio: 'ignore' });
  } catch {}
  good.kill();
  bad.kill();
  fs.writeFileSync(path.join(shots, 'native-lifecycle.json'), JSON.stringify({ results, at: new Date().toISOString() }, null, 2));
  console.log(results.every((r) => r.ok) && code === 0 ? '\nDesktop lifecycle: all steps passed' : '\nDesktop lifecycle: FAILED');
  process.exit(code);
}
