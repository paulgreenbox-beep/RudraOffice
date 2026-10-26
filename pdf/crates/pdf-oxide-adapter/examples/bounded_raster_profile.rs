//! Bounded, standalone profile for one guarded PDF page render.
//!
//! The `job.render()` interval is intentionally opaque: it includes the
//! guarded page-content decode/reject path, rasterization, cropping, and PNG
//! encoding. For page 30 it may measure only the guarded decode/reject path;
//! it is not a parser/raster hotspot measurement or proof of nontruncated
//! pixel fidelity. This runs synchronously in this standalone CPU process,
//! outside the application UI and `PdfRenderWorker`.

use std::env;
use std::ffi::OsStr;
use std::fmt::Write as FmtWrite;
use std::fs::File;
use std::io::{self, Read, Write as IoWrite};
use std::path::Path;
use std::time::{Duration, Instant};

use image::GenericImageView;
use pdf_core::{PageGeometry, PageId, PdfEngine};
use pdf_oxide_adapter::{PdfOxideAdapter, PdfRenderRequest};

const MAX_SOURCE_BYTES: usize = 64 * 1024 * 1024;
const MAX_RENDER_PIXELS: u64 = 16_000_000;
const MAX_RENDER_RASTER_BYTES: u64 = 48_000_000;
const MAX_RENDER_PNG_BYTES: usize = 64 * 1024 * 1024;
const MAX_JSON_RECORD_BYTES: usize = 4095;
const MAX_INPUT_LABEL_CHARS: usize = 256;
const MAX_ERROR_CAUSE_CHARS: usize = 512;
const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

#[derive(Default)]
struct Timings {
    read: Option<Duration>,
    parse_detached: Option<Duration>,
    adoption: Option<Duration>,
    page_geometry: Option<Duration>,
    prepare_render: Option<Duration>,
    job_render: Option<Duration>,
    png_decode: Option<Duration>,
    nonwhite_analysis: Option<Duration>,
}

struct Report {
    status: &'static str,
    stage: &'static str,
    input_label: String,
    source_bytes: Option<usize>,
    source_bytes_exact: Option<bool>,
    page: Option<u64>,
    scale: Option<f32>,
    geometry: Option<PageGeometry>,
    png_bytes: Option<usize>,
    png_width: Option<u32>,
    png_height: Option<u32>,
    pixel_count: Option<u64>,
    nonwhite_pixels: Option<u64>,
    timings: Timings,
    error: Option<String>,
}

impl Report {
    fn new(input_label: String) -> Self {
        Self {
            status: "INPUT_ERROR",
            stage: "arguments",
            input_label,
            source_bytes: None,
            source_bytes_exact: None,
            page: None,
            scale: None,
            geometry: None,
            png_bytes: None,
            png_width: None,
            png_height: None,
            pixel_count: None,
            nonwhite_pixels: None,
            timings: Timings::default(),
            error: None,
        }
    }

    fn fail(
        mut self,
        status: &'static str,
        stage: &'static str,
        error: impl Into<String>,
        exit_code: u8,
    ) -> (Self, u8) {
        self.status = status;
        self.stage = stage;
        self.error = Some(error.into());
        (self, exit_code)
    }
}

fn main() {
    let (report, exit_code) = run();
    let record = bounded_json_record(&report);
    let mut stdout = io::stdout().lock();
    let _ = stdout.write_all(record.as_bytes());
    let _ = stdout.write_all(b"\n");
    std::process::exit(exit_code as i32);
}

fn run() -> (Report, u8) {
    let mut arguments = env::args_os().skip(1);
    let Some(input_arg) = arguments.next() else {
        return Report::new(String::new()).fail(
            "INPUT_ERROR",
            "arguments",
            "usage: bounded_raster_profile INPUT.pdf PAGE [SCALE] (use - for stdin)",
            1,
        );
    };
    let input_label = if input_arg == OsStr::new("-") {
        "<stdin>".to_owned()
    } else {
        input_arg.to_string_lossy().into_owned()
    };
    let mut report = Report::new(input_label);

    let Some(page_arg) = arguments.next() else {
        return report.fail(
            "INPUT_ERROR",
            "arguments",
            "usage: bounded_raster_profile INPUT.pdf PAGE [SCALE] (use - for stdin)",
            1,
        );
    };
    let Some(page) = page_arg
        .to_str()
        .and_then(|value| value.parse::<u64>().ok())
    else {
        return report.fail(
            "INPUT_ERROR",
            "arguments",
            "PAGE must be a positive integer",
            1,
        );
    };
    if page == 0 {
        return report.fail(
            "INPUT_ERROR",
            "arguments",
            "PAGE must be a positive integer",
            1,
        );
    }
    report.page = Some(page);

    let scale = match arguments.next() {
        Some(scale_arg) => match scale_arg
            .to_str()
            .and_then(|value| value.parse::<f32>().ok())
        {
            Some(scale) => scale,
            None => {
                return report.fail(
                    "INPUT_ERROR",
                    "arguments",
                    "SCALE must be a finite number accepted by PdfRenderRequest",
                    1,
                );
            }
        },
        None => 1.0,
    };
    report.scale = Some(scale);
    if arguments.next().is_some() {
        return report.fail(
            "INPUT_ERROR",
            "arguments",
            "expected INPUT.pdf PAGE [SCALE]",
            1,
        );
    }

    let mut source = Vec::new();
    let read_started = Instant::now();
    let read_result = read_bounded_input(&input_arg, &mut source);
    report.timings.read = Some(read_started.elapsed());
    report.source_bytes = Some(source.len());
    report.source_bytes_exact = Some(read_result.is_ok() && source.len() <= MAX_SOURCE_BYTES);
    match read_result {
        Ok(Some(metadata_length)) => {
            report.source_bytes = usize::try_from(metadata_length).ok();
            report.source_bytes_exact = Some(report.source_bytes.is_some());
            return report.fail(
                "INPUT_ERROR",
                "admission",
                "input exceeds the 64 MiB metadata/admission cap",
                1,
            );
        }
        Err(error) => return report.fail("INPUT_ERROR", "read", error.to_string(), 1),
        Ok(None) => {}
    }
    if source.len() > MAX_SOURCE_BYTES {
        return report.fail(
            "INPUT_ERROR",
            "admission",
            "input exceeds the 64 MiB metadata/admission cap",
            1,
        );
    }

    let mut adapter = PdfOxideAdapter::default();
    let parse_started = Instant::now();
    let parsed = adapter.parse_detached(&source);
    report.timings.parse_detached = Some(parse_started.elapsed());
    drop(source);
    let artifact = match parsed {
        Ok(artifact) => artifact,
        Err(error) => return report.fail("PARSE_ERROR", "parse_detached", error.to_string(), 1),
    };

    let adoption_started = Instant::now();
    let document = adapter.adopt_detached(artifact);
    report.timings.adoption = Some(adoption_started.elapsed());
    let document = match document {
        Ok(document) => document,
        Err(error) => return report.fail("INPUT_ERROR", "adoption", error.to_string(), 1),
    };

    let request = match PdfRenderRequest::new(document.document_id, PageId::new(page), scale, 1) {
        Ok(request) => request,
        Err(error) => {
            return report.fail("INPUT_ERROR", "request_validation", error.to_string(), 1);
        }
    };

    let geometry_started = Instant::now();
    let geometry = adapter.page_geometry(document.document_id, PageId::new(page));
    report.timings.page_geometry = Some(geometry_started.elapsed());
    let geometry = match geometry {
        Ok(geometry) => geometry,
        Err(error) => return report.fail("INPUT_ERROR", "page_geometry", error.to_string(), 1),
    };
    report.geometry = Some(geometry);

    let prepare_started = Instant::now();
    let job = adapter.prepare_render(request);
    report.timings.prepare_render = Some(prepare_started.elapsed());
    let job = match job {
        Ok(job) => job,
        Err(error) => return report.fail("INPUT_ERROR", "prepare_render", error.to_string(), 1),
    };

    let render_started = Instant::now();
    let rendered = job.render();
    report.timings.job_render = Some(render_started.elapsed());
    let png = match rendered {
        Ok(png) => png,
        Err(error) => return report.fail("CONTROLLED_FAILURE", "job.render", error.to_string(), 2),
    };
    report.png_bytes = Some(png.len());

    let dimensions = match guarded_png_dimensions(&png) {
        Ok(dimensions) => dimensions,
        Err(error) => return report.fail("CONTROLLED_FAILURE", "png_admission", error, 2),
    };
    let (header_width, header_height, pixels) = dimensions;
    report.png_width = Some(header_width);
    report.png_height = Some(header_height);
    report.pixel_count = Some(pixels);

    let decode_started = Instant::now();
    let decoded = image::load_from_memory_with_format(&png, image::ImageFormat::Png);
    report.timings.png_decode = Some(decode_started.elapsed());
    let decoded = match decoded {
        Ok(decoded) => decoded,
        Err(error) => return report.fail("CONTROLLED_FAILURE", "png_decode", error.to_string(), 2),
    };
    if decoded.width() != header_width || decoded.height() != header_height {
        return report.fail(
            "CONTROLLED_FAILURE",
            "png_decode",
            "decoded PNG dimensions do not match the PNG header",
            2,
        );
    }

    let analysis_started = Instant::now();
    let mut nonwhite_pixels = 0u64;
    for (_, _, pixel) in decoded.pixels() {
        let rgba = pixel.0;
        if rgba[3] != 0 && (rgba[0] < 255 || rgba[1] < 255 || rgba[2] < 255) {
            nonwhite_pixels += 1;
        }
    }
    report.timings.nonwhite_analysis = Some(analysis_started.elapsed());
    report.nonwhite_pixels = Some(nonwhite_pixels);

    report.status = "RENDERED";
    report.stage = "complete";
    (report, 0)
}

fn read_bounded_input(input: &OsStr, output: &mut Vec<u8>) -> io::Result<Option<u64>> {
    let maximum_read = (MAX_SOURCE_BYTES as u64) + 1;
    if input == OsStr::new("-") {
        let stdin = io::stdin();
        stdin.lock().take(maximum_read).read_to_end(output)?;
    } else {
        let file = File::open(Path::new(input))?;
        let metadata_length = file.metadata()?.len();
        if metadata_length > MAX_SOURCE_BYTES as u64 {
            return Ok(Some(metadata_length));
        }
        file.take(maximum_read).read_to_end(output)?;
    }
    Ok(None)
}

fn guarded_png_dimensions(png: &[u8]) -> Result<(u32, u32, u64), String> {
    if png.len() > MAX_RENDER_PNG_BYTES {
        return Err("rendered PNG exceeds the 64 MiB byte guard".to_owned());
    }
    if png.len() < 24 || &png[..8] != PNG_SIGNATURE || &png[12..16] != b"IHDR" {
        return Err("rendered data is not a bounded PNG with an IHDR header".to_owned());
    }
    let width = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
    let height = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
    if width == 0 || height == 0 {
        return Err("rendered PNG dimensions must be nonzero".to_owned());
    }
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or_else(|| "rendered PNG pixel count overflows".to_owned())?;
    let raster_bytes = pixels
        .checked_mul(4)
        .ok_or_else(|| "rendered PNG raster byte count overflows".to_owned())?;
    if pixels > MAX_RENDER_PIXELS || raster_bytes > MAX_RENDER_RASTER_BYTES {
        return Err("rendered PNG exceeds the production pixel/raster guards".to_owned());
    }
    Ok((width, height, pixels))
}

fn bounded_json_record(report: &Report) -> String {
    let limits = [
        (MAX_INPUT_LABEL_CHARS, MAX_ERROR_CAUSE_CHARS),
        (192, 384),
        (128, 256),
        (64, 128),
        (0, 0),
    ];
    for (input_limit, error_limit) in limits {
        let record = serialize_report(report, input_limit, error_limit);
        if record.len() <= MAX_JSON_RECORD_BYTES {
            return record;
        }
    }
    // Fixed report fields are small, but keep a final valid record if a future
    // field unexpectedly makes the regular representation exceed its budget.
    let mut record = String::from("{\"status\":");
    push_json_string(&mut record, report.status, 32);
    record.push_str(",\"stage\":");
    push_json_string(&mut record, report.stage, 32);
    record.push_str(",\"error\":\"profile record exceeded its output bound\"}");
    record
}

fn serialize_report(report: &Report, input_limit: usize, error_limit: usize) -> String {
    let mut json = String::with_capacity(2300);
    json.push_str("{\"status\":");
    push_json_string(&mut json, report.status, 32);
    json.push_str(",\"stage\":");
    push_json_string(&mut json, report.stage, 32);
    json.push_str(",\"input\":");
    let input_truncated = push_json_string(&mut json, &report.input_label, input_limit);
    json.push_str(",\"input_label_truncated\":");
    push_bool(&mut json, input_truncated);
    json.push_str(",\"source_bytes\":");
    push_optional_usize(&mut json, report.source_bytes);
    json.push_str(",\"source_bytes_exact\":");
    push_optional_bool(&mut json, report.source_bytes_exact);
    json.push_str(",\"page\":");
    push_optional_u64(&mut json, report.page);
    json.push_str(",\"scale\":");
    push_optional_f32(&mut json, report.scale);
    json.push_str(",\"geometry\":");
    match report.geometry {
        Some(geometry) => push_geometry(&mut json, geometry),
        None => json.push_str("null"),
    }
    json.push_str(",\"png_bytes\":");
    push_optional_usize(&mut json, report.png_bytes);
    json.push_str(",\"png_width\":");
    push_optional_u32(&mut json, report.png_width);
    json.push_str(",\"png_height\":");
    push_optional_u32(&mut json, report.png_height);
    json.push_str(",\"pixel_count\":");
    push_optional_u64(&mut json, report.pixel_count);
    json.push_str(",\"nonwhite_pixels\":");
    push_optional_u64(&mut json, report.nonwhite_pixels);
    json.push_str(",\"nonwhite_rule\":\"nontransparent pixels with any RGB channel below 255; coarse content indicator only\"");
    json.push_str(",\"timings_ms\":{");
    push_timing(&mut json, "read", report.timings.read, false);
    push_timing(
        &mut json,
        "parse_detached",
        report.timings.parse_detached,
        true,
    );
    push_timing(&mut json, "adoption", report.timings.adoption, true);
    push_timing(
        &mut json,
        "page_geometry",
        report.timings.page_geometry,
        true,
    );
    push_timing(
        &mut json,
        "prepare_render",
        report.timings.prepare_render,
        true,
    );
    push_timing(&mut json, "job_render", report.timings.job_render, true);
    push_timing(&mut json, "png_decode", report.timings.png_decode, true);
    push_timing(
        &mut json,
        "nonwhite_analysis",
        report.timings.nonwhite_analysis,
        true,
    );
    json.push('}');
    json.push_str(",\"timing_basis\":\"Instant wall-clock elapsed; not CPU time or RSS\"");
    json.push_str(",\"execution_context\":\"standalone synchronous CPU process; not app UI or PdfRenderWorker\"");
    json.push_str(",\"job_render_scope\":\"opaque; includes guarded content decode/reject, CPU raster, crop, and PNG encoding; not a parser/raster hotspot or proof of nontruncated pixel fidelity\"");
    json.push_str(",\"page_30_interpretation\":\"job_render_ms may measure the guarded 8 MiB decoded-content reject path; it is not a parser/raster hotspot measurement or evidence of nontruncated fidelity\"");
    json.push_str(",\"error\":");
    match report.error.as_deref() {
        Some(error) => {
            let truncated = push_json_string(&mut json, error, error_limit);
            json.push_str(",\"error_cause_truncated\":");
            push_bool(&mut json, truncated);
        }
        None => json.push_str("null,\"error_cause_truncated\":false"),
    }
    json.push('}');
    json
}

fn push_geometry(json: &mut String, geometry: PageGeometry) {
    json.push_str("{\"media_box\":[");
    push_f32(json, geometry.media_box[0]);
    json.push(',');
    push_f32(json, geometry.media_box[1]);
    json.push(',');
    push_f32(json, geometry.media_box[2]);
    json.push(',');
    push_f32(json, geometry.media_box[3]);
    json.push_str("],\"crop_box\":");
    match geometry.crop_box {
        Some(crop) => {
            json.push('[');
            push_f32(json, crop[0]);
            json.push(',');
            push_f32(json, crop[1]);
            json.push(',');
            push_f32(json, crop[2]);
            json.push(',');
            push_f32(json, crop[3]);
            json.push(']');
        }
        None => json.push_str("null"),
    }
    json.push_str(",\"width_points\":");
    push_f32(json, geometry.width_points);
    json.push_str(",\"height_points\":");
    push_f32(json, geometry.height_points);
    json.push_str(",\"rotation_degrees\":");
    let _ = write!(json, "{}", geometry.rotation_degrees);
    json.push('}');
}

fn push_timing(json: &mut String, name: &str, duration: Option<Duration>, comma: bool) {
    if comma {
        json.push(',');
    }
    push_json_string(json, name, 32);
    json.push(':');
    match duration {
        Some(duration) => {
            let milliseconds = duration.as_secs_f64() * 1000.0;
            let _ = write!(json, "{milliseconds:.3}");
        }
        None => json.push_str("null"),
    }
}

fn push_optional_usize(json: &mut String, value: Option<usize>) {
    match value {
        Some(value) => {
            let _ = write!(json, "{value}");
        }
        None => json.push_str("null"),
    }
}

fn push_optional_u32(json: &mut String, value: Option<u32>) {
    match value {
        Some(value) => {
            let _ = write!(json, "{value}");
        }
        None => json.push_str("null"),
    }
}

fn push_optional_u64(json: &mut String, value: Option<u64>) {
    match value {
        Some(value) => {
            let _ = write!(json, "{value}");
        }
        None => json.push_str("null"),
    }
}

fn push_optional_f32(json: &mut String, value: Option<f32>) {
    match value {
        Some(value) if value.is_finite() => push_f32(json, value),
        _ => json.push_str("null"),
    }
}

fn push_f32(json: &mut String, value: f32) {
    if value.is_finite() {
        let _ = write!(json, "{value}");
    } else {
        json.push_str("null");
    }
}

fn push_optional_bool(json: &mut String, value: Option<bool>) {
    match value {
        Some(value) => push_bool(json, value),
        None => json.push_str("null"),
    }
}

fn push_bool(json: &mut String, value: bool) {
    json.push_str(if value { "true" } else { "false" });
}

fn push_json_string(json: &mut String, value: &str, maximum_chars: usize) -> bool {
    json.push('"');
    let mut chars = value.chars();
    let mut written = 0usize;
    while written < maximum_chars {
        let Some(character) = chars.next() else {
            break;
        };
        match character {
            '"' => json.push_str("\\\""),
            '\\' => json.push_str("\\\\"),
            '\u{08}' => json.push_str("\\b"),
            '\u{0c}' => json.push_str("\\f"),
            '\n' => json.push_str("\\n"),
            '\r' => json.push_str("\\r"),
            '\t' => json.push_str("\\t"),
            character if character <= '\u{1f}' => {
                let _ = write!(json, "\\u{:04x}", character as u32);
            }
            character => json.push(character),
        }
        written += 1;
    }
    let truncated = chars.next().is_some();
    json.push('"');
    truncated
}
