// Owned local-source staging. Not a GUI runner or an acceptance authority.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {fileURLToPath} from 'node:url';

export const hashBytes = bytes => crypto.createHash('sha256').update(bytes).digest('hex').toUpperCase();
export const hashFile = file => hashBytes(fs.readFileSync(file));
const slash = value => value.replaceAll('\\', '/');
const fail = (ok, message) => { if (!ok) throw Error(message); };
export function inside(root, file) {
  const relative = path.relative(path.resolve(root), path.resolve(file));
  return relative !== '..' && !relative.startsWith('..' + path.sep) && !path.isAbsolute(relative);
}
export function noLinks(root, file, declaredLinks = []) {
  fail(inside(root, file), 'outside allowed source root: ' + file);
  let cursor = path.resolve(root);
  fail(!fs.lstatSync(cursor).isSymbolicLink(), 'linked source root: ' + cursor);
  for (const part of path.relative(cursor, file).split(path.sep).filter(Boolean)) {
    cursor = path.join(cursor, part);
    if (fs.lstatSync(cursor).isSymbolicLink()) {
      const declaration = declaredLinks.find(item => path.resolve(item.path).toLowerCase() === cursor.toLowerCase());
      fail(declaration && fs.realpathSync(cursor).toLowerCase() === path.resolve(declaration.target).toLowerCase(), 'linked source component: ' + cursor);
      // An original checkout bootstrap link is permitted only when reviewed and
      // pinned to one physical, non-linked target. It is always materialized.
      noLinks(root, declaration.target);
    }
  }
  fail(inside(fs.realpathSync(root), fs.realpathSync(file)), 'physical source escape: ' + file);
}
export function mapped(sourceRoot, outputRoot, file) {
  fail(inside(sourceRoot, file), 'unmapped source: ' + file);
  return path.join(outputRoot, 's', path.relative(sourceRoot, file));
}
export function transformManifest(text, original, output, sourceRoot, outputRoot, selectedFiles) {
  const changes = [];
  // Only literal Cargo path values are mechanically rebased. Relative values
  // preserve their topology; absolute paths must point to the copied source set.
  let result = text.replace(/\bpath\s*=\s*(["'])([^"'\r\n]+)\1/g, (whole, quote, value) => {
    if (!path.isAbsolute(value)) return whole;
    const absolute = path.normalize(value);
    fail(inside(sourceRoot, absolute), 'absolute Cargo path outside closure: ' + value);
    fail(selectedFiles.has(path.join(absolute, 'Cargo.toml')) || selectedFiles.has(absolute), 'absolute Cargo path was not copied: ' + value);
    const replacement = slash(path.relative(path.dirname(output), mapped(sourceRoot, outputRoot, absolute)));
    changes.push({kind: 'absolute-path-rebase', before: value, after: replacement});
    return whole.replace(value, replacement);
  });
  // Ancestor workspaces sometimes list applications that are not dependencies.
  // Preserve all fields except members/default-members not present in the copy.
  let section = '';
  result = result.replace(/^\[([^\]\r\n]+)\].*$|^\s*((?:workspace\.)?(?:default-)?members)\s*=\s*\[([^\]]*)\]/gm, (whole, heading, key, body) => {
    if (heading) { section = heading; return whole; }
    if (!key.startsWith('workspace.') && section !== 'workspace') return whole;
    const values = [...body.replace(/#[^\r\n]*/g, '').matchAll(/["']([^"']+)["']/g)].map(m => m[1]);
    const retained = values.filter(value => {
      if (value.includes('*') || value.includes('?')) throw Error('workspace glob requires explicit review: ' + original + ': ' + value);
      return selectedFiles.has(path.join(path.dirname(original), value, 'Cargo.toml'));
    });
    if (retained.length === values.length) return whole;
    changes.push({kind: 'prune-absent-workspace-members', key, before: values, after: retained});
    return key + ' = [' + retained.map(v => JSON.stringify(v)).join(', ') + ']';
  });
  return {text: result, changes};
}
export function graphSignature(metadata, restore = value => value) {
  const identities = new Map(metadata.packages.map(pkg => [pkg.id,
    pkg.source === null ? `local:${slash(restore(pkg.manifest_path))}:${pkg.name}@${pkg.version}` : `${pkg.source}:${pkg.name}@${pkg.version}`]));
  const nodes = metadata.resolve.nodes.map(node => ({
    identity: identities.get(node.id), features: [...node.features].sort(),
    deps: node.deps.map(dep => ({name: dep.name, identity: identities.get(dep.pkg), kinds: dep.dep_kinds.map(k => ({kind: k.kind, target: k.target})).sort((a,b) => JSON.stringify(a).localeCompare(JSON.stringify(b)))})).sort((a,b) => JSON.stringify(a).localeCompare(JSON.stringify(b))),
  })).sort((a,b) => a.identity.localeCompare(b.identity));
  const packages = metadata.packages.map(pkg => ({
    identity: identities.get(pkg.id), edition: pkg.edition, rust_version: pkg.rust_version,
    links: pkg.links, license: pkg.license, features: pkg.features,
    targets: (pkg.targets ?? []).map(target => ({...target, src_path: slash(restore(target.src_path))})).sort((a,b) => JSON.stringify(a).localeCompare(JSON.stringify(b))),
    declarations: (pkg.dependencies ?? []).map(dep => ({...dep, ...(dep.path ? {path:slash(restore(dep.path))} : {})})).sort((a,b) => JSON.stringify(a).localeCompare(JSON.stringify(b))),
  })).sort((a,b)=>a.identity.localeCompare(b.identity));
  return {root: identities.get(metadata.resolve.root), nodes, packages,
    workspace_members: (metadata.workspace_members ?? []).map(id=>identities.get(id)).sort(),
    workspace_default_members: (metadata.workspace_default_members ?? []).map(id=>identities.get(id)).sort(),
    workspace_root: metadata.workspace_root ? slash(restore(metadata.workspace_root)) : null};
}
export function createCapsule(config) {
  const {sourceRoot, officeRoot, metadataFile, baselineFreeze, outputRoot, extraFiles = [], declaredLinks = []} = config;
  fail(inside(officeRoot, outputRoot) && path.resolve(outputRoot) !== path.resolve(officeRoot), 'capsule must be an explicit child of Office');
  fail(!fs.existsSync(outputRoot), 'capsule destination already exists; never overwrite');
  let ancestor = path.dirname(path.resolve(outputRoot));
  while (!fs.existsSync(ancestor)) ancestor = path.dirname(ancestor);
  noLinks(officeRoot, ancestor);
  const metadata = JSON.parse(fs.readFileSync(metadataFile, 'utf8'));
  const baseline = JSON.parse(fs.readFileSync(baselineFreeze, 'utf8'));
  const selected = new Set();
  const skipped = new Set(['target', '.git', 'node_modules', '.mimo-bridge', '.mimocode', '__pycache__']);
  function walk(directory) {
    noLinks(sourceRoot, directory, declaredLinks);
    for (const entry of fs.readdirSync(directory, {withFileTypes: true})) {
      if (skipped.has(entry.name)) continue;
      const file = path.join(directory, entry.name);
      noLinks(sourceRoot, file, declaredLinks);
      if (entry.isDirectory()) walk(file);
      else if (entry.isFile()) selected.add(path.normalize(file));
      else throw Error('nonregular source: ' + file);
    }
  }
  const packageRoots = [...new Set(metadata.packages.filter(p => p.source === null).map(p => path.dirname(p.manifest_path)))];
  for (const root of packageRoots) {
    walk(root);
    for (let parent = path.dirname(root); inside(sourceRoot, parent); parent = path.dirname(parent)) {
      for (const name of ['Cargo.toml', 'Cargo.lock', '.cargo/config.toml']) {
        const file = path.join(parent, name);
        if (fs.existsSync(file)) { noLinks(sourceRoot, file, declaredLinks); selected.add(file); }
      }
      if (path.resolve(parent) === path.resolve(sourceRoot)) break;
    }
  }
  for (const entry of baseline) selected.add(path.normalize(entry.path));
  for (const file of extraFiles) selected.add(path.normalize(file));
  fail(selected.size > 0 && selected.size <= 60000, 'unbounded inventory');
  const captures = [...selected].sort().map(original => {
    noLinks(sourceRoot, original, declaredLinks);
    const bytes = fs.readFileSync(original);
    fail(!/[\\/](?:credentials|\.env|id_rsa)(?:[\\/]|$)/i.test(original), 'secret-like input requires review: ' + original);
    return {original, physical: fs.realpathSync(original), bytes, sha256: hashBytes(bytes)};
  });
  // No writes occur before every input and transformation has been checked.
  const transformations = [];
  const planned = captures.map(input => {
    const destination = mapped(sourceRoot, outputRoot, input.original);
    let bytes = input.bytes;
    if (path.basename(input.original) === 'Cargo.toml') {
      const changed = transformManifest(bytes.toString('utf8'), input.original, destination, sourceRoot, outputRoot, selected);
      if (changed.changes.length) {
        bytes = Buffer.from(changed.text);
        transformations.push({original: slash(input.original), destination: slash(path.relative(outputRoot, destination)), original_sha256: input.sha256, transformed_sha256: hashBytes(bytes), changes: changed.changes});
      }
    }
    return {...input, destination, outputBytes: bytes};
  });
  fs.mkdirSync(outputRoot, {recursive: true});
  const inventory = [];
  for (const input of planned) {
    noLinks(sourceRoot, input.original, declaredLinks);
    fail(fs.realpathSync(input.original) === input.physical, 'source target changed while staging: ' + input.original);
    fail(hashFile(input.original) === input.sha256, 'source changed while staging: ' + input.original);
    fs.mkdirSync(path.dirname(input.destination), {recursive: true});
    fs.writeFileSync(input.destination, input.outputBytes, {flag: 'wx'});
    inventory.push({path: slash(path.relative(officeRoot, input.destination)), capsule_path: slash(path.relative(outputRoot, input.destination)), original_path: slash(input.original), original_physical_path: slash(input.physical), original_sha256: input.sha256, sha256: hashBytes(input.outputBytes), bytes: input.outputBytes.length, ownership: inside(officeRoot, input.original) ? 'office-owned-copy' : 'external-local-owned-copy', transformed: input.sha256 !== hashBytes(input.outputBytes)});
  }
  const previous = new Map(baseline.map(f => [path.normalize(f.path), f.sha256]));
  const drift = inventory.filter(f => previous.has(path.normalize(f.original_path)) && previous.get(path.normalize(f.original_path)) !== f.original_sha256);
  const write = (name, value) => fs.writeFileSync(path.join(outputRoot, name), JSON.stringify(value, null, 2) + '\n', {flag: 'wx'});
  const packages = metadata.packages.map(pkg => ({name: pkg.name, version: pkg.version, source: pkg.source, original_manifest: slash(pkg.manifest_path), copied: pkg.source === null, capsule_manifest: pkg.source === null ? slash(path.relative(outputRoot, mapped(sourceRoot, outputRoot, pkg.manifest_path))) : null, role: pkg.targets.map(t => ({name:t.name, kind:t.kind})), ownership: pkg.source === null ? (inside(officeRoot, pkg.manifest_path) ? 'office-local' : 'external-local') : 'registry-not-copied'}));
  write('source-inventory.json', inventory);
  write('source-freeze.json', inventory.map(({path, sha256}) => ({path, sha256})));
  write('dependency-map.json', {packages, transformations, materialized_source_links: declaredLinks, baseline_drift: drift, graph: graphSignature(metadata)});
  write('capsule-lock.json', {schema_version: 1, state: 'FROZEN_NOT_BUILT', recorded_at: new Date().toISOString(), source_inventory_sha256: hashFile(path.join(outputRoot, 'source-inventory.json')), source_freeze_sha256: hashFile(path.join(outputRoot, 'source-freeze.json')), dependency_map_sha256: hashFile(path.join(outputRoot, 'dependency-map.json')), baseline_freeze_sha256: hashFile(baselineFreeze), original_metadata_sha256: hashFile(metadataFile), files: inventory.length, local_packages: packageRoots.length, external_files: inventory.filter(f => f.ownership === 'external-local-owned-copy').length, boundary: 'Owned local path sources only. Registry cache, toolchain, Windows SDK and build outputs are separate, not hermetic.'});
  return {outputRoot, files: inventory.length, local_packages: packageRoots.length, external_files: inventory.filter(f => f.ownership === 'external-local-owned-copy').length, baseline_drift: drift.map(f => f.original_path), transformed_manifests: transformations.length};
}
export function verifyCapsule(outputRoot, officeRoot) {
  const lock = JSON.parse(fs.readFileSync(path.join(outputRoot, 'capsule-lock.json'), 'utf8'));
  for (const [file, field] of [['source-inventory.json','source_inventory_sha256'],['source-freeze.json','source_freeze_sha256'],['dependency-map.json','dependency_map_sha256']]) fail(hashFile(path.join(outputRoot,file)) === lock[field], 'manifest drift: ' + file);
  const inventory = JSON.parse(fs.readFileSync(path.join(outputRoot, 'source-inventory.json'), 'utf8'));
  noLinks(officeRoot, outputRoot);
  const actualFiles = new Set();
  function scan(directory) {
    for (const entry of fs.readdirSync(directory, {withFileTypes: true})) {
      const file = path.join(directory, entry.name);
      fail(!entry.isSymbolicLink(), 'linked capsule source: ' + file);
      if (entry.isDirectory()) scan(file);
      else if (entry.isFile()) actualFiles.add(path.resolve(file));
      else throw Error('nonregular capsule input: ' + file);
    }
  }
  scan(path.join(outputRoot, 's'));
  fail(actualFiles.size === inventory.length, 'unmanifested or missing capsule source files');
  for (const item of inventory) {
    const file = path.resolve(officeRoot, item.path);
    fail(inside(outputRoot, file), 'inventory path escaped capsule');
    fail(actualFiles.delete(file), 'missing or duplicate manifest source: ' + file);
    fail(hashFile(file) === item.sha256, 'capsule source drift: ' + file);
  }
  return {status: 'SOURCE_HASHES_MATCH_NOT_BUILD_ACCEPTANCE', files: inventory.length};
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const [command, configFile] = process.argv.slice(2);
    const config = JSON.parse(fs.readFileSync(configFile, 'utf8'));
    const result = command === 'create' ? createCapsule(config) : command === 'verify' ? verifyCapsule(config.outputRoot, config.officeRoot) : (()=>{throw Error('create|verify required');})();
    console.log(JSON.stringify(result));
  } catch (error) { console.error(error.stack); process.exitCode = 1; }
}
