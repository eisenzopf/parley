// Read-only verification of the exact dependency sources expected by this kit.
import { readFile, readdir, lstat } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const sha = data => createHash('sha256').update(data).digest('hex');
const git = (cwd, ...args) => execFileSync('git', args, { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
async function requireHash(path, expected, label) {
  if (!(await lstat(path)).isFile() || sha(await readFile(path)) !== expected) throw new Error(`${label}: source differs from the conference dependency manifest`);
}
export async function verifyDependencies(project = root, rvoip = resolve(project, '../rvoip-conference')) {
  const manifest = JSON.parse(await readFile(resolve(project, 'config/conference-dependencies.json'), 'utf8'));
  if (manifest.version !== 1) throw new Error('Unsupported dependency manifest');
  if (git(rvoip, 'rev-parse', 'HEAD') !== manifest.rvoip.revision) throw new Error('Rvoip checkout is not at the pinned baseline');
  if ((await readFile(resolve(project, 'patches/rvoip/BASE_REV'), 'utf8')).trim() !== manifest.rvoip.revision) throw new Error('Rvoip baseline and dependency manifest disagree');
  await requireHash(resolve(project, 'patches/rvoip/conference.patch'), manifest.rvoip.patch_sha256, 'Rvoip patch');
  const expected = new Set(Object.keys(manifest.rvoip.files));
  const changed = git(rvoip, 'diff', '--no-ext-diff', '--name-only', '--no-renames', 'HEAD').split('\n').filter(Boolean);
  const untracked = git(rvoip, 'ls-files', '--others', '--exclude-standard').split('\n').filter(Boolean);
  if ([...changed, ...untracked].some(path => !expected.has(path))) throw new Error('Rvoip contains changes outside the pinned conference patch');
  for (const [name, hash] of Object.entries(manifest.rvoip.files)) await requireHash(resolve(rvoip, name), hash, `Rvoip ${name}`);
  const vendor = resolve(project, 'vendor/server-sdk-rust');
  const buildScript = await lstat(resolve(vendor, 'build.rs')).catch(error => { if (error.code === 'ENOENT') return null; throw error; });
  if (buildScript) throw new Error('Vapi snapshot contains an unrecorded build script');
  const rustFiles = (await readdir(resolve(vendor, 'src'))).filter(name => name.endsWith('.rs')).map(name => `src/${name}`);
  if (rustFiles.some(name => !(name in manifest.vapi.files))) throw new Error('Vapi snapshot contains unrecorded Rust source');
  for (const [name, hash] of Object.entries(manifest.vapi.files)) await requireHash(resolve(vendor, name), hash, `Vapi ${name}`);
  return { rvoip: `${manifest.rvoip.version} + conference patches`, revision: manifest.rvoip.revision,
    patch_sha256: manifest.rvoip.patch_sha256, vapi: `${manifest.vapi.version} vendored snapshot`, verified: true };
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  verifyDependencies().then(result => console.log(JSON.stringify(result, null, 2)))
    .catch(error => { console.error(error.message); process.exitCode = 1; });
}
