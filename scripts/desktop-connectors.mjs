// Native check of one-click remote MCP connectors against the real app: a
// local stand-in connector that requires OAuth (MCP authorization spec:
// protected-resource metadata, dynamic client registration, PKCE), plus an
// MCP server that only answers with the issued token. The stand-in also plays
// the user's browser by approving the sign-in and redirecting back.
//
// Usage: node scripts/desktop-connectors.mjs [path-to-conductor-app.exe]
import { execFileSync } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { launch, stepper } from './lib/desktop.mjs';

const exe = path.resolve(process.argv[2] ?? 'target/claude/debug/conductor-app.exe');
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'conductor-connectors-'));
const results = [];
const step = stepper(results);
const seen = { registered: 0, pkce: false, resource: false, toolsListed: false };
const codes = new Map();
let token = null;

const json = (res, status, o, headers = {}) =>
  res.writeHead(status, { 'content-type': 'application/json', ...headers }).end(JSON.stringify(o));
const body = (req) => new Promise((r) => { let d = ''; req.on('data', (c) => (d += c)); req.on('end', () => r(d)); });

const server = http.createServer(async (req, res) => {
  const base = `http://127.0.0.1:${server.address().port}`;
  const url = new URL(req.url, base);
  if (url.pathname === '/mcp' && req.method === 'POST') {
    if (!token || req.headers.authorization !== `Bearer ${token}`)
      return json(res, 401, { error: 'unauthorized' }, { 'www-authenticate': `Bearer resource_metadata="${base}/.well-known/oauth-protected-resource"` });
    const msg = JSON.parse(await body(req));
    if (msg.id === undefined) return res.writeHead(202).end();
    if (msg.method === 'initialize')
      return json(res, 200, { jsonrpc: '2.0', id: msg.id, result: { protocolVersion: '2025-06-18', capabilities: { tools: {} }, serverInfo: { name: 'stand-in', version: '1' } } });
    if (msg.method === 'tools/list') {
      seen.toolsListed = true;
      return json(res, 200, { jsonrpc: '2.0', id: msg.id, result: { tools: [{ name: 'search_issues', description: 'Search', inputSchema: { type: 'object' } }] } });
    }
    return json(res, 200, { jsonrpc: '2.0', id: msg.id, error: { code: -32601, message: 'no' } });
  }
  if (url.pathname === '/.well-known/oauth-protected-resource')
    return json(res, 200, { resource: `${base}/mcp`, authorization_servers: [base], scopes_supported: ['issues:read'] });
  if (url.pathname === '/.well-known/oauth-authorization-server')
    return json(res, 200, { issuer: base, authorization_endpoint: `${base}/authorize`, token_endpoint: `${base}/token`, registration_endpoint: `${base}/register` });
  if (url.pathname === '/register' && req.method === 'POST') {
    const r = JSON.parse(await body(req));
    if (r.token_endpoint_auth_method !== 'none' || !r.redirect_uris?.[0]?.startsWith('http://127.0.0.1:')) return json(res, 400, {});
    seen.registered += 1;
    return json(res, 201, { client_id: `client-${seen.registered}` });
  }
  if (url.pathname === '/authorize') {
    const q = url.searchParams;
    const code = crypto.randomUUID();
    codes.set(code, { challenge: q.get('code_challenge'), client: q.get('client_id'), resource: q.get('resource') });
    const back = new URL(q.get('redirect_uri'));
    back.searchParams.set('code', code);
    back.searchParams.set('state', q.get('state'));
    return res.writeHead(302, { location: back.toString() }).end();
  }
  if (url.pathname === '/token' && req.method === 'POST') {
    const f = Object.fromEntries(new URLSearchParams(await body(req)));
    const c = codes.get(f.code);
    if (!c || crypto.createHash('sha256').update(f.code_verifier).digest('base64url') !== c.challenge || c.client !== f.client_id)
      return json(res, 400, { error: 'invalid_grant' });
    seen.pkce = true;
    seen.resource = f.resource === `${base}/mcp`;
    token = `tok-${crypto.randomUUID()}`;
    return json(res, 200, { access_token: token, refresh_token: 'r1', expires_in: 3600, token_type: 'Bearer' });
  }
  json(res, 404, {});
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const base = `http://127.0.0.1:${server.address().port}`;

// The test hook makes the app follow the sign-in redirect itself instead of
// opening the system browser.
process.env.CONDUCTOR_TEST_SUBSCRIPTIONS = '1';
process.env.CONDUCTOR_TEST_SUBSCRIPTION_BASE = base;
const app = await launch(exe, path.join(tmp, 'data'));
const { page, invoke } = app;
let code = 0;
try {
  await step('app starts', async () => {
    await page.getByRole('dialog', { name: 'Set up Conductor' }).waitFor({ timeout: 30_000 });
  });
  await step('connect by URL: discover, register, PKCE sign-in, token in keychain, tools listed', async () => {
    const d = await invoke('mcp_connect_remote', { name: 'standin', url: `${base}/mcp` });
    if (d.status !== 'connected') throw new Error(JSON.stringify(d));
    if (seen.registered !== 1 || !seen.pkce || !seen.resource || !seen.toolsListed) throw new Error(JSON.stringify(seen));
    const cfg = await invoke('mcp_list');
    const s = cfg.servers.find((x) => x.name === 'standin');
    const auth = s.transport.headers?.Authorization;
    if (!auth?.secret) throw new Error(`token not stored as a keychain reference: ${JSON.stringify(s)}`);
    if (JSON.stringify(cfg).includes(token)) throw new Error('token leaked into the config file');
  });
  await step('doctor stays connected with the stored token', async () => {
    const [d] = await invoke('mcp_doctor', { name: 'standin' });
    if (d.status !== 'connected' || !d.tools.includes('search_issues')) throw new Error(JSON.stringify(d));
  });
  await step('sign in again issues a fresh token', async () => {
    const old = token;
    const d = await invoke('mcp_sign_in', { name: 'standin' });
    if (d.status !== 'connected' || token === old) throw new Error(JSON.stringify(d));
  });
} catch {
  code = 1;
} finally {
  await invoke('mcp_remove', { name: 'standin' }).catch(() => {});
  for (const n of ['mcp-oauth-standin', 'mcp-oauth-standin-refresh']) await invoke('secret_delete', { name: n }).catch(() => {});
  await app.browser.close().catch(() => {});
  try {
    execFileSync('taskkill', ['/F', '/T', '/PID', String(app.proc.pid)], { stdio: 'ignore' });
  } catch {}
  server.close();
  console.log(code === 0 ? '\nConnectors: all steps passed' : '\nConnectors: FAILED');
  process.exit(code);
}
