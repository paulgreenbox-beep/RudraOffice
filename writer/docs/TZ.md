# TZ: Rudra Writer

## Crates: writer-api, writer-core, writer-ui, writer-rudra-adapter, writer-docx
## Direction: UI -> API -> Core -> Adapter -> Engine

## MVP (TZ p13):
- create doc, input/delete text, selection, cursor, copy/paste, undo/redo
- font, size, bold, italic, underline, strikethrough, text color, bg color
- alignment, line spacing, indents, bullet/numbered lists, heading styles
- insert images (PNG/JPEG/WebP), basic tables, search/replace, zoom
- save DOCX, export PDF/Markdown/TXT
- page mode + continuous mode
- sidebar: headings, structure, matches, pages

## Tables MVP: insert, add/remove rows/cols, resize, borders, cell bg, alignment
## Images: PNG/JPEG/WebP, resize, proportions, align, delete, copy

## Rules:
- Rust edition 2024, no external crates except std
- Each crate: Cargo.toml + src/lib.rs
- Focus on architecture, traits, types, logic
- MSVC missing - cargo check may fail on build.rs

## Architecture Decisions

### ARCH-WRITER-003 (supersedes ARCH-WRITER-002 dependency wording) — 2026-08-09

1. **Crates remain std-only**: `writer-core`, `writer-api`, `writer-rudra-adapter`, `writer-ui` remain std-only direct-dependency crates.

2. **writer-docx exception**: `writer-docx` alone may directly declare vendored `office_oxide` 0.1.8 via `../../../vendor/office-oxide` and registry `zip ^8.6` with `default-features=false` and manifest features exactly `["deflate"]`.

3. **Locked transitive closure only**: The exception includes ONLY the locked transitive closure required by those two dependencies, audited whenever the lockfile or vendor source changes. `zip`'s upstream expansion of the declared `deflate` feature is allowed, but no additional manifest feature names or new direct external dependencies are allowed without a new architecture decision.

4. **Reproducible-production status: CONDITIONAL/NOT PASSED** — until Office Oxide is a clean, immutable, content-addressed source (pinned SHA/tag or verified cargo-vendor tree) and the Writer workspace gains an auditable VCS baseline. Current vendor observed `b5819b19913ecf3f8295741244b907ea2de14e13-dirty`, 6 uncommitted files; do not alter it.

5. **Offline audit 2026-08-09**: lockfile resolves/validates offline, `cargo audit --no-fetch` reports zero advisories from local DB (freshness unverified), `cargo deny` defaults passed but deny.toml absent.

6. **Documentation must not claim** integration/merge/CI pass or fresh online audit.

### Unresolved Blockers (from ARCH-WRITER-003)

- **Blocker 1 — Dirty vendor provenance**: `vendor/office-oxide` is at `b5819b19913ecf3f8295741244b907ea2de14e13-dirty` with 6 uncommitted files — not a clean, immutable, content-addressed source.

- **Blocker 2 — Non-Git Writer workspace**: The Writer workspace has no auditable VCS baseline (not a Git repo), preventing reproducible-production status.