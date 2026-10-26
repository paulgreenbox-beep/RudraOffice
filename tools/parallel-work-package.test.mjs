import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { createStage, compareStage, rebaseManifest } from "./parallel-work-package.mjs";

test("only external Cargo paths are rebased; owned source paths stay local", () => {
  const root = path.resolve("test-source", "writer");
  const file = path.join(root, "crates", "writer-api", "Cargo.toml");
  const text = '[lib]\npath = "src/lib.rs"\n[dependencies]\ncore = { path = "../writer-core" }\nengine = { path = "../../../vendor/office-oxide" }\n';
  const result = rebaseManifest(text, file, root);
  assert.match(result, /path = "src\/lib.rs"/);
  assert.match(result, /path = "\.\.\/writer-core"/);
  assert.ok(result.includes(path.resolve(path.dirname(file), "../../../vendor/office-oxide").replaceAll("\\", "/")));
});

test("fresh bounded stage preserves original and detects conflict without merging", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "rudra-stage-test-"));
  try {
    const original = path.join(root, "writer", "crates", "writer-core", "src", "lib.rs");
    fs.mkdirSync(path.dirname(original), { recursive: true });
    fs.writeFileSync(original, "pub fn value() -> u32 { 1 }\n");
    const baseline = createStage(root, "writer", "unit");
    const receipt = path.join(baseline.stage_root, "baseline.json");
    assert.equal(compareStage(receipt).changes.length, 0);
    assert.throws(() => createStage(root, "writer", "unit"), /overwrite/);
    const staged = path.join(baseline.stage_root, "writer", "crates", "writer-core", "src", "lib.rs");
    fs.appendFileSync(staged, "// isolated change\n");
    assert.equal(compareStage(receipt).changes.length, 1);
    assert.equal(fs.readFileSync(original, "utf8"), "pub fn value() -> u32 { 1 }\n");
    fs.appendFileSync(original, "// foreign work\n");
    assert.equal(compareStage(receipt).status, "SOURCE_CONFLICT_REVIEW_REQUIRED");
    assert.equal(compareStage(receipt).automatic_merge, false);
    assert.throws(() => createStage(root, "writer", "../escape"), /Invalid/);
  } finally {
    // Test owns exactly this fresh temporary directory, never a user project.
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test("shared include fixtures are copied with hashes outside the owned component", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "rudra-fixture-test-"));
  try {
    const original = path.join(root, "pdf", "crates", "adapter", "tests", "public.rs");
    const fixture = path.join(root, "fixtures", "pdf", "minimal.pdf");
    fs.mkdirSync(path.dirname(original), { recursive: true });
    fs.mkdirSync(path.dirname(fixture), { recursive: true });
    fs.writeFileSync(original, 'const PDF: &[u8] = include_bytes!("../../../../fixtures/pdf/minimal.pdf");');
    fs.appendFileSync(original, '\nconst ABSENT: &[u8] = include_bytes!("../../../../fixtures/pdf/absent.pdf");');
    fs.writeFileSync(fixture, "%PDF-1.7\n");
    const baseline = createStage(root, "pdf", "unit");
    assert.equal(baseline.auxiliary_fixtures.length, 1);
    assert.deepEqual(baseline.missing_shared_fixtures, ["fixtures/pdf/absent.pdf"]);
    assert.equal(fs.readFileSync(path.join(baseline.stage_root, "fixtures/pdf/minimal.pdf"), "utf8"), "%PDF-1.7\n");
    const receipt = path.join(baseline.stage_root, "baseline.json");
    assert.equal(compareStage(receipt).conflicts.length, 0);
    fs.appendFileSync(path.join(baseline.stage_root, "fixtures/pdf/minimal.pdf"), "changed");
    assert.deepEqual(compareStage(receipt).conflicts, ["fixtures/pdf/minimal.pdf"]);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});
