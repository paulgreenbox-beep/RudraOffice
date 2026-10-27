# Private and missing acceptance fixtures

No user DOCX/XLSX/PDF/PPTX or reduced user-derived document is part of the public corpus. Test source and original private-fixture assertions remain present.

RDO-002 adds an independently generated, narrowly allowlisted corpus under
`fixtures/public/`. See its `README.md` and `manifest.json` for exact provenance,
generator commands and SHA-256 values. No operator document, external embedded
font or third-party image was used to create these seven files.

Default Writer tests use synthetic documents. Original private-layout/fidelity
tests require the explicit `private-fixtures` feature in `writer-api` or
`writer-docx`; enabling it still requires the original private files below.
The private PDF CID regression is an explicitly ignored, environment-selected
runtime test, not a compile-time include. The public CID regression uses a
self-authored minimal font. The missing controlled Slides document is replaced
by a synthetic shape/table/unsupported-graphic package without dropping its
existing structural assertions. Additional import/export and selection tests
use a second synthetic PPTX.

These changes make fixture ownership explicit; they do not establish successful
Cargo, renderer, GUI or full round-trip acceptance. In particular, imported
Writer header/footer metadata is tested, but its export fidelity remains a
known limitation. See the task's actual test receipts before claiming PASS.

The following historical private inputs remain opt-in and must not be mistaken for public test coverage:

- Writer API/DOCX tests: fixtures/release-acceptance/docx/writer-layout-a4-multipage.docx and writer-visual-fidelity-fixture.docx. One local controlled document also embeds an image with unreviewed independent provenance; another is already missing from the current development tree.
- PDF word-spacing regression: fixtures/pdf-reliability/cid-word-spacing/*.pdf. These contain a text run and embedded font reduced from a user document, not a wholly synthetic/licensed public fixture. Both documents and their identifying README are excluded.
- Slides adapter test: fixtures/release-acceptance/pptx/controlled-acceptance.pptx is already missing from the current development tree.

Default targets no longer deliberately include these absent private bytes at compile time. Explicit private-fixture runs still require their original files at runtime. Actual compilation, rendering and test results remain separate gates; retaining source assertions does not establish a PASS.

Future corpus additions must be generated independently with reviewed image/font provenance and preserve actual regression triggers/assertions. Do not upload the operator's real corpus or embedded proprietary fonts. Keep operator-provided inputs outside version control; .gitignore rejects document formats except the seven exact reviewed synthetic paths.

Historical GUI recipes use sanitized template paths and retained diagnostic RC identities. They are examples until bound to fresh operator-owned fixtures and a qualified executable. Recipe validation is not execution or acceptance.
