// Native check of the Usage view against the real app: provider rate-limit
// headers from a real HTTP response, and the full opt-in subscription
// sign-in (PKCE loopback, token exchange, OS keychain, refresh, usage) for
// Claude, ChatGPT and Gemini against a local stand-in for each provider.
//
// The stand-in plays the browser too: its authorize endpoint redirects
// straight back to the app's loopback callback, the way a provider does after
// the user signs in. Real provider sign-in still needs a person at a browser.
//
// Usage: node scripts/desktop-usage.mjs [path-to-conductor-app.exe]
import { execFileSync } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { connectProvider, launch, shots, startMock, stepper } from './lib/desktop.mjs';

const exe = path.resolve(process.argv[2] ?? 'target/debug/conductor-app.exe');
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'conductor-usage-'));
const results = [];
const step = stepper(results);

const b64 = (o) => Buffer.from(JSON.stringify(o)).toString('base64url');
const jwt = (claims) => `e30.${b64(claims)}.sig`;
const seen = { verifier: {}, refreshed: {} };
const codes = new Map(); // code -> { service, challenge }

function body(req) {
  return new Promise((r) => {
    let d = '';
    req.on('data', (c) => (d += c));
    req.on('end', () => r(d));
  });
}
const json = (res, status, o) => res.writeHead(status, { 'content-type': 'application/json' }).end(JSON.stringify(o));

const provider = http.createServer(async (req, res) => {
  const url = new URL(req.url, 'http://127.0.0.1');
  const [, service, ...rest] = url.pathname.split('/');
  const route = rest.join('/');
  if (route === 'authorize') {
    const q = url.searchParams;
    if (q.get('code_challenge_method') !== 'S256' || !q.get('code_challenge') || !q.get('state'))
      return json(res, 400, { error: 'pkce required' });
    const code = crypto.randomUUID();
    codes.set(code, { service, challenge: q.get('code_challenge') });
    const back = new URL(q.get('redirect_uri'));
    back.searchParams.set('code', code);
    back.searchParams.set('state', q.get('state'));
    return res.writeHead(302, { location: back.toString() }).end();
  }
  if (route === 'token') {
    const raw = await body(req);
    const f = req.headers['content-type']?.includes('json')
      ? JSON.parse(raw)
      : Object.fromEntries(new URLSearchParams(raw));
    if (f.grant_type === 'authorization_code') {
      const c = codes.get(f.code);
      codes.delete(f.code);
      const ok =
        c &&
        c.service === service &&
        crypto.createHash('sha256').update(f.code_verifier).digest('base64url') === c.challenge;
      if (!ok) return json(res, 400, { error: 'invalid_grant', error_description: 'PKCE verification failed' });
      seen.verifier[service] = true;
    } else if (f.grant_type === 'refresh_token') {
      if (f.refresh_token !== `rt-${service}`) return json(res, 400, { error: 'invalid_grant' });
      seen.refreshed[service] = true;
    } else return json(res, 400, { error: 'unsupported_grant_type' });
    const email = `${service}@example.com`;
    return json(res, 200, {
      access_token: `at-${service}`,
      refresh_token: `rt-${service}`,
      // Short-lived so the app has to refresh once.
      expires_in: f.grant_type === 'authorization_code' ? 1 : 3600,
      account: { email_address: email },
      id_token: jwt({
        email,
        'https://api.openai.com/auth': { chatgpt_plan_type: 'plus', chatgpt_account_id: 'acct-1' },
      }),
    });
  }
  const authed = req.headers.authorization === `Bearer at-${service}`;
  if (!authed) return json(res, 401, {});
  if (service === 'claude' && route === 'api/oauth/usage')
    return json(res, 200, {
      five_hour: { utilization: 42, resets_at: new Date(Date.now() + 7e6).toISOString() },
      seven_day: { utilization: 10 },
    });
  if (service === 'chatgpt' && route === 'backend-api/wham/usage') {
    if (req.headers['chatgpt-account-id'] !== 'acct-1') return json(res, 400, {});
    return json(res, 200, {
      plan_type: 'plus',
      rate_limit: { primary_window: { used_percent: 7, limit_window_seconds: 18000, reset_after_seconds: 900 } },
    });
  }
  if (service === 'gemini' && route === 'v1internal:loadCodeAssist')
    return json(res, 200, { cloudaicompanionProject: 'p-1', currentTier: { name: 'free-tier' } });
  if (service === 'gemini' && route === 'v1internal:retrieveUserQuota')
    return json(res, 200, { buckets: [{ modelId: 'gemini-2.5-pro', remainingFraction: 0.6 }] });
  json(res, 404, {});
});
await new Promise((r) => provider.listen(0, '127.0.0.1', r));
const base = `http://127.0.0.1:${provider.address().port}`;

const mockPort = 18093;
const mock = await startMock(mockPort);
process.env.CONDUCTOR_TEST_SUBSCRIPTIONS = '1';
process.env.CONDUCTOR_TEST_SUBSCRIPTION_BASE = base;
// Gemini sign-in needs a configured Google OAuth client.
process.env.CONDUCTOR_GEMINI_OAUTH_CLIENT_ID = 'test-client.apps.example';
process.env.CONDUCTOR_GEMINI_OAUTH_CLIENT_SECRET = 'test-client-secret';
const app = await launch(exe, path.join(tmp, 'data'));
const { page, invoke } = app;
let code = 0;
try {
  await step('app starts', async () => {
    await page.getByRole('dialog', { name: 'Set up Conductor' }).waitFor({ timeout: 30_000 });
  });

  await step('API rate limits come from real response headers', async () => {
    const p = await connectProvider(invoke, 'usage-mock', 'Mock', `http://127.0.0.1:${mockPort}/v1`);
    const project = path.join(tmp, 'proj');
    fs.mkdirSync(project);
    await invoke('open_project', { path: project });
    const snap = await invoke('snapshot');
    const conv = await invoke('create_conversation', { projectId: snap.projects[0].id, mode: 'chat' });
    const done = page.evaluate(
      () =>
        new Promise((resolve) => {
          const t = setInterval(async () => {
            const s = await window.__TAURI_INTERNALS__.invoke('usage_overview');
            const prov = s.providers.find((x) => x.id === 'usage-mock');
            if (prov?.limits && prov.session.requests > 0) {
              clearInterval(t);
              resolve(prov);
            }
          }, 200);
        }),
    );
    await invoke('send_message', {
      conversationId: conv.id,
      text: 'hello',
      target: `model:${p.id}/${p.models[0].id}`,
      effort: null,
      mode: 'chat',
    });
    const prov = await Promise.race([
      done,
      new Promise((_, rej) => setTimeout(() => rej(new Error('no limits recorded')), 20_000)),
    ]);
    if (
      prov.limits.requests.limit !== 500 ||
      prov.limits.tokens.remaining !== 29500 ||
      prov.limits.tokens.reset !== '1s'
    )
      throw new Error(JSON.stringify(prov.limits));
  });

  await step('model lists stay current: new models appear, retired ones disappear', async () => {
    const setModels = (ids) => fetch(`http://127.0.0.1:${mockPort}/__models`, { method: 'POST', body: JSON.stringify({ ids }) });
    await setModels(['mock-coder', 'mock-brand-new']);
    const changed = await invoke('models_refresh');
    const p = (await invoke('snapshot')).providers.find((x) => x.id === 'usage-mock');
    const ids = p.models.map((m) => m.id);
    if (changed !== 1 || !ids.includes('mock-brand-new') || ids.includes('mock-reviewer')) throw new Error(`${changed} ${ids}`);
    if ((await invoke('models_refresh')) !== 0) throw new Error('unchanged list reported as changed');
    await setModels(['mock-coder', 'mock-reviewer']);
  });

  await step('subscription sign-in is refused until the user opts in', async () => {
    const r = await invoke('subscription_sign_in', { service: 'claude' }).then(
      () => 'allowed',
      (e) => String(e),
    );
    if (!r.includes('off')) throw new Error(r);
    const prefs = await invoke('prefs_get');
    await invoke('prefs_save', { prefs: { ...prefs, subscription_signin: true } });
  });

  for (const [service, check] of [
    ['claude', (u) => u.windows[0].label === '5-hour session' && u.windows[0].used_percent === 42],
    ['chatgpt', (u) => u.plan === 'plus' && u.windows[0].label === '5-hour window' && u.windows[0].used_percent === 7],
    ['gemini', (u) => u.plan === 'free-tier' && Math.round(u.windows[0].used_percent) === 40],
  ]) {
    await step(`${service}: PKCE sign-in, keychain, token refresh and usage`, async () => {
      const who = await invoke('subscription_sign_in', { service });
      if (who.account !== `${service}@example.com`) throw new Error(JSON.stringify(who));
      if (!seen.verifier[service]) throw new Error('PKCE verifier was not checked');
      await new Promise((r) => setTimeout(r, 1200)); // let the 1 s access token expire
      const u = await invoke('subscription_usage', { service });
      if (!check(u)) throw new Error(JSON.stringify(u));
      if (!seen.refreshed[service]) throw new Error('expired access token was not refreshed');
      const o = await invoke('usage_overview');
      if (!o.subscriptions.find((s) => s.service === service)?.signed_in) throw new Error('not shown as signed in');
    });
  }

  await step('Usage view renders each provider in its own theme', async () => {
    await page.reload();
    const wizard = page.getByRole('dialog', { name: 'Set up Conductor' });
    if (
      await wizard.waitFor({ timeout: 5_000 }).then(
        () => true,
        () => false,
      )
    )
      await page.getByRole('button', { name: 'Skip setup' }).click();
    await page.keyboard.press('Control+,');
    const s = page.getByRole('dialog', { name: 'Settings' });
    await s.getByRole('navigation').getByRole('button', { name: 'Usage', exact: true }).click();
    const panel = s.getByTestId('usage-panel');
    const colors = new Set();
    for (const label of ['Claude', 'ChatGPT', 'Gemini']) {
      await s.getByRole('tab', { name: label }).click();
      await panel.getByRole('meter').first().waitFor({ timeout: 15_000 });
      colors.add(
        await panel.evaluate(
          (el) =>
            getComputedStyle(el).backgroundColor +
            getComputedStyle(el.querySelector('.glyph')).backgroundImage +
            getComputedStyle(el.querySelector('.glyph')).backgroundColor,
        ),
      );
      await page.screenshot({ path: `${shots}/native-usage-${label.toLowerCase()}.png` });
    }
    if (colors.size !== 3) throw new Error(`themes not distinct: ${[...colors].join(' | ')}`);
  });

  await step('sign out removes the keychain entry', async () => {
    for (const service of ['claude', 'chatgpt', 'gemini']) await invoke('subscription_sign_out', { service });
    const o = await invoke('usage_overview');
    if (o.subscriptions.some((s) => s.signed_in)) throw new Error('still signed in');
    const r = await invoke('subscription_usage', { service: 'claude' }).then(
      () => 'usage returned',
      (e) => String(e),
    );
    if (!r.includes('Not signed in')) throw new Error(r);
  });
} catch {
  code = 1;
} finally {
  // Never leave test sign-ins in the keychain.
  for (const service of ['claude', 'chatgpt', 'gemini'])
    await invoke('subscription_sign_out', { service }).catch(() => {});
  await invoke('remove_provider', { id: 'usage-mock' }).catch(() => {});
  await app.browser.close().catch(() => {});
  try {
    execFileSync('taskkill', ['/F', '/T', '/PID', String(app.proc.pid)], { stdio: 'ignore' });
  } catch {}
  mock.kill();
  provider.close();
  console.log(code === 0 ? '\nUsage: all steps passed' : '\nUsage: FAILED');
  process.exit(code);
}
