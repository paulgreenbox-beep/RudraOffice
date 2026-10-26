//! Bounded, read-only probe of the production PDF adapter (never edits input PDFs).
use pdf_core::{PageId, PdfEngine};
use pdf_oxide_adapter::PdfOxideAdapter;
use std::{env, fs, path::PathBuf, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err("usage: reliability_probe INPUT.pdf FRESH_OUTPUT_DIR 1,8,16".into());
    }
    let input = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    let pages: Vec<u64> = args[2]
        .to_string_lossy()
        .split(',')
        .map(str::parse)
        .collect::<Result<_, _>>()?;
    if pages.is_empty() || pages.len() > 8 || pages.contains(&0) {
        return Err("probe requires 1..8 positive, one-based page numbers".into());
    }
    let size = fs::metadata(&input)?.len();
    if size > 64 * 1024 * 1024 {
        return Err("input exceeds the production 64 MiB admission bound".into());
    }
    let started = Instant::now();
    let bytes = fs::read(&input)?;
    println!(
        "byte_read=PASS bytes={} elapsed_ms={}",
        bytes.len(),
        started.elapsed().as_millis()
    );
    let mut adapter = PdfOxideAdapter::default();
    let artifact = adapter.parse_detached(&bytes)?;
    println!(
        "backend_parse=PASS page_count={} initial_previews={}",
        artifact.document_info().page_count,
        artifact.initial_previews().len()
    );
    let document = adapter.adopt_detached(artifact)?;
    println!("adoption=PASS document_id={:?}", document.document_id);
    // A fresh directory avoids overwriting previous evidence.
    fs::create_dir(&output)?;
    for number in pages {
        let page = PageId::new(number);
        let geometry = adapter.page_geometry(document.document_id, page)?;
        let started = Instant::now();
        let png = adapter.render_page(document.document_id, page, 1.0)?;
        let pixels = image::load_from_memory_with_format(&png, image::ImageFormat::Png)?.to_rgb8();
        let nonwhite = pixels
            .pixels()
            .filter(|pixel| pixel.0.iter().any(|channel| *channel < 245))
            .count();
        let file = output.join(format!("page-{number}.png"));
        fs::write(&file, &png)?;
        println!("raster=PASS page={number} points={}x{} pixels={}x{} rotation={} bytes={} nonwhite={} elapsed_ms={} output={}", geometry.width_points, geometry.height_points, pixels.width(), pixels.height(), geometry.rotation_degrees, png.len(), nonwhite, started.elapsed().as_millis(), file.display());
    }
    println!("NOTE: nonwhite pixels prove raster activity, NOT visual fidelity.");
    adapter.close_document(document.document_id)?;
    Ok(())
}
