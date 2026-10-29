import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const base = path.join(root, 'fixtures/public');
const manifest = JSON.parse(fs.readFileSync(path.join(base, 'manifest.json'), 'utf8'));
const ignore = fs.readFileSync(path.join(root, '.gitignore'), 'utf8').split(/\r?\n/);

test('public corpus has unique bounded relative paths and explicit provenance', () => {
  assert.equal(manifest.license, 'MIT');
  assert.equal(manifest.fixtures.length, 9);
  assert.match(manifest.origin, /synthetic/);
  assert.equal(new Set(manifest.fixtures.map(f => f.path)).size, manifest.fixtures.length);
  for (const f of manifest.fixtures) {
    assert.match(f.path, /^(writer|pdf|slides)\/[a-z0-9-]+\.(docx|pdf|pptx)$/);
    assert.match(f.generator, /^tools\/generate-public-(writer|pdf|slides)-fixtures\.mjs$/);
    assert.ok(fs.lstatSync(path.join(root, f.generator)).isFile());
    assert.ok(f.bytes > 0 && f.bytes < 64 * 1024);
  }
});

for (const fixture of manifest.fixtures) {
  test(`exact synthetic bytes and .gitignore exception: ${fixture.path}`, () => {
    const file = path.join(base, fixture.path);
    assert.ok(fs.lstatSync(file).isFile(), 'not a directory or symlink');
    const bytes = fs.readFileSync(file);
    assert.equal(bytes.length, fixture.bytes);
    assert.equal(createHash('sha256').update(bytes).digest('hex').toUpperCase(), fixture.sha256);
    assert.ok(ignore.includes(`!fixtures/public/${fixture.path}`));
  });
}

test('no unmanifested document files in public corpus', () => {
  const actual = [];
  for (const category of ['writer', 'pdf', 'slides']) {
    for (const name of fs.readdirSync(path.join(base, category))) actual.push(`${category}/${name}`);
  }
  assert.deepEqual(actual.sort(), manifest.fixtures.map(f => f.path).sort());
});

for (const category of ['writer', 'pdf', 'slides']) {
  test(`deterministic ${category} generator matches without modifying files`, () => {
    const result = spawnSync(process.execPath, [`tools/generate-public-${category}-fixtures.mjs`, '--check'], {
      cwd: root, encoding: 'utf8', timeout: 15000, windowsHide: true,
    });
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
  });
}
