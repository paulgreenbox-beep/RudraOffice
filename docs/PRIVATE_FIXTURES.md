# Private and missing acceptance fixtures

No DOCX/XLSX/PDF/PPTX file from the working project was copied into this public baseline, including reduced documents derived from user input. Test source and assertions remain present.

The following compile-time inputs are referenced by tests and must not be mistaken for public test coverage:

- Writer API/DOCX tests: fixtures/release-acceptance/docx/writer-layout-a4-multipage.docx and writer-visual-fidelity-fixture.docx. One local controlled document also embeds an image with unreviewed independent provenance; another is already missing from the current development tree.
- PDF word-spacing regression: fixtures/pdf-reliability/cid-word-spacing/*.pdf. These contain a text run and embedded font reduced from a user document, not a wholly synthetic/licensed public fixture. Both documents and their identifying README are excluded.
- Slides adapter test: fixtures/release-acceptance/pptx/controlled-acceptance.pptx is already missing from the current development tree.

Because the retained tests use include_bytes!, full affected test targets may fail at compilation without this corpus. The baseline does not hide that by deleting tests, dropping assertions, changing production logic, embedding placeholders or reporting false PASS. Pure packages without these inputs can be checked independently.

A future public fixture replacement must be generated independently with reviewed image/font licenses and preserve the actual regression trigger/assertions. Do not upload the operator's real corpus or embedded proprietary fonts. Keep operator-provided inputs outside version control; .gitignore rejects the document formats by default.

Historical GUI recipes use sanitized template paths and retained diagnostic RC identities. They are examples until bound to fresh operator-owned fixtures and a qualified executable. Recipe validation is not execution or acceptance.
