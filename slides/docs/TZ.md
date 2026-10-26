# TZ: Rudra Slides

## Crates: slides-api, slides-core, slides-canvas, slides-ui, slides-pptx-adapter
## Direction: UI -> API -> Core -> Adapter -> Engine (PPT Engine / rust-pptx)
## Reference: experiments/pptx-engines/ppt-rs (MIT), experiments/pptx-engines/msoffice-pptx-rs (MIT OR Apache-2.0)

## MVP (TZ p15):
- Interface: thumbnail list, central canvas, properties panel, notes panel, Ribbon, status bar, zoom
- Slides: create presentation, add/delete/duplicate/reorder slide, choose layout, change background, thumbnails
- Objects MVP: text block, rectangle, ellipse, line, image, basic table
- Operations: create, select, move, resize, rotate, delete, copy, paste, layer order, alignment
- Canvas: GPU rendering, selection handles, bounding box, snapping, guides, grid, zoom, pan, hit testing
- Text in objects: edit, font, size, bold, italic, color, alignment, lists, vertical align
- Slideshow: start from first/current, fullscreen, keyboard navigation, simple transitions (no animations)

## Rules:
- Rust edition 2024, no external crates except std
- Each crate: Cargo.toml + src/lib.rs
- Focus on architecture, traits, types, logic
- PPTX adapter wraps ppt-rs or msoffice-pptx-rs behind PresentationEngine trait
- Canvas is Makepad GPU widget (slides-canvas crate)
