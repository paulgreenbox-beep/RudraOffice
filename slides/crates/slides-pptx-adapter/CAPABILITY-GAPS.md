# slides-pptx-adapter capability gaps

## Deliberately supported subset

The default `PptxAdapter` uses the deterministic, bounded `RSLD001` codec. It is
an internal interchange representation, not a claim of PPTX compatibility. It
supports:

- presentation slide records;
- blank slide records with the API default slide size and no speaker notes;
- text, rectangle, and ellipse objects with finite bounds;
- UTF-8 text;
- stable little-endian encoding and strict trailing-byte rejection;
- configurable input, output, slide, shape, and text limits.

The adapter performs no filesystem or network I/O. Callers pass bytes in and
receive bytes out. The backend and codec traits are injectable so a format
implementation can be added without moving UI or editor/core ownership into
this crate.

## Explicit gaps

The built-in codec does **not** read or write real OOXML/PPTX packages. It does
not implement ZIP packaging, relationships, themes, masters, layouts, images,
tables, custom slide sizes, notes, charts, animations, transitions, comments,
speaker metadata, fonts, rich text runs, or compatibility behavior of Microsoft
PowerPoint. `RSLD001` specifically rejects a slide size other than
`slides_api::DEFAULT_SLIDE_SIZE` and any `Slide.notes` rather than silently
losing them.

`RSLD001` also serializes no object transform fields. It accepts only
`Transform::IDENTITY`—including zero rotation degrees—and rejects translation,
scale, or rotation changes at encode time rather than silently flattening them.

There is currently no real `ppt-rs` integration and no real
`msoffice-pptx-rs` integration. Those are explicit future backend work items,
not hidden fallbacks. A future implementation must be injected behind
`PresentationCodec`/`PresentationBackend`, preserve the `slides-api`
`PresentationEngine` contract, and retain bounded validation and deterministic
behavior where the underlying library permits it.
