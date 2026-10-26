// Epic-local source-copy/receipt helper. It never builds, launches GUI or merges.
import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";
import { fileURLToPath } from "node:url";

const COMPONENTS = new Set(["writer", "sheets", "slides", "pdf"]);
const SKIP_DIRS = new Set([".git", "target", "node_modules", "reports", "evidence", ".cache"]);
const EXTENSIONS = new Set([".rs", ".toml", ".lock", ".json", ".xml", ".md", ".txt", ".pdf", ".docx", ".xlsx", ".pptx", ".png", ".jpg", ".jpeg", ".svg", ".ttf", ".otf"]);
const hash = (bytes) => crypto.createHash("sha256").update(bytes).digest("hex").toUpperCase();
const within = (root, file) => { const rel = path.relative(root, file); return rel === "" || (!rel.startsWith("..") && !path.isAbsolute(rel)); };

export function rebaseManifest(text, originalFile, componentRoot) {
  return text.replace(/\bpath\s*=\s*"([^"\r\n]+)"/g, (matched, reference) => {
    const resolved = path.resolve(path.dirname(originalFile), reference);
    return within(componentRoot, resolved) ? matched : `path = ${JSON.stringify(resolved.replaceAll("\\", "/"))}`;
  });
}

function collect(root) {
  const files = [];
  function visit(dir) {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      if (SKIP_DIRS.has(entry.name) || entry.name.startsWith(".env")) continue;
      const file = path.join(dir, entry.name);
      if (entry.isSymbolicLink()) throw new Error(`Source symlink needs review: ${file}`);
      if (entry.isDirectory()) visit(file);
      else if (entry.isFile() && EXTENSIONS.has(path.extname(entry.name).toLowerCase())) {
        const relative = path.relative(root, file).replaceAll("\\", "/");
        if (relative === "Cargo.toml" || relative === "Cargo.lock" || relative.startsWith("crates/") || relative.startsWith("fixtures/")) {
          const bytes = fs.readFileSync(file);
          files.push({ file, relative, bytes, sha256: hash(bytes) });
        }
      }
    }
  }
  visit(root);
  const bytes = files.reduce((sum, file) => sum + file.bytes.length, 0);
  if (files.length > 1000 || bytes > 64 * 1024 * 1024) throw new Error("Source-copy budget exceeded; review selected package instead of copying more");
  return files;
}

function sharedFixtures(root, source) {
  const fixturesRoot = path.join(root, "fixtures");
  const files = new Map();
  const missing = new Set();
  for (const entry of source.filter((file) => file.relative.endsWith(".rs"))) {
    for (const match of entry.bytes.toString("utf8").matchAll(/include_(?:bytes|str)!\s*\(\s*"([^"\r\n]+)"/g)) {
      const file = path.resolve(path.dirname(entry.file), match[1]);
      if (!within(fixturesRoot, file)) continue;
      if (!fs.existsSync(file)) { missing.add(path.relative(root, file).replaceAll("\\", "/")); continue; }
      if (!fs.statSync(file).isFile()) throw new Error(`Shared fixture is not a file: ${file}`);
      const bytes = fs.readFileSync(file);
      files.set(file, { file, relative: path.relative(root, file).replaceAll("\\", "/"), bytes, sha256: hash(bytes) });
    }
  }
  return { files: [...files.values()], missing: [...missing].sort() };
}

export function createStage(root, component, attempt) {
  if (!COMPONENTS.has(component) || !/^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(attempt)) throw new Error("Invalid component/attempt");
  root = path.resolve(root);
  const originalRoot = path.join(root, component);
  const stageRoot = path.join(root, "reports", "parallel-development", "staging", `${component}-${attempt}`);
  if (fs.existsSync(stageRoot)) throw new Error("Refusing to overwrite existing stage");
  const source = collect(originalRoot);
  const fixtures = sharedFixtures(root, source);
  const auxiliary = fixtures.files;
  if (source.length + auxiliary.length > 1000 || [...source, ...auxiliary].reduce((sum, file) => sum + file.bytes.length, 0) > 64 * 1024 * 1024) throw new Error("Source and shared-fixture budget exceeded");
  const entries = source.map((file) => {
    const staged = path.basename(file.file) === "Cargo.toml"
      ? Buffer.from(rebaseManifest(file.bytes.toString("utf8"), file.file, originalRoot)) : file.bytes;
    return { relative: file.relative, bytes: file.bytes.length, source_sha256: file.sha256, staged_sha256: hash(staged), staged, original: file.file };
  });
  // All reads/budget checks precede creation. Never fall back to overwriting.
  fs.mkdirSync(stageRoot, { recursive: true });
  for (const entry of entries) {
    const destination = path.join(stageRoot, component, entry.relative);
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    fs.writeFileSync(destination, entry.staged, { flag: "wx" });
    if (hash(fs.readFileSync(entry.original)) !== entry.source_sha256) throw new Error("Source changed during capture; stage is not admitted");
  }
  for (const entry of auxiliary) {
    const destination = path.join(stageRoot, entry.relative);
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    fs.writeFileSync(destination, entry.bytes, { flag: "wx" });
    if (hash(fs.readFileSync(entry.file)) !== entry.sha256) throw new Error("Shared fixture changed during capture; stage is not admitted");
  }
  const manifest = {
    schema_version: 1, component, attempt, production_root: root, stage_root: stageRoot,
    source_isolation: "component source copy; external dependencies read-only by owner policy, NOT OS sandbox",
    generated_manifest_rebases_are_not_production_changes: true,
    files: entries.map(({ staged, original, ...entry }) => entry),
    auxiliary_fixtures: auxiliary.map((entry) => ({ relative: entry.relative, bytes: entry.bytes.length, sha256: entry.sha256, ownership: "read-only shared test fixture; never merge" })),
    missing_shared_fixtures: fixtures.missing,
    total_source_bytes: source.reduce((sum, file) => sum + file.bytes.length, 0),
  };
  fs.writeFileSync(path.join(stageRoot, "baseline.json"), JSON.stringify(manifest, null, 2) + "\n", { flag: "wx" });
  return manifest;
}

export function compareStage(manifestPath) {
  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  if (manifest.schema_version !== 1 || !COMPONENTS.has(manifest.component)) throw new Error("Invalid stage manifest");
  const stageRoot = path.resolve(manifest.stage_root);
  const allowed = path.resolve(manifest.production_root, "reports", "parallel-development", "staging");
  if (!within(allowed, stageRoot) || stageRoot === allowed || path.resolve(path.dirname(manifestPath)) !== stageRoot) throw new Error("Stage escaped declared workspace");
  const currentRoot = path.join(stageRoot, manifest.component);
  const changes = [], conflicts = [];
  for (const entry of manifest.auxiliary_fixtures ?? []) {
    const original = path.resolve(manifest.production_root, entry.relative);
    const staged = path.resolve(stageRoot, entry.relative);
    if (!within(path.resolve(manifest.production_root, "fixtures"), original) || !within(path.join(stageRoot, "fixtures"), staged)) throw new Error("Shared fixture escaped declared root");
    if (!fs.existsSync(original) || hash(fs.readFileSync(original)) !== entry.sha256 || !fs.existsSync(staged) || hash(fs.readFileSync(staged)) !== entry.sha256) conflicts.push(entry.relative);
  }
  const seen = new Set();
  for (const entry of manifest.files) {
    const original = path.resolve(manifest.production_root, manifest.component, entry.relative);
    const staged = path.resolve(currentRoot, entry.relative);
    if (!within(path.resolve(manifest.production_root, manifest.component), original) || !within(currentRoot, staged)) throw new Error("Receipt path escaped owner component");
    seen.add(entry.relative);
    if (!fs.existsSync(original) || hash(fs.readFileSync(original)) !== entry.source_sha256) conflicts.push(entry.relative);
    if (!fs.existsSync(staged)) changes.push({ relative: entry.relative, kind: "deleted", baseline: entry.source_sha256 });
    else {
      const current = hash(fs.readFileSync(staged));
      if (current !== entry.staged_sha256) changes.push({ relative: entry.relative, kind: "modified", baseline: entry.source_sha256, staged_sha256: current });
    }
  }
  for (const file of collect(currentRoot)) {
    if (!seen.has(file.relative)) changes.push({ relative: file.relative, kind: "added", staged_sha256: file.sha256 });
  }
  return { component: manifest.component, status: conflicts.length ? "SOURCE_CONFLICT_REVIEW_REQUIRED" : "READY_FOR_MANUAL_REVIEW", conflicts, changes, missing_shared_fixtures: manifest.missing_shared_fixtures ?? [], automatic_merge: false };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const [, , command, component, attempt] = process.argv;
    const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
    const result = command === "stage" ? createStage(root, component, attempt)
      : command === "compare" ? compareStage(path.resolve(component)) : (() => { throw new Error("usage: parallel-work-package.mjs stage COMPONENT ATTEMPT | compare BASELINE.json"); })();
    console.log(JSON.stringify(command === "stage" ? { stage_root: result.stage_root, files: result.files.length, source_bytes: result.total_source_bytes, auxiliary_fixtures: result.auxiliary_fixtures.length, missing_shared_fixtures: result.missing_shared_fixtures } : result));
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
