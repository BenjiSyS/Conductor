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

const output = path.resolve('target/native-boundary');
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
// The app's own stdout/stderr are kept for diagnosis when start-up fails.
const appOut = path.join(output, 'app-stdout.log');
const appErr = path.join(output, 'app-stderr.log');
function launch() { return spawn(exe, [], {
  env: { ...process.env, CONDUCTOR_DATA_DIR: data, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`, RUST_LOG: process.env.RUST_LOG ?? 'info' },
  stdio: ['ignore', fs.openSync(appOut, 'a'), fs.openSync(appErr, 'a')], windowsHide: true,
}); }
// cargo build's debug app uses the configured devUrl. CI has no Vite process;
// serve the already-built frontend there without HMR/dependency reloads.
async function prepareFrontend() {
  const ready = async () => {
    try {
      const response = await fetch('http://127.0.0.1:1420/', { signal: AbortSignal.timeout(1000) });
      if (!response.ok) return false;
      const reader = response.body.getReader(); let bytes = 0; const chunks = [];
      while (true) { const { done, value } = await reader.read(); if (done) break; bytes += value.length; if (bytes > 256 * 1024) { await reader.cancel(); return false; } chunks.push(value); }
      return Buffer.concat(chunks).toString('utf8').includes('<title>Conductor</title>');
    } catch { return false; }
  };
  if (await ready()) return undefined;
  assert.ok(fs.existsSync('apps/desktop/dist/index.html'), 'Build frontend first: npm run build');
  const frontend = spawn(process.execPath, [path.resolve('node_modules/vite/bin/vite.js'), 'preview', '--host', '127.0.0.1', '--port', '1420', '--strictPort'], { stdio: 'ignore', windowsHide: true });
  try {
    for (let attempt = 0; attempt < 60; attempt++) {
      if (frontend.exitCode !== null) throw new Error(`Frontend preview exited: ${frontend.exitCode}; check port 1420`);
      if (await ready()) return frontend;
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    throw new Error('Built frontend preview did not start on port 1420');
  } catch (error) { frontend.kill(); throw error; }
}
const frontend = await prepareFrontend();
const frontendEvidence = {
  mode: frontend ? 'owned-built-preview' : 'reused-existing-server',
  url: 'http://127.0.0.1:1420/',
  owns_process: Boolean(frontend),
};
let app = launch();
const results = [];
let browser, remoteBrowser, remotePage, invoke, projectId, otherProjectId, host, control, viewer;
const secretName = `boundary-${randomUUID()}`;
const sentinel = `sk-fixture-${randomUUID().replaceAll('-', '')}`;
const providerId = `boundary-${randomUUID()}`;
const integrationSecretName = `runtime-${randomUUID()}`;
const integrationSecretValue = `opaque-native-mcp-${randomUUID()}`;
const runtimeRequests = [];
const mcpRequests = [];
let runtimeReply;
let replyMode = 'stream';
let streamReady;
const provider = http.createServer((request, response) => {
  if (request.url === '/mcp') {
    if (request.headers['x-fixture-token'] !== integrationSecretValue) { response.writeHead(401); response.end('{}'); return; }
    let body = ''; request.on('data', chunk => { body += chunk; }); request.on('end', () => {
      const message = JSON.parse(body); mcpRequests.push(message.method);
      if (message.id === undefined) { response.writeHead(202); response.end(); return; }
      const result = message.method === 'initialize'
        ? { protocolVersion: '2025-06-18', capabilities: { tools: {} }, serverInfo: { name: 'native-http-fixture', version: '1' } }
        : message.method === 'tools/list'
          ? { tools: [{ name: 'fixture_echo', description: 'Fixture only', inputSchema: { type: 'object' } }] }
          : { content: [{ type: 'text', text: `NATIVE_HTTP_MCP_MARKER ${integrationSecretValue}` }] };
      response.setHeader('content-type', 'application/json'); response.end(JSON.stringify({ jsonrpc: '2.0', id: message.id, result }));
    }); return;
  }
  if (request.headers.authorization !== `Bearer ${sentinel}`) { response.writeHead(401); response.end('{}'); return; }
  if (request.url === '/v1/models') { response.setHeader('content-type', 'application/json'); response.end(JSON.stringify({ data: [{ id: 'boundary-model' }] })); return; }
  if (request.url !== '/v1/chat/completions') { response.writeHead(404); response.end('{}'); return; }
  let body = ''; request.on('data', chunk => { body += chunk; }); request.on('end', () => {
    response.setHeader('content-type', 'text/event-stream');
    const message = JSON.parse(body); if (runtimeReply) runtimeRequests.push(message);
    let text;
    try { text = runtimeReply ? runtimeReply(message) : replyMode === 'unshared' ? '<done>PRIVATE_UNSHARED_PROJECT_FIXTURE</done>' : replyMode === 'approval' ? '<write_file path="should-not-exist.txt">bad</write_file>' : 'partial native fixture'; }
    catch (error) { response.writeHead(500, { 'content-type': 'application/json' }); response.end(JSON.stringify({ error: { message: error.message } })); return; }
    response.write(`data: ${JSON.stringify({ choices: [{ delta: { content: text } }] })}\n\n`);
    if (runtimeReply || replyMode !== 'stream') response.end('data: [DONE]\n\n');
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
    const read = (f) => (fs.existsSync(f) ? fs.readFileSync(f, 'utf8').slice(-1500) : '');
    const logs = path.join(data, 'logs');
    const appLog = fs.existsSync(logs) ? fs.readdirSync(logs).map((f) => read(path.join(logs, f))).join(' | ') : 'no app log';
    const listing = fs.existsSync(data) ? fs.readdirSync(data).join(', ') : 'data dir missing';
    let webviews = 'unknown';
    try {
      webviews = String(execFileSync('tasklist', ['/FI', 'IMAGENAME eq msedgewebview2.exe', '/NH'], { encoding: 'utf8' }).split('\n').filter((l) => /msedgewebview2/i.test(l)).length);
    } catch {}
    throw new Error(
      `WebView2 debugging endpoint did not open after 120 s. app alive: ${app.exitCode === null}; WebView2 processes: ${webviews}; data dir: [${listing}]; app log: ${appLog}; stderr: ${read(appErr)}; stdout: ${read(appOut)}`,
    );
  }
  const page = browser.contexts()[0].pages()[0];
  await page.waitForFunction(() => !!window.__TAURI_INTERNALS__ && location.origin !== 'null', null, { timeout: 30_000 });
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
  await step('native Agent uses scoped stdio MCP, OS credential and selected skill', async () => {
    const settings = (await invoke('snapshot')).settings;
    try {
      await invoke('save_settings', { settings: { ...settings, permission: 'full_access' } });
      await invoke('secret_set', { name: integrationSecretName, value: integrationSecretValue });
      const source = path.join(root, 'runtime-skill'); fs.mkdirSync(source);
      fs.writeFileSync(path.join(source, 'SKILL.md'), '---\nname: boundary-runtime-skill\ndescription: Native runtime fixture\n---\nNATIVE_SELECTED_SKILL_MARKER\n');
      await invoke('skills_install', { source });
      const script = path.join(root, 'runtime-mcp.cjs');
      fs.writeFileSync(script, `const fs=require('fs');const rl=require('readline').createInterface({input:process.stdin});rl.on('line',line=>{const m=JSON.parse(line);if(m.id===undefined)return;let result;if(m.method==='initialize')result={protocolVersion:m.params.protocolVersion,capabilities:{tools:{}},serverInfo:{name:'runtime',version:'1'}};else if(m.method==='tools/list')result={tools:[{name:'fixture_echo',description:'Fixture',inputSchema:{type:'object'}}]};else if(m.method==='tools/call')result={content:[{type:'text',text:'NATIVE_STDIO_MCP_MARKER '+process.env.FIXTURE_TOKEN}]};process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:m.id,result})+'\\n');});`);
      await invoke('mcp_add_custom', { server: { name: 'runtime-stdio', project: projectId, providers: ['openai'], transport: { type: 'stdio', command: process.execPath, args: [script], env: { FIXTURE_TOKEN: { secret: integrationSecretName } } }, enabled: true } });
      let turn = 0;
      runtimeReply = message => {
        assert.ok(!JSON.stringify(message).includes(integrationSecretValue), 'Integration credential reached model request');
        if (turn++ === 0) { assert.ok(JSON.stringify(message).includes('NATIVE_SELECTED_SKILL_MARKER')); return '<mcp_call server="runtime-stdio" tool="fixture_echo">{"text":"hello"}</mcp_call>'; }
        assert.ok(JSON.stringify(message).includes('NATIVE_STDIO_MCP_MARKER')); return '<done>NATIVE_STDIO_RUNTIME_DONE</done>';
      };
      const conversation = await invoke('create_conversation', { projectId, mode: 'agent' });
      await within(invoke('send_message', { conversationId: conversation.id, text: 'Use boundary-runtime-skill and MCP fixture', target: `model:${providerId}/boundary-model`, mode: 'agent', effort: null }), 15_000, 'Native MCP Agent did not finish');
      const saved = (await invoke('snapshot')).conversations.find(item => item.id === conversation.id);
      assert.equal(saved.messages.at(-1).status, 'complete'); assert.ok(saved.messages.at(-1).text.includes('NATIVE_STDIO_RUNTIME_DONE')); assert.equal(turn, 2);
    } finally {
      runtimeReply = undefined; await invoke('emergency_stop'); await invoke('mcp_remove', { name: 'runtime-stdio' }).catch(() => {});
      await invoke('skills_remove', { name: 'boundary-runtime-skill' }).catch(() => {}); await invoke('save_settings', { settings });
    }
  });
  await step('native Agent HTTP MCP round trip protects integration credential', async () => {
    const settings = (await invoke('snapshot')).settings;
    try {
      await invoke('save_settings', { settings: { ...settings, permission: 'full_access' } });
      await invoke('secret_set', { name: integrationSecretName, value: integrationSecretValue });
      await invoke('mcp_add_custom', { server: { name: 'runtime-http', project: projectId, providers: ['openai'], transport: { type: 'http', url: `http://127.0.0.1:${providerPort}/mcp`, headers: { 'x-fixture-token': { secret: integrationSecretName } } }, enabled: true } });
      let turn = 0;
      runtimeReply = message => {
        assert.ok(!JSON.stringify(message).includes(integrationSecretValue), 'HTTP MCP credential reached model request');
        if (turn++ === 0) return '<mcp_call server="runtime-http" tool="fixture_echo">{}</mcp_call>';
        assert.ok(JSON.stringify(message).includes('NATIVE_HTTP_MCP_MARKER')); return '<done>NATIVE_HTTP_RUNTIME_DONE</done>';
      };
      const conversation = await invoke('create_conversation', { projectId, mode: 'agent' });
      await within(invoke('send_message', { conversationId: conversation.id, text: 'Use the HTTP MCP fixture', target: `model:${providerId}/boundary-model`, mode: 'agent', effort: null }), 15_000, 'Native HTTP MCP Agent did not finish');
      assert.equal(turn, 2); assert.ok(mcpRequests.includes('tools/call'));
    } finally { runtimeReply = undefined; await invoke('emergency_stop'); await invoke('mcp_remove', { name: 'runtime-http' }).catch(() => {}); await invoke('save_settings', { settings }); }
  });
  await step('native Agent invokes installed plugin with literal arguments and instructions', async () => {
    const settings = (await invoke('snapshot')).settings;
    const literal = 'literal $(echo unsafe); & | value';
    try {
      await invoke('save_settings', { settings: { ...settings, permission: 'full_access' } });
      const source = path.join(root, 'runtime-plugin'); fs.mkdirSync(source);
      const script = path.join(root, 'runtime-plugin.cjs'); fs.writeFileSync(script, 'process.stdout.write("NATIVE_PLUGIN_MARKER "+JSON.stringify(process.argv[2]));');
      fs.writeFileSync(path.join(source, 'instructions.md'), 'NATIVE_PLUGIN_INSTRUCTIONS_MARKER\n');
      fs.writeFileSync(path.join(source, 'plugin.toml'), `name = "boundary-runtime-plugin"\nversion = "1.0.0"\ndescription = "Native runtime fixture"\npermissions = ["terminal.execute"]\ninstructions = "instructions.md"\n[[tools]]\nname = "echo"\ndescription = "Echo literal input"\nargv = [${JSON.stringify(process.execPath)}, ${JSON.stringify(script)}, "{text}"]\n[tools.params]\ntext = "literal string"\n`);
      await invoke('plugins_install', { source });
      let turn = 0;
      runtimeReply = message => {
        if (turn++ === 0) { assert.ok(JSON.stringify(message).includes('NATIVE_PLUGIN_INSTRUCTIONS_MARKER')); return `<plugin_call plugin="boundary-runtime-plugin" tool="echo">${JSON.stringify({ text: literal })}</plugin_call>`; }
        const encoded = JSON.stringify(message); assert.ok(encoded.includes('NATIVE_PLUGIN_MARKER')); assert.ok(encoded.includes('literal $(echo unsafe); & | value')); return '<done>NATIVE_PLUGIN_RUNTIME_DONE</done>';
      };
      const conversation = await invoke('create_conversation', { projectId, mode: 'agent' });
      await within(invoke('send_message', { conversationId: conversation.id, text: 'Use boundary-runtime-plugin echo', target: `model:${providerId}/boundary-model`, mode: 'agent', effort: null }), 15_000, 'Native plugin Agent did not finish');
      assert.equal(turn, 2);
    } finally { runtimeReply = undefined; await invoke('emergency_stop'); await invoke('plugins_remove', { name: 'boundary-runtime-plugin' }).catch(() => {}); await invoke('save_settings', { settings }); }
  });
  await step('native Agent executes installed skill script with literal argv and project cwd', async () => {
    const settings = (await invoke('snapshot')).settings;
    try {
      await invoke('save_settings', { settings: { ...settings, permission: 'full_access' } });
      const source = path.join(root, 'runtime-command-skill'); fs.mkdirSync(path.join(source, 'scripts'), { recursive: true });
      const literal = 'literal $(echo unsafe); & | value';
      fs.writeFileSync(path.join(source, 'SKILL.md'), 'NATIVE_SKILL_COMMAND_INSTRUCTIONS_MARKER\n');
      fs.writeFileSync(path.join(source, 'scripts', 'echo.cjs'), 'const fs=require("fs"); fs.writeFileSync("native-skill-output.json",JSON.stringify({cwd:process.cwd(),arg:process.argv[2]})); process.stdout.write("NATIVE_SKILL_COMMAND_MARKER");');
      fs.writeFileSync(path.join(source, 'skill.toml'), `name="boundary-runtime-command"\nversion="1"\ndescription="Native skill command fixture"\npermissions=["terminal.execute","filesystem.write"]\n[[commands]]\nname="echo"\ndescription="Fixture script"\nargv=[${JSON.stringify(process.execPath)},"{skill_dir}/scripts/echo.cjs",${JSON.stringify(literal)}]\n`);
      await invoke('skills_install', { source });
      let turn = 0;
      runtimeReply = message => {
        const encoded = JSON.stringify(message);
        if (turn++ === 0) {
          assert.ok(encoded.includes('NATIVE_SKILL_COMMAND_INSTRUCTIONS_MARKER'));
          assert.ok(encoded.includes('boundary-runtime-command:echo'));
          return '<skill_call skill="boundary-runtime-command" command="echo">{}</skill_call>';
        }
        assert.ok(encoded.includes('NATIVE_SKILL_COMMAND_MARKER'));
        return '<done>NATIVE_SKILL_COMMAND_DONE</done>';
      };
      const conversation = await invoke('create_conversation', { projectId, mode: 'agent' });
      await within(invoke('send_message', { conversationId: conversation.id, text: 'Use boundary-runtime-command echo', target: `model:${providerId}/boundary-model`, mode: 'agent', effort: null }), 15_000, 'Native skill command Agent did not finish');
      assert.equal(turn, 2);
      const output = JSON.parse(fs.readFileSync(path.join(project, 'native-skill-output.json'), 'utf8'));
      assert.equal(output.arg, literal);
      assert.equal(fs.realpathSync(output.cwd), fs.realpathSync(project));
    } finally {
      runtimeReply = undefined; await invoke('emergency_stop');
      await invoke('skills_remove', { name: 'boundary-runtime-command' }).catch(() => {});
      await invoke('save_settings', { settings });
    }
  });
  await step('native Goal uses scoped MCP and skill before a verified file write', async () => {
    const settings = (await invoke('snapshot')).settings;
    let goalId;
    try {
      await invoke('save_settings', { settings: { ...settings, permission: 'full_access' } });
      await invoke('secret_set', { name: integrationSecretName, value: integrationSecretValue });
      await invoke('mcp_add_custom', { server: { name: 'goal-http', project: projectId, providers: ['openai'], transport: { type: 'http', url: `http://127.0.0.1:${providerPort}/mcp`, headers: { 'x-fixture-token': { secret: integrationSecretName } } }, enabled: true } });
      const source = path.join(root, 'goal-runtime-skill'); fs.mkdirSync(source);
      fs.writeFileSync(path.join(source, 'skill.toml'), 'name="boundary-native-goal"\nversion="1"\ndescription="Native Goal fixture"\nkeywords=["boundary-native-goal"]\nmcp=["goal-http"]\n');
      fs.writeFileSync(path.join(source, 'SKILL.md'), 'NATIVE_GOAL_SKILL_MARKER\n');
      await invoke('skills_install', { source });
      let turn = 0;
      runtimeReply = message => {
        const encoded = JSON.stringify(message);
        assert.ok(!encoded.includes(integrationSecretValue), 'Goal integration credential reached model');
        const current = turn++;
        if (current === 0) return JSON.stringify({ tasks: [{ id: 'code', title: 'Use boundary-native-goal to write fixture file', role: 'coder', important: false }] });
        assert.ok(encoded.includes('NATIVE_GOAL_SKILL_MARKER'), 'Goal did not select installed skill');
        if (current === 1) return '<mcp_call server="goal-http" tool="fixture_echo">{}</mcp_call>';
        assert.ok(encoded.includes('NATIVE_HTTP_MCP_MARKER'), 'Goal did not receive scoped MCP output');
        if (current === 2) return '<write_file path="native-goal-mcp.js">console.log("NATIVE_GOAL_FILE_MARKER");</write_file>';
        assert.equal(current, 3, 'Goal unexpectedly continued after done');
        return '<done>Native Goal MCP file written.</done>';
      };
      const record = await invoke('goal_create', { goal: { project_id: projectId, objective: 'Use boundary-native-goal MCP and write native-goal-mcp.js', combo_id: `model:${providerId}/boundary-model`, checks: ['node --check native-goal-mcp.js'] } });
      goalId = record.goal.id;
      await invoke('goal_start', { id: goalId });
      const completed = await within((async () => {
        for (;;) {
          const current = await invoke('goal_get', { id: goalId });
          if (['complete', 'blocked', 'paused'].includes(current.goal.state)) return current;
          await delay(50);
        }
      })(), 15_000, 'Native MCP Goal did not finish');
      assert.equal(completed.goal.state, 'complete', JSON.stringify(completed.goal.notes));
      assert.ok(completed.goal.checks.some(check => check.passed), 'Goal Test Gate did not pass');
      assert.ok(fs.readFileSync(path.join(project, 'native-goal-mcp.js'), 'utf8').includes('NATIVE_GOAL_FILE_MARKER'));
      assert.equal(turn, 4);
    } finally {
      if (goalId) await invoke('goal_stop', { id: goalId }).catch(() => {});
      runtimeReply = undefined; await invoke('emergency_stop');
      await invoke('mcp_remove', { name: 'goal-http' }).catch(() => {});
      await invoke('skills_remove', { name: 'boundary-native-goal' }).catch(() => {});
      await invoke('save_settings', { settings });
    }
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
  await step('Fast Stop preserves a streamed Agent reply', async () => {
    replyMode='stream';
    const conversation=await invoke('create_conversation',{projectId,mode:'agent'});
    const ready=new Promise(resolve=>{streamReady=resolve;});
    const run=invoke('send_message',{conversationId:conversation.id,text:'Stream an Agent fixture reply',target:`model:${providerId}/boundary-model`,effort:null,mode:'agent'}).catch(()=>{});
    try {
      await within(ready,10_000,'Agent provider request did not start');await delay(100);
      await invoke('stop',{conversationId:conversation.id});await within(run,2000,'Stop did not cancel Agent stream');
      const saved=(await invoke('snapshot')).conversations.find(item=>item.id===conversation.id);
      assert.equal(saved.messages.at(-1).status,'stopped');assert.equal(saved.messages.at(-1).text,'partial native fixture');
    } finally {streamReady=undefined;await invoke('emergency_stop');await within(run,2000,'Emergency cleanup did not stop Agent stream');}
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
    const allowed=await invoke('create_conversation',{projectId,mode:'agent'});
    await invoke('send_message',{conversationId:allowed.id,text:'Answer using the allowed fixture marker',target:`model:${providerId}/boundary-model`,effort:null,mode:'agent'});
    await remotePage.waitForFunction(id=>window.fixtureEvents.some(event=>event.project===id && JSON.stringify(event).includes('PRIVATE_UNSHARED_PROJECT_FIXTURE')),projectId,{timeout:5000});
    await remotePage.evaluate(()=>{window.fixtureEvents=[];});
    const conversation=await invoke('create_conversation',{projectId:otherProjectId,mode:'agent'});
    await invoke('send_message',{conversationId:conversation.id,text:'Answer using the private fixture marker',target:`model:${providerId}/boundary-model`,effort:null,mode:'agent'});
    await delay(200);
    const leaked=await remotePage.evaluate(()=>window.fixtureEvents.some(event=>JSON.stringify(event).includes('PRIVATE_UNSHARED_PROJECT_FIXTURE')));
    assert.equal(leaked,false,'Project-scoped client received unshared Agent output');
  });
  await step('remote Stop requires scope and cancels only the authorized project', async () => {
    const context=await remoteBrowser.newContext({ignoreHTTPSErrors:true});
    const controller=await context.newPage(); await controller.goto(`https://${host.addr}/`);
    await controller.evaluate(token=>new Promise((resolve,reject)=>{
      window.fixtureEvents=[]; const ws=new WebSocket(`wss://${location.host}/ws`); window.fixtureSocket=ws;
      const timer=setTimeout(()=>reject(new Error('Control WebSocket hello timeout')),5000);
      ws.onopen=()=>ws.send(JSON.stringify({token})); ws.onerror=()=>reject(new Error('Control WebSocket failed'));
      ws.onmessage=event=>{const value=JSON.parse(event.data);window.fixtureEvents.push(value);if(value.type==='hello'){clearTimeout(timer);resolve();}};
    }),control.token);
    let run;
    try {
      replyMode='stream';
      const conversation=await invoke('create_conversation',{projectId:otherProjectId,mode:'chat'});
      const ready=new Promise(resolve=>{streamReady=resolve;}); let finished=false;
      run=invoke('send_message',{conversationId:conversation.id,text:'Keep private fixture stream active',target:`model:${providerId}/boundary-model`,effort:null,mode:'chat'}).then(()=>{finished=true;},()=>{finished=true;});
      await within(ready,10_000,'Private provider request did not start');
      await controller.evaluate(project=>window.fixtureSocket.send(JSON.stringify({type:'stop',project})),projectId);
      await delay(300); assert.equal(finished,false,'Authorized project Stop cancelled work in another project');
      for(const message of [{type:'stop',project:otherProjectId},{type:'stop'}]) {
        await controller.evaluate(message=>{window.fixtureEvents=[];window.fixtureSocket.send(JSON.stringify(message));},message);
        await controller.waitForFunction(()=>window.fixtureEvents.some(event=>event.type==='error'),null,{timeout:3000});
        assert.equal(finished,false,'Rejected remote Stop cancelled private work');
      }
      await invoke('stop',{conversationId:conversation.id}); await within(run,2000,'Local cleanup did not stop private stream');
      const allowed=await invoke('create_conversation',{projectId,mode:'chat'});
      const allowedReady=new Promise(resolve=>{streamReady=resolve;});
      run=invoke('send_message',{conversationId:allowed.id,text:'Keep authorized fixture stream active',target:`model:${providerId}/boundary-model`,effort:null,mode:'chat'}).catch(()=>{});
      await within(allowedReady,10_000,'Authorized provider request did not start');
      await controller.evaluate(project=>window.fixtureSocket.send(JSON.stringify({type:'stop',project})),projectId);
      await within(run,2000,'Authorized remote Stop did not cancel its project');
      assert.equal((await invoke('snapshot')).conversations.find(item=>item.id===allowed.id).messages.at(-1).status,'stopped');
    } finally {streamReady=undefined;await invoke('emergency_stop');if(run)await within(run,2000,'Control cleanup did not stop stream');await context.close();}
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
  if (invoke) { await invoke('emergency_stop').catch(() => {}); await invoke('remove_provider', { id: providerId }).catch(() => {}); await invoke('secret_delete', { name: secretName }).catch(() => {}); await invoke('secret_delete', { name: integrationSecretName }).catch(() => {}); await invoke('remote_stop').catch(() => {}); await invoke('quit_app').catch(() => {}); }
  await remoteBrowser?.close().catch(() => {}); await browser?.close().catch(() => {}); app.kill();
  frontend?.kill();
  provider.closeAllConnections(); await new Promise(resolve => provider.close(resolve));
  const report = { at: new Date().toISOString(), exe, exe_sha256: createHash('sha256').update(fs.readFileSync(exe)).digest('hex'), fixture: root, frontend: frontendEvidence, scope: 'Windows native IPC; local provider, stdio/HTTP MCP, installed skill/plugin and Goal fixtures, OS vault and fingerprint-pinned loopback TLS; no live account or global hotkey proof', results };
  fs.writeFileSync(path.join(output, 'results.json'), JSON.stringify(report, null, 2));
  console.log(`${results.filter(item => item.ok).length}/${results.length} passed`);
  process.exitCode = results.every(item => item.ok) ? 0 : 1;
}
