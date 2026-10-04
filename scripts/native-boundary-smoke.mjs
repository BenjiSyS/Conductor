// Isolated integration tests of real desktop IPC and loopback TLS boundaries.
// No live provider accounts, downloaded packages, tunnels, login settings or
// existing user data are touched. Keep evidence separate from UI smoke tests.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { spawn, execFileSync } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import https from 'node:https';
import http from 'node:http';
import net from 'node:net';
import { chromium } from '@playwright/test';

const output = path.resolve('target/native-boundary-codex');
fs.mkdirSync(output, { recursive: true });
const root = fs.mkdtempSync(path.join(output, 'run-'));
const data = path.join(root, 'data');
const project = path.join(root, 'project');
const exe = path.resolve(process.argv[2] ?? 'target/debug/conductor-app.exe');
assert.ok(fs.existsSync(exe), `Build the desktop app first: ${exe}`);
fs.mkdirSync(project);
fs.writeFileSync(path.join(project, 'hello.txt'), 'initial\n');
fs.writeFileSync(path.join(project, '.env'), 'PASSWORD=fixture-only\n');
const git = (...args) => execFileSync('git', args, { cwd: project, stdio: 'pipe', windowsHide: true });
git('init'); git('config', 'user.email', 'fixture@example.invalid');
git('config', 'user.name', 'Conductor test'); git('config', 'commit.gpgsign', 'false');
git('add', 'hello.txt'); git('commit', '-m', 'fixture');

const mcpFixture = path.join(root, 'mcp-fixture.mjs');
fs.writeFileSync(mcpFixture, `import {createInterface} from 'node:readline';
for await (const line of createInterface({input:process.stdin})) {
  const req=JSON.parse(line); if(req.id===undefined)continue;
  let result;
  if(req.method==='initialize')result={protocolVersion:'2024-11-05',capabilities:{tools:{}},serverInfo:{name:'native-fixture',version:'1.0.0'}};
  else if(req.method==='tools/list')result={tools:[{name:'fixture_echo',description:'Fixture only',inputSchema:{type:'object'}}]};
  else if(req.method==='tools/call')result={content:[{type:'text',text:'fixture response'}]};
  else result={};
  process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:req.id,result})+'\\n');
}
`);

// Reserve a fresh debugging port before starting only this process.
const port = await new Promise((resolve, reject) => {
  const server = net.createServer(); server.on('error', reject);
  server.listen(0, '127.0.0.1', () => { const port = server.address().port; server.close(() => resolve(port)); });
});
function launch() { return spawn(exe, [], {
  env: { ...process.env, CONDUCTOR_DATA_DIR: data, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}` },
  stdio: 'ignore', windowsHide: true,
}); }
let app = launch();
const results = [];
let browser, remoteBrowser, remotePage, invoke, projectId, otherProjectId, host, control, viewer;
const secretName = `boundary-${randomUUID()}`;
const sentinel = `sk-fixture-${randomUUID().replaceAll('-', '')}`;
const providerId = `boundary-${randomUUID()}`;
let replyMode = 'stream';
let streamReady;
const provider = http.createServer((request, response) => {
  if (request.headers.authorization !== `Bearer ${sentinel}`) { response.writeHead(401); response.end('{}'); return; }
  if (request.url === '/v1/models') { response.setHeader('content-type', 'application/json'); response.end(JSON.stringify({ data: [{ id: 'boundary-model' }] })); return; }
  if (request.url !== '/v1/chat/completions') { response.writeHead(404); response.end('{}'); return; }
  request.resume(); request.on('end', () => {
    response.setHeader('content-type', 'text/event-stream');
    const text = replyMode === 'unshared' ? '<done>PRIVATE_UNSHARED_PROJECT_FIXTURE</done>' : replyMode === 'approval' ? '<write_file path="should-not-exist.txt">bad</write_file>' : 'partial native fixture';
    response.write(`data: ${JSON.stringify({ choices: [{ delta: { content: text } }] })}\n\n`);
    if (replyMode !== 'stream') response.end('data: [DONE]\n\n');
    streamReady?.();
  });
});
await new Promise(resolve => provider.listen(0, '127.0.0.1', resolve));
const providerPort = provider.address().port;
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
async function within(promise, ms, message) {
  let timer;
  try { return await Promise.race([promise, new Promise((_, reject) => { timer = setTimeout(() => reject(new Error(message)), ms); })]); }
  finally { clearTimeout(timer); }
}
async function step(name, fn) {
  const started = Date.now();
  try { await fn(); results.push({ name, ok: true, ms: Date.now() - started }); console.log(`PASS ${name}`); }
  catch (error) { results.push({ name, ok: false, error: String(error.message), ms: Date.now() - started }); console.log(`FAIL ${name}: ${error.message}`); }
}
async function connect() {
  // Cold CI runners can take well over 30 s to start WebView2 in a debug build.
  for (let tries = 0; tries < 480; tries++) {
    if (app.exitCode !== null) throw new Error(`App exited: ${app.exitCode}`);
    try { browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`); break; }
    catch { await new Promise(resolve => setTimeout(resolve, 250)); }
  }
  if (!browser) {
    const logs = path.join(data, 'logs');
    const tail = fs.existsSync(logs) ? fs.readdirSync(logs).map((f) => fs.readFileSync(path.join(logs, f), 'utf8')).join(' | ').slice(-2000) : 'no app log';
    throw new Error(`WebView2 debugging endpoint did not open after 120 s. App log tail: ${tail}`);
  }
  const page = browser.contexts()[0].pages()[0];
  await page.waitForFunction(() => !!window.__TAURI_INTERNALS__, null, { timeout: 30_000 });
  invoke = (command, args = {}) => page.evaluate(([command, args]) => window.__TAURI_INTERNALS__.invoke(command, args), [command, args]);
}

// Trust comes from the native IPC fingerprint, not a disabled CA check. Pin
// the presented certificate before accepting any HTTP response or sending data.
function remote(route, { token, method = 'GET', body } = {}) {
  return new Promise((resolve, reject) => {
    const headers = {}; if (token) headers.authorization = `Bearer ${token}`;
    if (body) headers['content-type'] = 'application/json';
    const request = https.request(`https://${host.addr}${route}`, { method, headers, rejectUnauthorized: false, agent: false }, response => {
      let bytes = ''; response.on('data', chunk => { bytes += chunk; if (bytes.length > 3_000_000) request.destroy(new Error('Fixture response exceeds bound')); });
      response.on('end', () => { try { resolve({ status: response.statusCode, value: bytes ? JSON.parse(bytes) : null }); } catch (error) { reject(error); } });
    });
    request.on('error', reject); request.setTimeout(10_000, () => request.destroy(new Error('Remote timeout')));
    request.on('socket', socket => socket.once('secureConnect', () => {
      const actual = createHash('sha256').update(socket.getPeerCertificate().raw).digest('hex').toUpperCase();
      if (actual !== host.fingerprint.replaceAll(':', '')) { request.destroy(new Error('TLS fingerprint mismatch')); return; }
      request.end(body ? JSON.stringify(body) : undefined);
    }));
  });
}
try {
  await connect();
  await step('isolated native data directory', async () => {
    const info = await invoke('app_info'); assert.equal(path.resolve(info.data_dir), data); assert.equal(info.os, 'windows');
  });
  await step('open and reopen one canonical project', async () => {
    const first = await invoke('open_project', { path: project });
    const second = await invoke('open_project', { path: project });
    assert.equal(first.id, second.id); projectId = first.id;
    assert.equal((await invoke('snapshot')).projects.length, 1);
  });
  await step('OS vault secret and export omit plaintext', async () => {
    await invoke('secret_set', { name: secretName, value: sentinel });
    const exported = await invoke('config_export'); assert.ok(!exported.includes(sentinel));
    assert.equal(JSON.parse(exported).format, 'conductor.config');
  });
  await step('MCP missing executable is diagnosed', async () => {
    const diagnosis = await invoke('mcp_add_custom', { server: { name: 'missing-fixture', transport: { type: 'stdio', command: 'conductor-nonexistent-fixture' }, enabled: true } });
    assert.equal(diagnosis.status, 'missing_dependency');
  });
  await step('MCP stdio initialization and tools listing', async () => {
    const diagnosis = await invoke('mcp_add_custom', { server: { name: 'stdio-fixture', transport: { type: 'stdio', command: process.execPath, args: [mcpFixture] }, enabled: true } });
    assert.equal(diagnosis.status, 'connected'); assert.deepEqual(diagnosis.tools, ['fixture_echo']);
  });
  await step('MCP disable, export and removal', async () => {
    assert.ok((await invoke('mcp_export', { target: 'codex' })).includes('stdio-fixture'));
    await invoke('mcp_set_enabled', { name: 'stdio-fixture', enabled: false });
    const diagnoses = await invoke('mcp_doctor', { name: 'stdio-fixture' }); assert.equal(diagnoses[0].status, 'disabled');
    assert.ok(!(await invoke('mcp_export', { target: 'codex' })).includes('stdio-fixture'), 'Disabled servers must stay out of provider exports');
    await invoke('mcp_remove', { name: 'stdio-fixture' }); await invoke('mcp_remove', { name: 'missing-fixture' });
    assert.equal((await invoke('mcp_list')).servers.length, 0);
  });
  await step('frontmatter skill installs and removes', async () => {
    const source = path.join(root, 'skill'); fs.mkdirSync(source);
    fs.writeFileSync(path.join(source, 'SKILL.md'), '---\nname: boundary-skill\ndescription: Local fixture only\n---\nUse exact fixture text.\n');
    const installed = await invoke('skills_install', { source }); assert.equal(installed.name, 'boundary-skill');
    assert.ok((await invoke('skills_list')).some(item => item.manifest.name === installed.name));
    await invoke('skills_remove', { name: installed.name }); assert.equal((await invoke('skills_list')).length, 0);
  });
  await step('declarative plugin installs and removes', async () => {
    const source = path.join(root, 'plugin'); fs.mkdirSync(source);
    fs.writeFileSync(path.join(source, 'plugin.toml'), 'name = "boundary-plugin"\nversion = "1.0.0"\ndescription = "Fixture"\npermissions = []\n');
    const installed = await invoke('plugins_install', { source }); assert.equal(installed.name, 'boundary-plugin');
    assert.equal((await invoke('plugins_list'))[0].health, 'healthy');
    await invoke('plugins_remove', { name: installed.name }); assert.equal((await invoke('plugins_list')).length, 0);
  });
  await step('pure theme installs and executable values are refused', async () => {
    const source = path.join(root, 'theme'); fs.mkdirSync(source);
    const manifest = { name: 'boundary-theme', version: '1.0.0', light: { bg: '#ffffff', accent: '#2563eb' }, dark: { bg: '#111111' } };
    fs.writeFileSync(path.join(source, 'theme.json'), JSON.stringify(manifest));
    const installed = await invoke('themes_install', { source }); assert.equal(installed.name, 'boundary-theme');
    await invoke('themes_remove', { name: installed.name }); assert.equal((await invoke('themes_list')).length, 0);
    manifest.name = 'unsafe-theme'; manifest.light.bg = 'url(https://example.invalid/track)';
    fs.writeFileSync(path.join(source, 'theme.json'), JSON.stringify(manifest));
    await assert.rejects(invoke('themes_install', { source }));
  });
  await step('Caveman toggles survive settings persistence', async () => {
    const snapshot = await invoke('snapshot'); snapshot.settings.caveman = false;
    await invoke('save_settings', { settings: snapshot.settings }); assert.equal((await invoke('caveman_status')).enabled, false);
    snapshot.settings.caveman = true; await invoke('save_settings', { settings: snapshot.settings }); assert.equal((await invoke('caveman_status')).enabled, true);
  });
  await step('custom provider connection uses real HTTP and OS vault', async () => {
    const configured = await invoke('save_provider', { config: { id: providerId, name: 'Boundary fixture', kind: 'openai_compatible', base_url: `http://127.0.0.1:${providerPort}/v1`, models: [], enabled: true }, apiKey: sentinel });
    assert.equal(configured.models.length, 1); assert.equal(configured.enabled, true);
    assert.ok(!(await invoke('config_export')).includes(sentinel));
  });
  await step('Fast Stop cancels HTTP and preserves streamed text', async () => {
    replyMode = 'stream';
    const conversation = await invoke('create_conversation', { projectId, mode: 'chat' });
    const ready = new Promise(resolve => { streamReady = resolve; });
    const run = invoke('send_message', { conversationId: conversation.id, text: 'Stream a fixture reply', target: `model:${providerId}/boundary-model`, effort: null, mode: 'chat' }).then(() => true, error => ({ error: String(error) }));
    try {
      await within(ready, 10_000, 'Provider request did not start'); await delay(100);
      await invoke('stop', { conversationId: conversation.id });
      await within(run, 2000, 'Stop did not cancel the provider request within 2 seconds');
      const saved = (await invoke('snapshot')).conversations.find(item => item.id === conversation.id);
      assert.equal(saved.messages.at(-1).status, 'stopped');
      assert.equal(saved.messages.at(-1).text, 'partial native fixture', 'Stopping must preserve the partial reply in SQLite');
    } finally { streamReady = undefined; await invoke('emergency_stop'); await within(run, 2000, 'Emergency cleanup did not stop request'); }
  });
  await step('Fast Stop cancels an Agent waiting for permission', async () => {
    replyMode = 'approval';
    const conversation = await invoke('create_conversation', { projectId, mode: 'agent' });
    const run = invoke('send_message', { conversationId: conversation.id, text: 'Request a file edit', target: `model:${providerId}/boundary-model`, effort: null, mode: 'agent' }).then(() => true, error => ({ error: String(error) }));
    try {
      await within((async () => { while (!(await invoke('approvals_pending')).length) await delay(30); })(), 10_000, 'Agent did not request permission');
      assert.ok(!fs.existsSync(path.join(project, 'should-not-exist.txt')));
      await invoke('stop', { conversationId: conversation.id });
      await within(run, 2000, 'Stop left Agent waiting on a permission request');
      assert.ok(!fs.existsSync(path.join(project, 'should-not-exist.txt')));
    } finally { await invoke('emergency_stop'); await within(run, 2000, 'Emergency cleanup did not stop Agent'); }
  });
  await step('config round trip and invalid format rejection', async () => {
    const json = await invoke('config_export'); assert.ok((await invoke('config_import', { json })).length > 0);
    await assert.rejects(invoke('config_import', { json: '{"format":"unrelated"}' }));
  });
  await step('diagnostics omit fixture secrets and source code', async () => {
    const reportPath = await invoke('diagnostics_export'); assert.equal(path.dirname(path.resolve(reportPath)), data);
    const text = fs.readFileSync(reportPath, 'utf8'); assert.ok(!text.includes(sentinel)); assert.ok(!text.includes('initial\\n'));
  });
  await step('TLS host rejects an unpaired request', async () => {
    const other = path.join(root,'unshared-project'); fs.mkdirSync(other); fs.writeFileSync(path.join(other,'private.txt'),'Unshared fixture project\n');
    otherProjectId = (await invoke('open_project',{path:other})).id;
    const prefs = await invoke('prefs_get'); prefs.remote_bind = '127.0.0.1'; prefs.remote_port = 0;
    await invoke('prefs_save', { prefs }); host = await invoke('remote_start'); assert.equal(host.running, true);
    assert.equal((await remote('/api/status')).status, 401);
  });
  await step('one-use pairing code scopes project access', async () => {
    const pairing = await invoke('remote_pair_code', { projects: [projectId], canControl: true });
    const paired = await remote('/api/pair', { method: 'POST', body: { code: pairing.code, name: 'control-fixture' } });
    assert.equal(paired.status, 200); control = paired.value;
    assert.equal((await remote('/api/pair', { method: 'POST', body: { code: pairing.code, name: 'replay' } })).status, 401);
    const status = await remote('/api/status', { token: control.token }); assert.equal(status.status, 200); assert.equal(status.value.projects.length, 1);
    assert.equal((await remote('/api/projects/unshared/file?path=hello.txt', { token: control.token })).status, 403);
  });
  await step('remote file edit checks hash and preserves local conflict', async () => {
    const route = `/api/projects/${projectId}/file`;
    const first = await remote(`${route}?path=hello.txt`, { token: control.token }); assert.equal(first.status, 200);
    const edited = await remote(route, { token: control.token, method: 'PUT', body: { path: 'hello.txt', content: 'remote\n', base_hash: first.value.hash } });
    assert.equal(edited.status, 200); assert.equal(fs.readFileSync(path.join(project, 'hello.txt'), 'utf8'), 'remote\n');
    fs.writeFileSync(path.join(project, 'hello.txt'), 'local concurrent edit\n');
    const conflict = await remote(route, { token: control.token, method: 'PUT', body: { path: 'hello.txt', content: 'overwrite\n', base_hash: edited.value.hash } });
    assert.equal(conflict.status, 409); assert.equal(fs.readFileSync(path.join(project, 'hello.txt'), 'utf8'), 'local concurrent edit\n');
  });
  await step('remote sensitive files and Git aliases are blocked', async () => {
    const route = `/api/projects/${projectId}/file`;
    for (const filename of ['.env', '../outside.txt', '.git./config']) {
      assert.equal((await remote(`${route}?path=${encodeURIComponent(filename)}`, { token: control.token })).status, 403, filename);
    }
    for (const filename of ['.git/config', '.GIT/config', '.git./config', '.git /config']) {
      assert.equal((await remote(route, { token: control.token, method: 'PUT', body: { path: filename, content: 'bad', base_hash: null } })).status, 403, filename);
    }
  });
  await step('view-only paired device cannot write', async () => {
    const pairing = await invoke('remote_pair_code', { projects: [projectId], canControl: false });
    const paired = await remote('/api/pair', { method: 'POST', body: { code: pairing.code, name: 'viewer-fixture' } }); assert.equal(paired.status, 200); viewer = paired.value;
    assert.equal((await remote(`/api/projects/${projectId}/file`, { token: viewer.token, method: 'PUT', body: { path: 'new.txt', content: 'bad', base_hash: null } })).status, 403);
  });
  await step('WebSocket authenticates and local file changes arrive remotely', async () => {
    // The HTTP connection above checked the native fingerprint. This isolated
    // browser context accepts that same loopback test certificate for WSS.
    remoteBrowser = await chromium.launch({headless:true,channel:process.env.CI ? undefined : 'chrome'});
    const context = await remoteBrowser.newContext({ignoreHTTPSErrors:true});
    remotePage = await context.newPage(); await remotePage.goto(`https://${host.addr}/`);
    await remotePage.evaluate(token => new Promise((resolve,reject) => {
      window.fixtureEvents=[];
      const ws = new WebSocket(`wss://${location.host}/ws`); window.fixtureSocket=ws;
      const timeout=setTimeout(()=>reject(new Error('Remote WebSocket hello timeout')),5000);
      ws.onopen=()=>ws.send(JSON.stringify({token}));
      ws.onerror=()=>reject(new Error('Remote WebSocket connection failed'));
      ws.onmessage=event=>{const value=JSON.parse(event.data); window.fixtureEvents.push(value); if(value.type==='hello'){clearTimeout(timeout);resolve(value);} };
    }),viewer.token);
    fs.writeFileSync(path.join(project,'hello.txt'),'watched local change\n');
    await remotePage.waitForFunction(id => window.fixtureEvents.some(event=>event.type==='file_changed' && event.project===id && event.path==='hello.txt'),projectId,{timeout:5000});
  });
  await step('project-scoped remote device cannot receive another project agent output', async () => {
    assert.ok(remotePage,'WebSocket fixture must be connected'); replyMode='unshared';
    const conversation=await invoke('create_conversation',{projectId:otherProjectId,mode:'agent'});
    await invoke('send_message',{conversationId:conversation.id,text:'Answer using the private fixture marker',target:`model:${providerId}/boundary-model`,effort:null,mode:'agent'});
    await delay(200);
    const leaked=await remotePage.evaluate(()=>window.fixtureEvents.some(event=>JSON.stringify(event).includes('PRIVATE_UNSHARED_PROJECT_FIXTURE')));
    assert.equal(leaked,false,'Project-scoped client received unshared Agent output');
  });
  await step('revoked device loses HTTP access immediately', async () => {
    assert.equal(await invoke('remote_revoke', { deviceId: control.device_id }), true);
    assert.equal((await remote('/api/status', { token: control.token })).status, 401);
    assert.equal(await invoke('remote_revoke', { deviceId: viewer.device_id }), true);
    if(remotePage)await remotePage.waitForFunction(()=>window.fixtureEvents.some(event=>event.type==='revoked'),null,{timeout:5000});
  });
  await step('host stops and disables restart activation', async () => {
    await invoke('remote_stop'); assert.equal((await invoke('remote_status')).running, false); assert.equal((await invoke('prefs_get')).remote_enabled, false);
  });
  await step('emergency stop command responds with no active work', async () => {
    const result = await invoke('emergency_stop'); assert.equal(typeof result, 'object');
  });
  await step('project and settings survive native app restart', async () => {
    const exited = new Promise(resolve => app.once('exit', resolve));
    await invoke('quit_app').catch(() => {}); await within(exited, 10_000, 'Native app did not exit');
    await browser.close(); browser = undefined; invoke = undefined;
    app = launch(); await connect();
    const snapshot = await invoke('snapshot'); assert.equal(snapshot.projects.length, 2); assert.ok(snapshot.projects.some(item=>item.id===projectId));
    assert.equal(snapshot.settings.caveman, true); assert.equal((await invoke('remote_status')).running, false);
  });
} catch (error) { results.push({ name: 'harness setup', ok: false, error: String(error.message) }); }
finally {
  if (invoke) { await invoke('emergency_stop').catch(() => {}); await invoke('remove_provider', { id: providerId }).catch(() => {}); await invoke('secret_delete', { name: secretName }).catch(() => {}); await invoke('remote_stop').catch(() => {}); await invoke('quit_app').catch(() => {}); }
  await remoteBrowser?.close().catch(() => {}); await browser?.close().catch(() => {}); app.kill();
  provider.closeAllConnections(); await new Promise(resolve => provider.close(resolve));
  const report = { at: new Date().toISOString(), exe, exe_sha256: createHash('sha256').update(fs.readFileSync(exe)).digest('hex'), fixture: root, scope: 'Windows native IPC; local fixture packages, stdio MCP and fingerprint-pinned loopback TLS; no live provider or global hotkey proof', results };
  fs.writeFileSync(path.join(output, 'results.json'), JSON.stringify(report, null, 2));
  console.log(`${results.filter(item => item.ok).length}/${results.length} passed`);
  process.exitCode = results.every(item => item.ok) ? 0 : 1;
}
