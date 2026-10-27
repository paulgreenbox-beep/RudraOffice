// Public source admission only. This does not replace Verification Core or run GUI.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';

const toolkitUrl = 'https://github.com/paulgreenbox-beep/rust-makepad-toolkit.git';
const makepadUrl = 'https://github.com/makepad/makepad.git';
const modules = ['package.json', 'core/global-policy.json', 'core/global-policy.mjs',
  'core/verification-policy.mjs', 'core/verification-run-envelope.mjs',
  'core/executable-provenance.mjs', 'core/index.mjs'];
const sha = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex').toUpperCase();
const read = file => JSON.parse(fs.readFileSync(file, 'utf8'));
const real = file => fs.realpathSync(file);
function samePhysicalDirectory(first, second) {
  const a = fs.statSync(first, {bigint: true}), b = fs.statSync(second, {bigint: true});
  if (!a.isDirectory() || !b.isDirectory()) return false;
  // Windows Git/Node may spell drive letters and temp directories differently.
  // Compare actual directory identity, not path casing or a basename heuristic.
  if (a.ino !== 0n && b.ino !== 0n) return a.dev === b.dev && a.ino === b.ino;
  return real(first) === real(second); // Unknown file identity stays fail-closed.
}
const revision = value => typeof value === 'string' && /^[a-f0-9]{40}$/i.test(value);
function physicalProjection(file) {
  let cursor = path.resolve(file);
  const missing = [];
  while (!fs.existsSync(cursor)) {
    try { fs.lstatSync(cursor); throw Error('DANGLING_DEPENDENCY_LINK'); }
    catch (error) { if (error.code !== 'ENOENT') throw error; }
    const parent = path.dirname(cursor);
    if (parent === cursor) throw Error('NO_PHYSICAL_PARENT');
    missing.unshift(path.basename(cursor)); cursor = parent;
  }
  return path.resolve(real(cursor), ...missing);
}
function physicalWithin(root, file) {
  const relative = path.relative(physicalProjection(root), physicalProjection(file));
  if (path.isAbsolute(relative) || relative === '..' || relative.startsWith('..' + path.sep)) throw Error('PHYSICAL_PATH_ESCAPES_ROOT');
}
function child(root, relative) {
  if (typeof relative !== 'string' || !relative || path.isAbsolute(relative)
      || path.win32.isAbsolute(relative) || /^[a-z]:/i.test(relative)) throw Error('INVALID_RELATIVE_PATH');
  const result = path.resolve(root, relative);
  const rel = path.relative(root, result);
  if (path.isAbsolute(rel) || rel === '..' || rel.startsWith('..' + path.sep)) throw Error('PATH_ESCAPES_ROOT');
  physicalWithin(root, result);
  return result;
}
export function gitRead(cwd, args) {
  const result = spawnSync('git', ['--no-optional-locks', '-c', 'core.longpaths=true', '-C', cwd, ...args],
    {encoding: 'utf8', timeout: 15000, maxBuffer: 1024 * 1024, windowsHide: true});
  return {ok: result.status === 0, text: (result.stdout ?? '').trim()};
}
function normalizeRemote(value) {
  // Do not print credentials, access tokens or arbitrary remote strings.
  if (value.startsWith('git@github.com:')) value = 'https://github.com/' + value.slice(15);
  try {
    const url = new URL(value);
    if (url.protocol !== 'https:' || url.hostname !== 'github.com' || url.username || url.password || url.search || url.hash) return null;
    return ('https://github.com' + url.pathname.replace(/\.git$/, '')).toLowerCase();
  } catch { return null; }
}
export function inspectRepository(root, expected, run = gitRead) {
  const issues = [];
  if (!fs.existsSync(root)) return {exists: false, issues: ['MISSING_CHECKOUT']};
  const top = run(root, ['rev-parse', '--show-toplevel']);
  if (!top.ok || !fs.existsSync(top.text) || !samePhysicalDirectory(top.text, root)) {
    return {exists: true, independent_git_root: false, issues: ['NOT_INDEPENDENT_GIT_ROOT']};
  }
  const head = run(root, ['rev-parse', 'HEAD']);
  const dirty = run(root, ['status', '--porcelain=v1', '--untracked-files=normal']);
  const flags = run(root, ['ls-files', '-v', '-z']);
  const remote = run(root, ['remote', 'get-url', 'origin']);
  if (!head.ok || !revision(head.text)) issues.push('INVALID_HEAD');
  if (!revision(expected.reviewed_revision)) issues.push('REVISION_NOT_REVIEWED');
  else if (head.text.toLowerCase() !== expected.reviewed_revision.toLowerCase()) issues.push('REVISION_MISMATCH');
  if (!dirty.ok || dirty.text) issues.push('DIRTY_OR_UNREADABLE_TREE');
  // status may hide edits under assume-unchanged/skip-worktree index flags.
  const hiddenFlags = !flags.ok || flags.text.split('\0').filter(Boolean).some(row => /^[a-zS]/.test(row));
  if (hiddenFlags) issues.push('FORBIDDEN_OR_UNREADABLE_INDEX_FLAGS');
  const remoteMatches = remote.ok && normalizeRemote(remote.text) === normalizeRemote(expected.repository);
  if (!remoteMatches) issues.push('ORIGIN_MISMATCH');
  return {exists: true, independent_git_root: true, actual_revision: head.ok ? head.text : null,
    clean: dirty.ok && !dirty.text && !hiddenFlags, remote_matches: remoteMatches, issues};
}
function configuration(root) {
  const config = read(path.join(root, 'verification/public-dependencies.json'));
  const binding = read(path.join(root, 'verification/toolkit-core.json'));
  if (config.schema_version !== 1 || binding.schema_version !== 1 || binding.core_version !== '1.0.0'
      || config.toolkit?.repository !== toolkitUrl || config.makepad?.repository !== makepadUrl
      || config.makepad.relative_root !== '.deps/makepad') throw Error('UNSUPPORTED_DEPENDENCY_CONTRACT');
  // Cargo manifests currently require this sibling directory, not an arbitrary env override.
  if (binding.toolkit_relative_root !== '../Rust_Makepad_Toolkit') throw Error('UNSUPPORTED_CARGO_TOPOLOGY');
  if (!binding.module_hashes || Object.keys(binding.module_hashes).sort().join('|') !== [...modules].sort().join('|')
      || modules.some(m => !/^[0-9a-f]{64}$/i.test(binding.module_hashes[m] ?? ''))) throw Error('INVALID_CORE_INTEGRITY_PINS');
  const toolkitRoot = path.resolve(root, binding.toolkit_relative_root);
  physicalWithin(path.dirname(path.resolve(root)), toolkitRoot);
  return {config, binding, toolkitRoot};
}
export function verifyPublicDependencies(root, {run = gitRead} = {}) {
  const {config, binding, toolkitRoot} = configuration(root);
  const makepadRoot = child(toolkitRoot, config.makepad.relative_root);
  const toolkit = inspectRepository(toolkitRoot, config.toolkit, run);
  const makepad = inspectRepository(makepadRoot, config.makepad, run);
  const issues = [];
  if (config.status !== 'reviewed') issues.push({component: 'contract', code: 'OWNER_REVIEW_PENDING'});
  for (const [name, result, dir, expected] of [['toolkit', toolkit, toolkitRoot, config.toolkit], ['makepad', makepad, makepadRoot, config.makepad]]) {
    for (const code of result.issues) issues.push({component: name, code});
    for (const relative of expected.required_files ?? []) {
      const file = child(dir, relative);
      if (!fs.existsSync(file) || !fs.statSync(file).isFile()) issues.push({component: name, code: 'MISSING_REQUIRED_FILE', file: relative});
    }
  }
  const core = child(toolkitRoot, 'verification-economy');
  for (const relative of modules) {
    const file = child(core, relative);
    if (!fs.existsSync(file)) issues.push({component: 'core', code: 'MISSING_PINNED_MODULE', file: relative});
    else if (sha(file) !== binding.module_hashes[relative].toUpperCase()) issues.push({component: 'core', code: 'MODULE_HASH_MISMATCH', file: relative});
  }
  const pkgFile = path.join(core, 'package.json');
  if (fs.existsSync(pkgFile)) {
    try {
      const pkg = read(pkgFile);
      if (pkg.name !== '@rust-makepad-toolkit/verification-economy' || pkg.version !== binding.core_version) issues.push({component: 'core', code: 'PACKAGE_IDENTITY_MISMATCH'});
    } catch { issues.push({component: 'core', code: 'INVALID_PACKAGE_JSON'}); }
  }
  return {schema_version: 1, status: issues.length ? 'BLOCKED_DEPENDENCIES' : 'SOURCE_DEPENDENCIES_VERIFIED_ONLY',
    toolkit, makepad, core_version: binding.core_version, issues, runs_network: false,
    windows_build_verified: false, gui_verified: false};
}
export function bootstrapPublicDependencies(root, {run = gitRead, clone} = {}) {
  const {config, toolkitRoot} = configuration(root);
  // Refuse before mkdir/network when either dependency is only a proposed pin.
  if (config.status !== 'reviewed' || !revision(config.toolkit.reviewed_revision) || !revision(config.makepad.reviewed_revision)) {
    throw Error('OWNER_REVIEW_PENDING: no checkout or network operation was performed');
  }
  const clonePinned = clone ?? ((destination, expected) => {
    fs.mkdirSync(path.dirname(destination), {recursive: true});
    for (const args of [
      ['clone', '--depth', '1', '--no-checkout', expected.repository, destination],
      ['-C', destination, 'fetch', '--depth', '1', 'origin', expected.reviewed_revision],
      ['-C', destination, 'checkout', '--detach', expected.reviewed_revision]
    ]) {
      const result = spawnSync('git', ['-c', 'core.longpaths=true', ...args], {stdio: 'inherit', windowsHide: true});
      if (result.status !== 0) throw Error('BOOTSTRAP_FAILED: inspect the created checkout; it was not deleted or overwritten');
    }
  });
  for (const [destination, expected] of [[toolkitRoot, config.toolkit], [child(toolkitRoot, config.makepad.relative_root), config.makepad]]) {
    if (!fs.existsSync(destination)) clonePinned(destination, expected);
    const state = inspectRepository(destination, expected, run);
    if (state.issues.length) throw Error('EXISTING_OR_CREATED_CHECKOUT_REJECTED:' + state.issues.join(','));
    if (destination === toolkitRoot) {
      const early = verifyPublicDependencies(root, {run}).issues.filter(issue => issue.component !== 'makepad');
      if (early.length) throw Error('TOOLKIT_OR_CORE_REJECTED:' + early.map(issue => issue.code).join(','));
    }
  }
  return verifyPublicDependencies(root, {run});
}
const self = fileURLToPath(import.meta.url);
if (process.argv[1] && path.resolve(process.argv[1]) === self) {
  const args = process.argv.slice(2);
  try {
    let root = path.resolve(path.dirname(self), '..'), bootstrap = false;
    for (let index = 0; index < args.length; index++) {
      if (args[index] === '--bootstrap') bootstrap = true;
      else if (args[index] === '--root' && args[index + 1] && !args[index + 1].startsWith('--')) root = path.resolve(args[++index]);
      else throw Error('INVALID_ARGUMENTS');
    }
    const result = bootstrap ? bootstrapPublicDependencies(root) : verifyPublicDependencies(root);
    console.log(JSON.stringify(result, null, 2));
    process.exitCode = result.issues.length ? 2 : 0;
  } catch (error) {
    console.error(JSON.stringify({status: 'BLOCKED_DEPENDENCIES', error: error.message}));
    process.exitCode = 2;
  }
}
