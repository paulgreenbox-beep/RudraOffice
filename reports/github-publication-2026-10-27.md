# GitHub publication — 2026-10-27

Status: **PUBLICATION — COMPLETE** (Initial Public Source Baseline; not a production release).

## GitHub

- Owner / repository: paulgreenbox-beep/RudraOffice.
- URL: https://github.com/paulgreenbox-beep/RudraOffice.
- Visibility: PUBLIC, independently verified by anonymous GitHub API and ordinary HTTP requests.
- Default branch: main.
- Initial commit: **8766b97b3648aab124a0ecb02e7437aa00b5197f**.
- Message: Initial public source release of Rudra Office.
- Initial Git tree: d0b18d7f6d86781a9dd541fcfbd56aa85b53350d.
- First verified public tree: 1,449 files, 32,698,922 source bytes as reported by GitHub Git blobs; no truncated tree or gitlinks.
- A separate documentation-only follow-up records this observed publication and adds the machine-readable receipt. No force push, tag or GitHub Release is used.

## Source and topology

Real Writer, Sheets, Slides, PDF, shared application, separate launcher, session/runtime facade, adapters, Cargo manifests, lockfiles, tests, verification tooling and architecture documentation are present. Publication is not README-only. Source came from the current filesystem, including uncommitted first-party module work, not only from an incomplete nested Git index.

The source was physically copied into a new RudraOffice-GitHub-Publish staging directory through an explicit allowlist. No blind copy of the full project followed by git add was performed. Git added exactly the independently reviewed manifest of 1,449 files. Build output, evidence/capsules, reference projects and Git metadata were excluded before staging.

The original slides/.git is an unborn master repository with two staged/modified source files and additional untracked work. It was not removed, moved, committed or turned into a public gitlink. Its current source is included as normal first-party files. The original project root has no .git, and remains that way.

Dependencies are directional: Rudra Office consumes the separate sibling Toolkit, and Toolkit consumes upstream Makepad. Neither Toolkit nor the unused old vendor/makepad is duplicated. The three required local engine source snapshots are included with original licensing and explicit local-modification notices. See THIRD_PARTY_NOTICES.md and third_party/source-inventory.json.

## License

The owner explicitly authorized public publication of first-party code under MIT. Root LICENSE contains the complete standard MIT text, copyright 2026 paulgreenbox-beep and Rudra Office contributors. First-party Cargo license declarations in this staging snapshot were aligned to MIT; upstream manifests/licenses were not relicensed.

IronCalc, Office Oxide and PDFOxide retain their own MIT OR Apache-2.0 declarations, copyright, license/NOTICE and trademark files. Eleven included locally modified engine source files have prominent modification notices. DejaVu, Droid/AOSP and Noto fonts retain separate notices; the full Noto OFL text was fetched from a pinned official Google repository revision. References with AGPL or unknown license status were excluded, not relabeled MIT.

This is technical license/source inventory, not a title opinion or complete legal clearance of every transitive dependency or historical font binary version.

## Security and privacy

- Explicit source allowlist; no user DOCX/XLSX/PDF/PPTX bytes, including reduced real-document reproducers, were copied.
- No EXE/DLL/PDB/OBJ/WASM, target/cache, .env, credential/key files, local evidence, agent progress note or nested .git was committed.
- Personal usernames and identifying private fixture names were removed from staged example paths/comments. GUI recipes are templates, not qualified runnable public bindings.
- Pattern scans checked tokens, credential assignments/URLs, complete private-key blocks and known local/private identifiers. No unresolved findings remained. The two PEM-format markers in upstream API documentation are format descriptions, not actual private keys.
- Scans and exact file-list checks were repeated before the first commit/push. No credential values were printed or committed. A pattern scan is not a guarantee against every possible secret.
- .gitignore was tested against representative secrets, documents, binaries, build output and capsules, and checked not to exclude Cargo manifests/lockfiles, source, documentation or verification tools.

## Build and CI

Six staged Cargo entry manifests passed `cargo metadata --locked --offline --no-deps --format-version 1` using the existing local dependency environment: shared workspace, Writer, Sheets, Slides, PDF core and PDF adapter. Lockfiles were not regenerated. This proves manifest/path readability in that environment, not compilation or a clean public checkout.

Full build, strict Clippy, all component tests, GUI/RC acceptance and clean-machine/public-only bootstrap were **not claimed** by publication. No knowingly broken mandatory GitHub Actions workflow was added.

Known bootstrap limitations are explicit in BUILDING.md:

1. Public Toolkit base c3b168ba10255ba644bd3a0d91524f3cd23cbf6b differs from the dirty development checkout; the required development verification-economy core is absent from that public tree.
2. The operator-managed Makepad junction lacks independent Git metadata. Its enclosing unrelated repository HEAD is not a Makepad revision. The Toolkit bootstrap pin is recorded, but its Rudra compatibility is not qualified.
3. Full Writer DOCX, PDF adapter and Slides adapter test targets reference omitted/private or already-missing binary fixtures through include_bytes!. Assertions remain unchanged; those targets are not reported green. No fabricated replacement corpus or validator bypass was introduced.

## Preservation

- Fresh source-hash checks confirmed all 1,439 copied original selected source/vendor files remained unchanged in the working project.
- All four inventoried original fixture files remained hash-identical and were not uploaded.
- All seven original frozen release EXEs were fingerprinted and confirmed unchanged after the first public push.
- Original Slides Git directory, branch and two staged/modified entries were preserved; no original history/index was intentionally rewritten.
- Original root Git absence, configuration, vendor code, executable candidates and document bytes were not changed for publication. All MIT metadata, privacy examples, new documentation and vendor notice additions are staging-only.

Private read-only inventory/scan/hash receipts remain outside the public repository. The public machine-readable receipt contains bounded counts, verdicts and public source identities, not private paths or documents.

## Independent public verification

Anonymous requests (no Authorization header or browser cookies) returned the public repository metadata, main commit, full recursive Git tree, pinned README, complete MIT LICENSE and ordinary repository HTML. The observed main SHA matched the initial local commit. Required component source and Cargo files were present; no prohibited document/artifact files or nested gitlinks appeared. The Git tree was not truncated.

The documentation follow-up must be verified separately against the remote head; the initial SHA above intentionally remains the initial source commit, not a self-referential current-document hash.

## Remaining issues and next workflow

Source publication is complete. Portable dependency/bootstrap qualification, private fixture replacement and PARALLEL CYCLE 2 functional closure remain unfinished. Writer focus, PDF dense-vector rendering, Sheets clipboard GUI and full Slides editing remain subject to their own acceptance; there is no production-ready/full Office compatibility claim.

Keep main integrated; open focused Writer/Sheets/Slides/PDF branches and Pull Requests as work is actually needed. No empty branch set, executable Release, release tag or new development cycle was started automatically.
