// Verifies that the real app's updater accepts a correctly signed package and
// rejects a tampered one — using the public key built into the app. Nothing
// is installed: the app's test hook downloads and verifies only.
//
// Requires the release signing key (see docs/RELEASES.md) at
// ~/.conductor-release/updater.key, and OpenSSL (bundled with Git for Windows).
//
// Usage: node scripts/desktop-update.mjs [path-to-conductor-app.exe]
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import https from 'node:https';
import os from 'node:os';
import path from 'node:path';
import { launch, stepper } from './lib/desktop.mjs';

const exe = path.resolve(process.argv[2] ?? 'target/claude/debug/conductor-app.exe');
const key = path.join(os.homedir(), '.conductor-release', 'updater.key');
if (!fs.existsSync(key)) {
  console.log('skipped: no release signing key at', key);
  process.exit(0);
}
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'conductor-update-'));
const results = [];
const step = stepper(results);

// Self-signed TLS for the local feed.
execFileSync('openssl', ['req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '1', '-subj', '/CN=localhost', '-keyout', path.join(tmp, 'key.pem'), '-out', path.join(tmp, 'cert.pem')], { stdio: 'ignore' });

// A package and its minisign signature made with the release key.
const pkg = path.join(tmp, 'Conductor_99.0.0_x64-setup.nsis.zip');
fs.writeFileSync(pkg, Buffer.from('conductor update payload ' + Date.now()));
const signOut = execFileSync(process.platform === 'win32' ? 'npx.cmd' : 'npx', ['tauri', 'signer', 'sign', '-f', key, '-p', process.platform === 'win32' ? '""' : '', pkg], { encoding: 'utf8', shell: process.platform === 'win32' });
// The signer prints the base64 minisign signature (and may also write <file>.sig).
const signature = fs.existsSync(pkg + '.sig') ? fs.readFileSync(pkg + '.sig', 'utf8').trim() : signOut.split(/\s+/).find((t) => t.length > 200);
if (!signature) throw new Error('could not sign test package');
const tampered = Buffer.concat([fs.readFileSync(pkg), Buffer.from('!')]);

const server = https.createServer({ key: fs.readFileSync(path.join(tmp, 'key.pem')), cert: fs.readFileSync(path.join(tmp, 'cert.pem')) }, (req, res) => {
  const port = server.address().port;
  const feed = (kind) => ({
    version: '99.0.0',
    notes: 'Test update',
    pub_date: new Date().toISOString(),
    platforms: { 'windows-x86_64': { url: `https://127.0.0.1:${port}/${kind}/pkg.zip`, signature } },
  });
  if (req.url === '/good/latest.json') return res.end(JSON.stringify(feed('good')));
  if (req.url === '/bad/latest.json') return res.end(JSON.stringify(feed('bad')));
  if (req.url === '/good/pkg.zip') return res.end(fs.readFileSync(pkg));
  if (req.url === '/bad/pkg.zip') return res.end(tampered);
  res.writeHead(404).end();
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const port = server.address().port;

let code = 0;
async function verifyWith(kind) {
  process.env.CONDUCTOR_TEST_UPDATES = '1';
  process.env.CONDUCTOR_TEST_UPDATE_ENDPOINT = `https://127.0.0.1:${port}/${kind}/latest.json`;
  const app = await launch(exe, path.join(tmp, `data-${kind}`));
  try {
    await app.page.getByRole('dialog', { name: 'Set up Conductor' }).waitFor({ timeout: 30_000 });
    return await app.invoke('update_verify_test');
  } finally {
    await app.browser.close().catch(() => {});
    try {
      execFileSync('taskkill', ['/F', '/T', '/PID', String(app.proc.pid)], { stdio: 'ignore' });
    } catch {}
  }
}

try {
  await step('correctly signed update passes signature verification', async () => {
    const r = await verifyWith('good');
    if (!r.available || r.version !== '99.0.0' || !r.verified) throw new Error(JSON.stringify(r));
  });
  await step('tampered update is rejected before it could be installed', async () => {
    const r = await verifyWith('bad');
    if (!r.available || r.verified !== false) throw new Error(JSON.stringify(r));
    console.log(`    rejected: ${r.error}`);
  });
} catch {
  code = 1;
} finally {
  server.close();
  console.log(code === 0 ? '\nUpdate verification: all steps passed' : '\nUpdate verification: FAILED');
  process.exit(code);
}
