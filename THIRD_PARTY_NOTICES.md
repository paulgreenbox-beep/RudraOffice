# Third-party notices and technical license inventory

The root MIT license applies to first-party Rudra Office code, not to third-party code or assets. Original upstream copyrights, notices and license texts are preserved. This is a bounded technical inventory, **not full legal approval or an audit of every transitive dependency**.

## Included vendored Rust components

| Component | Version / upstream base | Declared license | Preserved notices |
| --- | --- | --- | --- |
| [IronCalc](https://github.com/ironcalc/ironcalc) | ironcalc / ironcalc_base 0.8.3; `879e71ddb6d4b0babaf88c21ce657e0abfecfdda` | MIT OR Apache-2.0 | vendor/ironcalc/LICENSE-MIT; LICENSE-Apache-2.0 |
| [Office Oxide](https://github.com/yfedoseev/office_oxide) | 0.1.8; `b5819b19913ecf3f8295741244b907ea2de14e13` | MIT OR Apache-2.0 | vendor/office-oxide/LICENSE-MIT; LICENSE-APACHE; NOTICE; TRADEMARKS.md |
| [PDFOxide](https://github.com/yfedoseev/pdf_oxide) | 0.3.77; `10b87f153200cd5c4d4a4defee471757091e6559` | MIT OR Apache-2.0 | vendor/pdf-oxide/LICENSE-MIT; LICENSE-APACHE; NOTICE; TRADEMARKS.md |

IronCalc notices identify EqualTo GmbH and Nicolas Hatcher; Oxide notices identify Yury Fedoseev and contributors. Exact upstream spelling/copyright statements remain in the preserved files. Rudra Office does not claim ownership of those libraries. Existing trademark notices are retained; no endorsement or trademark license is asserted by this publication.

These are **current Rust source snapshots with local integration changes**, not clean clones at the listed upstream base commits. Required source/data and Rust examples/workspace member manifests are included; upstream binary document corpora, platform bindings not needed by this consumer, generated outputs and development histories are not a redistribution of the entire upstream projects.

Local modifications included in this snapshot:

- IronCalc: base/src/expressions/parser/stringify.rs; base/src/expressions/parser/tests/test_general.rs; xlsx/src/import/styles.rs; xlsx/src/import/util.rs.
- Office Oxide: src/convert_docx.rs; src/convert_pptx.rs; src/create.rs; src/pptx/shape.rs; src/pptx/slide.rs; src/pptx/text.rs.
- PDFOxide: src/rendering/text_rasterizer.rs. A separate modified upstream private-corpus test is not copied.

Each included modified Rust file has a prominent local-modification comment while retaining original licensing. See [machine-readable inventory](third_party/source-inventory.json). Replacing these snapshots with arbitrary upstream revisions is not qualified by this publication.

## Bundled PDF font assets

The existing vendor source uses these assets; they are source resources, not executable artifacts or user documents:

- DejaVuSans.ttf / DejaVuSans-Bold.ttf: Bitstream/DejaVu terms; complete local notice vendor/pdf-oxide/src/fonts/assets/LICENSE-DejaVu is preserved.
- DroidSansFallbackFull.ttf: Google/AOSP, Apache-2.0 per the preserved LICENSE-fallback-fonts attribution; full Apache text is preserved in vendor/pdf-oxide/LICENSE-APACHE.
- NotoEmoji-Regular.ttf: Google and contributors, SIL Open Font License 1.1 per the preserved LICENSE-fallback-fonts attribution. The full official OFL text is additionally preserved in third_party/licenses/noto-emoji/LICENSE-OFL-1.1, copied from [Google's Noto Emoji repository](https://github.com/googlefonts/noto-emoji/blob/e20cbc2bbec1926686be9f9bee7d1d2cfa1fea0e/2D/fonts/LICENSE).

Asset SHA-256 values are recorded in third_party/source-inventory.json. The upstream fallback notice is the basis for font-origin/license declarations; a complete historical binary-to-upstream font-version audit is not claimed. Do not strip these notices or treat the fonts as Rudra Office MIT code.

## Separate dependencies, not copied

- [Rust Makepad Toolkit](https://github.com/paulgreenbox-beep/rust-makepad-toolkit): MIT, public base `c3b168ba10255ba644bd3a0d91524f3cd23cbf6b`; maintain a sibling checkout. Local development differences remain unpublished here.
- [Makepad](https://github.com/makepad/makepad): MIT OR Apache-2.0; remains an upstream Toolkit dependency. Public Toolkit bootstrap default is `105158e08c89d47f57af4621420ed68eb3a147ce`. The current operator SDK's own exact Git revision is unverified; see BUILDING. The unused older vendor/makepad source is intentionally not duplicated here.
- Cargo registry dependencies remain specified in manifests and lockfiles, not vendored by this publication. Their own terms remain applicable; no transitive license clearance is asserted.

## Excluded references and private material

Rudra-Editor (AGPL-3.0), pdforg (unknown local license status), Freecell and experimental/reference engines are not included as Rudra Office dependencies. In particular, no AGPL reference source is relabeled MIT. The first-party writer-rudra-adapter imports the existing first-party writer-core, not the excluded reference repository.

User DOCX/XLSX/PDF/PPTX, reduced real-document reproducers, private embedded fonts/images, local evidence, executable candidates, caches, credentials, agent progress notes and nested .git metadata are not included. Their absence is not proof that private acceptance scenarios are runnable or passing publicly.
