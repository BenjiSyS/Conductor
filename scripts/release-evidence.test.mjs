import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { promisify } from 'node:util';
import { execFile } from 'node:child_process';
const run = promisify(execFile);

test('package evidence hashes actual bytes and labels unsigned artifacts honestly', async () => {
  const folder = await mkdtemp(join(tmpdir(), 'conductor-package-test-'));
  try {
    await mkdir(join(folder, 'nsis'));
    await writeFile(join(folder, 'nsis', 'Conductor fixture.exe'), 'abc');
    await writeFile(join(folder, 'unrelated.log'), 'not a package');
    await run(process.execPath, ['scripts/release-evidence.mjs', folder]);
    const manifest = JSON.parse(await readFile(join(folder, 'release-manifest.json'), 'utf8'));
    assert.equal(manifest.packages.length, 1);
    assert.equal(manifest.packages[0].path, 'nsis/Conductor fixture.exe');
    assert.equal(manifest.packages[0].bytes, 3);
    assert.equal(manifest.packages[0].sha256, 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad');
    assert.equal(manifest.installation_verified, false);
    assert.equal(manifest.os_signature_verified, false);
    assert.equal(manifest.updater_signature_verified, false);
    assert.equal(manifest.status, 'development');
    // Regeneration must not hash the old manifest or checksums themselves.
    await run(process.execPath, ['scripts/release-evidence.mjs', folder]);
    assert.equal(JSON.parse(await readFile(join(folder, 'release-manifest.json'), 'utf8')).packages.length, 1);
  } finally {
    await rm(folder, { recursive: true, force: true });
  }
});

test('empty build output cannot produce a success manifest', async () => {
  const folder = await mkdtemp(join(tmpdir(), 'conductor-package-test-'));
  try {
    await assert.rejects(run(process.execPath, ['scripts/release-evidence.mjs', folder]), /No built package found/);
  } finally {
    await rm(folder, { recursive: true, force: true });
  }
});
