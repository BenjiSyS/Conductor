import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { readdir, readFile, writeFile, lstat } from 'node:fs/promises';
import { resolve, join, relative } from 'node:path';

// Hash actual packages only. Signatures, installation and runtime behaviour
// need their own verification; producing a hash never claims those passed.
const folder = resolve(process.argv[2] ?? 'target/release/bundle');
const files = [];
async function walk(path) {
  for (const entry of await readdir(path, { withFileTypes: true })) {
    const file = join(path, entry.name);
    if (entry.isSymbolicLink()) continue;
    if (entry.isDirectory()) await walk(file);
    else if (entry.isFile() && /\.(?:exe|msi|deb|rpm|AppImage|dmg|tar\.gz|sig)$/.test(entry.name)) {
      files.push(file);
    }
  }
}
await walk(folder);
files.sort();
if (!files.some((file) => !file.endsWith('.sig'))) throw new Error('No built package found. Refusing an empty release manifest.');
const packages = [];
for (const file of files) {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(file)) hash.update(chunk);
  packages.push({ path: relative(folder, file).replaceAll('\\', '/'), bytes: (await lstat(file)).size, sha256: hash.digest('hex') });
}
const version = JSON.parse(await readFile('package.json', 'utf8')).version;
await writeFile(join(folder, 'SHA256SUMS.txt'), packages.map((p) => `${p.sha256}  ${p.path}`).join('\n') + '\n');
await writeFile(join(folder, 'release-manifest.json'), JSON.stringify({
  schema: 1, product: 'Conductor', version,
  target: process.env.CONDUCTOR_PACKAGE_TARGET ?? null,
  revision: process.env.GITHUB_SHA ?? null,
  build_run: process.env.GITHUB_RUN_ID ?? null,
  created_at: new Date().toISOString(),
  status: 'development',
  os_signature_verified: false,
  updater_signature_verified: false,
  installation_verified: false,
  packages,
}, null, 2) + '\n');
console.log(`Recorded SHA-256 for ${packages.length} artifact(s). Installation and signatures remain unverified.`);
