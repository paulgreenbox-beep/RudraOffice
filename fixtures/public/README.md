# Public synthetic regression corpus

These nine files are authored entirely by the checked-in generators. They are
first-party MIT test data under the repository LICENSE, not reduced user files.
No private document, external font program, stock image, screenshot or recovery
dump is an input. CRC/xref offsets and fixed ZIP metadata are generated from
code; no clock, username, random identifier or machine path enters the bytes.

| Files | Generator / provenance | Substantive coverage |
| --- | --- | --- |
| `writer/synthetic-basic.docx`, `synthetic-rich.docx`, `synthetic-grid.docx` | `tools/generate-public-writer-fixtures.mjs`; fixed OOXML and self-created pixel PNG | Cyrillic/emoji paragraphs, runs, page break, sections, imported header/footer, table/image, canonical history/export; additional 4-row/3-column grid retains public multi-row/column render-plan coverage |
| `writer/synthetic-editing-pages.docx` | `tools/generate-public-writer-fixtures.mjs`; fixed OOXML | Two adjacent styled Unicode paragraphs followed by a page break and a third paragraph; one-document page navigation and boundary-editing observation |
| `pdf/cid-unicode-space-control.pdf`, `cid-unicode-space-tw.pdf`, `cid-cyrillic.pdf` | `tools/generate-public-pdf-fixtures.mjs`; self-defined TrueType glyphs/metrics and PDF objects | Two-byte Identity-H CID/word-spacing regression and non-ASCII ToUnicode extraction; synthetic outlines are not a typography specimen |
| `slides/controlled-projection.pptx`, `supported-roundtrip.pptx` | `tools/generate-public-slides-fixtures.mjs`; fixed OOXML and self-created PNG | Three-slide shape/table/unknown-graphic regression; two-slide text/geometry/PNG import-export and selection |

From the repository root, regenerate only missing matching fixtures and check:

```text
node tools/generate-public-writer-fixtures.mjs
node tools/generate-public-writer-fixtures.mjs --check
node tools/generate-public-pdf-fixtures.mjs
node tools/generate-public-pdf-fixtures.mjs --check
node tools/generate-public-slides-fixtures.mjs
node tools/generate-public-slides-fixtures.mjs --check
```

Normal generation refuses differing existing fixture bytes. The Slides tool's
explicit `--force` is not used by verification. Keep the nine exact `.gitignore`
exceptions; do not replace them with a blanket document-format allowlist.

Sheets already has in-memory synthetic formula/style/merge/clipboard/structural
and XLSX codec tests; it requires no operator workbook for its owning tests.
PDF multipage, dense-vector resource-budget and malformed-byte cases are also
built directly by existing Rust test helpers. Their presence is not a test PASS.

Limits: the font in the PDF is deliberately synthetic, not a typography specimen.
Writer header/footer export fidelity is not claimed. A public corpus does not
replace native focus/clipboard/visual acceptance or representative opt-in real
documents. No L4b, RC or production release status follows from these files.
