#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const PROJECT_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const TOOLKIT_ROOT = path.resolve(PROJECT_ROOT, "..", "Rust_Makepad_Toolkit");
const OPERATIONS_PATH = path.join(
  TOOLKIT_ROOT,
  "prototypes",
  "ai-ui-test-tool-auto-probe",
  "operations.json",
);
const OUTPUT_DIR = path.join(PROJECT_ROOT, "verification", "pdf-reliability", "scenarios");

const INPUTS = Object.freeze({
  pdf16: "C:\\Users\\PUBLIC_USER\\Desktop\\private-acceptance-pdf.pdf",
  pdf60: "C:\\Users\\PUBLIC_USER\\Desktop\\private-large-acceptance-pdf.pdf",
  helloPdf: path.join(PROJECT_ROOT, "vendor", "pdf-oxide", "tests", "fixtures", "hello_structure.pdf"),
  scannedPdf: path.join(
    PROJECT_ROOT,
    "vendor",
    "pdf-oxide",
    "tests",
    "fixtures",
    "ocr",
    "auto_image_text_ru.pdf",
  ),
  docx: path.join(PROJECT_ROOT, "fixtures", "release-acceptance", "docx", "writer-layout-a4-multipage.docx"),
});

const ID = (suffix) => "rudra-office/" + suffix;
const IDS = Object.freeze({
  fileMenu: ID("menu/file-trigger"),
  openAction: ID("action/open"),
  pdfPage: ID("pdf/page-input"),
  pdfGo: ID("pdf/go-to-page"),
  pdfFirst: ID("pdf/first"),
  pdfLast: ID("pdf/last"),
  pdfNext: ID("pdf/next"),
  pdfPrevious: ID("pdf/previous"),
  pdfZoomIn: ID("pdf/zoom-in"),
  pdfZoomOut: ID("pdf/zoom-out"),
  pdfZoomReset: ID("pdf/reset-zoom"),
  pdfFitPage: ID("pdf/fit-page"),
  pdfFitWidth: ID("pdf/fit-width"),
  pdfCanvas: ID("pdf/canvas"),
  homeNewXlsx: ID("home/new-xlsx"),
  homeNewPptx: ID("home/new-pptx"),
});

const ALLOWED_OPERATIONS = new Set([
  "session.open",
  "session.close",
  "inspect_ui",
  "windows_set_window_bounds",
  "wait_idle",
  "wait_for_automation_id",
  "wait_for_value",
  "wait_enabled",
  "set_value",
  "click",
  "wait_selected",
  "select",
  "wait_hidden",
  "assert_valid_bounds",
  "screenshot_element",
  "screenshot_window",
  "get_runtime_health",
  "get_runtime_stderr",
  "windows_export_evidence",
]);

function fail(message) {
  throw new Error(message);
}

function parseArguments(argv) {
  const result = {};
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === "--help" || argument === "-h") {
      return { help: true };
    }
    if (argument !== "--exe" && argument !== "--attempt") {
      fail("Unknown argument: " + argument);
    }
    const key = argument.slice(2);
    if (Object.hasOwn(result, key)) {
      fail("Argument supplied more than once: " + argument);
    }
    const value = argv[index + 1];
    if (!value || value.startsWith("--")) {
      fail("Missing value for " + argument);
    }
    result[key] = value;
    index += 1;
  }
  if (!result.exe || !result.attempt) {
    fail("Both --exe and --attempt are required. Use --help for usage.");
  }
  if (!/^[A-Za-z0-9][A-Za-z0-9_-]{0,47}$/.test(result.attempt)) {
    fail("--attempt must be a 1-48 character filename-safe identifier.");
  }
  const exe = path.isAbsolute(result.exe)
    ? path.normalize(result.exe)
    : path.resolve(PROJECT_ROOT, result.exe);
  if (path.extname(exe).toLowerCase() !== ".exe") {
    fail("--exe must name an .exe path; the generator never launches it.");
  }
  return { attempt: result.attempt, exe };
}

function printUsage() {
  process.stdout.write(
    "Usage: node tools/generate-pdf-reliability-scenarios.mjs --exe <candidate.exe> --attempt <unique-id>\n" +
      "Writes five public AI UI Test Tool v1 scripts below verification/pdf-reliability/scenarios.\n" +
      "The candidate is stored in each script but is never started. Existing outputs are never overwritten.\n",
  );
}

function requireFile(filePath, label) {
  let stat;
  try {
    stat = fs.statSync(filePath);
  } catch {
    fail(label + " is missing: " + filePath);
  }
  if (!stat.isFile()) {
    fail(label + " is not a regular file: " + filePath);
  }
  return stat;
}

function checkInputs() {
  requireFile(INPUTS.pdf16, "16-page Desktop PDF");
  const largePdf = requireFile(INPUTS.pdf60, "60-page Desktop PDF");
  if (largePdf.size !== 31_662_062) {
    fail("60-page Desktop PDF size changed: expected 31662062 bytes, found " + largePdf.size);
  }
  requireFile(INPUTS.helloPdf, "two-page hello_structure.pdf fixture");
  requireFile(INPUTS.scannedPdf, "scanned OCR fixture PDF");

  const docxExists = fs.existsSync(INPUTS.docx);
  if (docxExists && !fs.statSync(INPUTS.docx).isFile()) {
    fail("Optional DOCX fixture path exists but is not a regular file: " + INPUTS.docx);
  }
  return { docxIncluded: docxExists };
}

function ui(operation, args = {}) {
  return Object.assign({ op: operation, session: "app" }, args);
}

function click(id, stepId) {
  return ui("click", { id, step_id: stepId });
}

function waitForId(id, stepId, timeoutMs = 90_000) {
  return ui("wait_for_automation_id", { id, step_id: stepId, timeout_ms: timeoutMs });
}

function waitForPage(page, stepId, timeoutMs = 90_000) {
  return [
    ui("wait_for_value", {
      id: IDS.pdfPage,
      value: String(page),
      step_id: stepId + "-page-value",
      timeout_ms: timeoutMs,
    }),
    ui("wait_idle", { step_id: stepId + "-render-idle", timeout_ms: timeoutMs }),
  ];
}

function goToPage(page, stepId) {
  return [
    ui("set_value", {
      id: IDS.pdfPage,
      value: String(page),
      step_id: stepId + "-set-input",
    }),
    ui("wait_for_value", {
      id: IDS.pdfPage,
      value: String(page),
      step_id: stepId + "-input-value",
      timeout_ms: 15_000,
    }),
    click(IDS.pdfGo, stepId + "-invoke-go-to-page"),
    ...waitForPage(page, stepId + "-page-state"),
  ];
}

function clickNavigation(buttonId, page, stepId) {
  return [click(buttonId, stepId + "-invoke"), ...waitForPage(page, stepId)];
}

function zoomTo(percent, stepId) {
  if ((percent - 100) % 25 !== 0 || percent < 50 || percent > 200) {
    fail("Unsupported zoom target in generator: " + percent);
  }
  const direction = percent < 100 ? IDS.pdfZoomOut : IDS.pdfZoomIn;
  const clickCount = Math.abs((percent - 100) / 25);
  const steps = [click(IDS.pdfZoomReset, stepId + "-reset-to-100")];
  steps.push(ui("wait_idle", { step_id: stepId + "-reset-idle", timeout_ms: 90_000 }));
  for (let count = 1; count <= clickCount; count += 1) {
    steps.push(click(direction, stepId + "-step-" + count));
    steps.push(ui("wait_idle", { step_id: stepId + "-idle-" + count, timeout_ms: 90_000 }));
  }
  return steps;
}

function captureCanvas(label, stepId) {
  return ui("screenshot_element", {
    id: IDS.pdfCanvas,
    step_id: stepId,
    extract: { [label]: "$.path" },
  });
}

function openFileSteps(filePath, documentNumber, canvasId, prefix) {
  const tabId = ID("document:" + documentNumber + "/tab");
  return [
    click(IDS.fileMenu, prefix + "-open-menu"),
    ui("wait_idle", { step_id: prefix + "-menu-idle", timeout_ms: 10_000 }),
    ui("wait_enabled", { id: IDS.openAction, step_id: prefix + "-open-enabled", timeout_ms: 15_000 }),
    ui("set_value", { id: IDS.openAction, value: filePath, step_id: prefix + "-set-file-path" }),
    ui("wait_for_value", {
      id: IDS.openAction,
      value: filePath,
      step_id: prefix + "-file-path-set",
      timeout_ms: 15_000,
    }),
    ui("click", { id: IDS.openAction, step_id: prefix + "-invoke-open", timeout_ms: 120_000 }),
    waitForId(tabId, prefix + "-tab-visible", 120_000),
    ui("wait_selected", { id: tabId, step_id: prefix + "-tab-selected", timeout_ms: 120_000 }),
    waitForId(canvasId, prefix + "-canvas-visible", 120_000),
    ui("wait_idle", { step_id: prefix + "-document-idle", timeout_ms: 120_000 }),
  ];
}

function closeTab(documentNumber, prefix) {
  const tabId = ID("document:" + documentNumber + "/tab");
  return [
    click(tabId + "/close", prefix + "-close"),
    ui("wait_hidden", { id: tabId, step_id: prefix + "-tab-hidden", timeout_ms: 30_000 }),
    ui("wait_idle", { step_id: prefix + "-close-idle", timeout_ms: 30_000 }),
  ];
}

function createHomeDocument(actionId, documentNumber, canvasId, prefix) {
  const tabId = ID("document:" + documentNumber + "/tab");
  return [
    waitForId(actionId, prefix + "-home-action-visible", 30_000),
    ui("wait_enabled", { id: actionId, step_id: prefix + "-home-action-enabled", timeout_ms: 30_000 }),
    click(actionId, prefix + "-create-blank"),
    waitForId(tabId, prefix + "-tab-visible", 30_000),
    ui("wait_selected", { id: tabId, step_id: prefix + "-tab-selected", timeout_ms: 30_000 }),
    waitForId(canvasId, prefix + "-canvas-visible", 30_000),
    ui("assert_valid_bounds", { id: canvasId, step_id: prefix + "-positive-canvas-bounds" }),
    ui("wait_idle", { step_id: prefix + "-document-idle", timeout_ms: 30_000 }),
  ];
}

function beginScenario() {
  return [
    { op: "session.open", save_session_as: "app" },
    ui("inspect_ui", { save_as: "initial-ui" }),
    waitForId(IDS.fileMenu, "home-file-menu-visible", 30_000),
  ];
}

function finishScenario(steps) {
  steps.push(ui("get_runtime_health", { step_id: "runtime-health" }));
  steps.push(ui("get_runtime_stderr", { step_id: "runtime-stderr" }));
  steps.push(ui("windows_export_evidence", { step_id: "export-evidence" }));
  return steps;
}

function makeScenario(attempt, exe, slug, intent, steps) {
  return {
    version: 1,
    name: "rudra-pdf-" + attempt + "-" + slug,
    intent,
    application: { command: exe, window_title: "Rudra Office" },
    options: { stop_on_error: true, evidence_on_failure: true },
    tags: ["pdf-reliability", "diagnostic", "read-only-source-documents", "visual-review-required"],
    generated_by: "tools/generate-pdf-reliability-scenarios.mjs",
    // These are curated workflow templates, not Toolkit inventory generation.
    generated: false,
    generator_version: "1.0.0",
    steps: finishScenario(steps),
    teardown: [{ op: "session.close", session: "app" }],
  };
}

function buildScenarios(attempt, exe, docxIncluded) {
  const navigation16 = [
    ...beginScenario(),
    ui("windows_set_window_bounds", { width: 1500, height: 900, step_id: "window-1500x900" }),
    ui("wait_idle", { step_id: "window-1500x900-idle", timeout_ms: 30_000 }),
    ...openFileSteps(INPUTS.pdf16, 1, IDS.pdfCanvas, "open-16pp"),
    ...waitForPage(1, "initial-page-1"),
    ui("assert_valid_bounds", { id: IDS.pdfCanvas, step_id: "canvas-positive-bounds-1500x900" }),
    captureCanvas("page_1_initial", "capture-page-1-initial"),
    ...goToPage(8, "go-to-page-8"),
    captureCanvas("page_8", "capture-page-8"),
    ...clickNavigation(IDS.pdfNext, 9, "next-to-page-9"),
    ...clickNavigation(IDS.pdfPrevious, 8, "previous-to-page-8"),
    ...clickNavigation(IDS.pdfLast, 16, "last-to-page-16"),
    captureCanvas("page_16", "capture-page-16"),
    ...clickNavigation(IDS.pdfFirst, 1, "first-to-page-1"),
    captureCanvas("page_1_return", "capture-page-1-return"),
    ...zoomTo(50, "zoom-50"),
    captureCanvas("zoom_50", "capture-zoom-50"),
    ...zoomTo(75, "zoom-75"),
    ...zoomTo(100, "zoom-100"),
    ...zoomTo(150, "zoom-150"),
    ...zoomTo(200, "zoom-200"),
    captureCanvas("zoom_200", "capture-zoom-200"),
    click(IDS.pdfFitWidth, "fit-width-at-1500x900"),
    ui("wait_idle", { step_id: "fit-width-1500x900-idle", timeout_ms: 90_000 }),
    click(IDS.pdfFitPage, "fit-page-at-1500x900"),
    ui("wait_idle", { step_id: "fit-page-1500x900-idle", timeout_ms: 90_000 }),
    ui("windows_set_window_bounds", { width: 1000, height: 700, step_id: "window-1000x700" }),
    ui("wait_idle", { step_id: "window-1000x700-idle", timeout_ms: 30_000 }),
    ui("assert_valid_bounds", { id: IDS.pdfCanvas, step_id: "canvas-positive-bounds-1000x700" }),
    click(IDS.pdfFitWidth, "fit-width-at-1000x700"),
    ui("wait_idle", { step_id: "fit-width-1000x700-idle", timeout_ms: 90_000 }),
    click(IDS.pdfFitPage, "fit-page-at-1000x700"),
    ui("wait_idle", { step_id: "fit-page-1000x700-idle", timeout_ms: 90_000 }),
    ui("windows_set_window_bounds", { width: 760, height: 600, step_id: "window-760x600" }),
    ui("wait_idle", { step_id: "window-760x600-idle", timeout_ms: 30_000 }),
    ui("assert_valid_bounds", { id: IDS.pdfCanvas, step_id: "canvas-positive-bounds-760x600" }),
    click(IDS.pdfFitWidth, "fit-width-at-760x600"),
    ui("wait_idle", { step_id: "fit-width-760x600-idle", timeout_ms: 90_000 }),
    captureCanvas("fit_width_760x600", "capture-fit-width-760x600"),
    click(IDS.pdfFitPage, "fit-page-at-760x600"),
    ui("wait_idle", { step_id: "fit-page-760x600-idle", timeout_ms: 90_000 }),
    captureCanvas("fit_page_760x600", "capture-fit-page-760x600"),
    ...closeTab(1, "close-16pp"),
  ];

  const cache60 = [
    ...beginScenario(),
    ui("windows_set_window_bounds", { width: 1500, height: 900, step_id: "large-initial-window-1500x900" }),
    ...openFileSteps(INPUTS.pdf60, 1, IDS.pdfCanvas, "open-60pp"),
    ...waitForPage(1, "initial-page-1"),
    ui("assert_valid_bounds", { id: IDS.pdfCanvas, step_id: "canvas-positive-bounds" }),
    captureCanvas("page_1_initial", "capture-page-1-initial"),
    ...goToPage(30, "go-to-page-30"),
    // Page 30 intentionally returns a bounded complexity error in this candidate.
    // Capture the whole window: the error label lives outside the page canvas.
    ui("screenshot_window", { step_id: "capture-page-30-controlled-error", extract: { page_30_error_window: "$.path" } }),
    captureCanvas("page_30", "capture-page-30"),
    ...clickNavigation(IDS.pdfLast, 60, "last-to-page-60"),
    captureCanvas("page_60", "capture-page-60"),
    ...clickNavigation(IDS.pdfFirst, 1, "first-return-to-page-1"),
    captureCanvas("page_1_cache_return", "capture-page-1-cache-return"),
    ...zoomTo(50, "large-zoom-50"),
    captureCanvas("large_zoom_50", "capture-large-zoom-50"),
    ...zoomTo(75, "large-zoom-75"),
    captureCanvas("large_zoom_75", "capture-large-zoom-75"),
    ...zoomTo(100, "large-zoom-100"),
    captureCanvas("large_zoom_100", "capture-large-zoom-100"),
    ...zoomTo(150, "large-zoom-150"),
    captureCanvas("large_zoom_150", "capture-large-zoom-150"),
    ...zoomTo(200, "large-zoom-200"),
    captureCanvas("large_zoom_200", "capture-large-zoom-200"),
    ...[ [1500, 900], [1000, 700], [760, 600] ].flatMap(([width, height]) => [
      ui("windows_set_window_bounds", { width, height, step_id: `large-window-${width}x${height}` }),
      ui("wait_idle", { step_id: `large-resize-${width}x${height}-idle`, timeout_ms: 30_000 }),
      ui("assert_valid_bounds", { id: IDS.pdfCanvas, step_id: `large-canvas-bounds-${width}x${height}` }),
      click(IDS.pdfFitWidth, `large-fit-width-${width}x${height}`),
      ui("wait_idle", { step_id: `large-fit-width-${width}x${height}-idle`, timeout_ms: 90_000 }),
      captureCanvas(`large_fit_width_${width}x${height}`, `capture-large-fit-width-${width}x${height}`),
      click(IDS.pdfFitPage, `large-fit-page-${width}x${height}`),
      ui("wait_idle", { step_id: `large-fit-page-${width}x${height}-idle`, timeout_ms: 90_000 }),
      captureCanvas(`large_fit_page_${width}x${height}`, `capture-large-fit-page-${width}x${height}`),
    ]),
    ...closeTab(1, "close-60pp"),
  ];

  const multiPdf = [
    ...beginScenario(),
    ...openFileSteps(INPUTS.pdf16, 1, IDS.pdfCanvas, "open-first-pdf"),
    ...waitForPage(1, "first-pdf-page-1"),
    ui("assert_valid_bounds", { id: IDS.pdfCanvas, step_id: "first-pdf-positive-bounds" }),
    captureCanvas("first_pdf_page_1_default", "capture-first-pdf-page-1-default"),
    ...zoomTo(150, "first-pdf-zoom-150"),
    captureCanvas("first_pdf_page_1_zoom_150", "capture-first-pdf-page-1-zoom-150"),
    // Page 8's heavy raster is covered by navigation16, not replayed here.
    ...goToPage(16, "first-pdf-go-to-page-16"),
    ...openFileSteps(INPUTS.helloPdf, 2, IDS.pdfCanvas, "open-second-hello-pdf"),
    ...waitForPage(1, "second-pdf-page-1"),
    ui("assert_valid_bounds", { id: IDS.pdfCanvas, step_id: "second-pdf-positive-bounds" }),
    captureCanvas("second_pdf_page_1_content_review", "capture-second-pdf-page-1-content-review"),
    ...zoomTo(50, "second-pdf-zoom-50"),
    captureCanvas("second_pdf_page_1_zoom_50", "capture-second-pdf-page-1-zoom-50"),
    ...goToPage(2, "second-pdf-go-to-page-2"),
    captureCanvas("second_pdf_page_2", "capture-second-pdf-page-2"),
    ui("select", { id: ID("document:1/tab"), step_id: "switch-to-first-pdf" }),
    ui("wait_selected", { id: ID("document:1/tab"), step_id: "first-pdf-selected", timeout_ms: 30_000 }),
    ...waitForPage(16, "first-pdf-position-after-switch"),
    captureCanvas("first_pdf_page_16_after_switch", "capture-first-pdf-after-switch"),
    ui("select", { id: ID("document:2/tab"), step_id: "switch-to-second-pdf" }),
    ui("wait_selected", { id: ID("document:2/tab"), step_id: "second-pdf-selected", timeout_ms: 30_000 }),
    ...waitForPage(2, "second-pdf-position-after-switch"),
    captureCanvas("second_pdf_page_2_after_switch", "capture-second-pdf-after-switch"),
    ...closeTab(2, "close-second-pdf"),
    ui("wait_selected", { id: ID("document:1/tab"), step_id: "first-pdf-selected-after-close", timeout_ms: 30_000 }),
    ...waitForPage(16, "first-pdf-position-after-close"),
    ...closeTab(1, "close-first-pdf"),
  ];

  const mixedFormat = [
    ...beginScenario(),
    ...openFileSteps(INPUTS.scannedPdf, 1, IDS.pdfCanvas, "open-scanned-vendor-pdf"),
    ...waitForPage(1, "scanned-pdf-page-1"),
    ui("assert_valid_bounds", { id: IDS.pdfCanvas, step_id: "scanned-pdf-positive-bounds" }),
    captureCanvas("scanned_pdf_page_1", "capture-scanned-pdf-page-1"),
    ...closeTab(1, "close-scanned-pdf"),
    ...createHomeDocument(IDS.homeNewXlsx, 2, ID("document:2/sheets/grid"), "home-new-xlsx"),
    ...closeTab(2, "close-blank-xlsx"),
    ...createHomeDocument(IDS.homeNewPptx, 3, ID("slides/canvas"), "home-new-pptx"),
    ...closeTab(3, "close-blank-pptx"),
  ];

  let mixedIntent =
    "One process: open and inspect the one-page scanned vendor PDF, close it, create a blank XLSX from Home with no open tabs, close it, then create and close a blank PPTX from Home with no open tabs. No document is saved and no OCR/content assertion is made.";
  if (docxIncluded) {
    mixedFormat.push(
      ...openFileSteps(INPUTS.docx, 4, ID("writer/canvas"), "open-existing-docx-fixture"),
      ui("assert_valid_bounds", {
        id: ID("writer/canvas"),
        step_id: "docx-positive-bounds",
      }),
      ...closeTab(4, "close-existing-docx-fixture"),
    );
    mixedIntent += " The existing repository DOCX fixture is included because it is present at generation time.";
  } else {
    mixedIntent += " The optional DOCX lifecycle is omitted because its repository fixture is absent.";
  }

  const measuredFit = [
    ...beginScenario(),
    ...openFileSteps(INPUTS.pdf60, 1, IDS.pdfCanvas, "open-fit-real-pdf"),
    ...[ [1500, 900], [1000, 700], [760, 600] ].flatMap(([width, height]) => [
      ui("windows_set_window_bounds", { width, height, step_id: `fit-window-${width}x${height}` }),
      ui("wait_idle", { step_id: `fit-resize-${width}x${height}-idle`, timeout_ms: 30_000 }),
      ui("assert_valid_bounds", { id: IDS.pdfCanvas, step_id: `fit-canvas-bounds-${width}x${height}` }),
      click(IDS.pdfFitWidth, `fit-width-${width}x${height}`),
      ui("wait_idle", { step_id: `fit-width-${width}x${height}-idle`, timeout_ms: 30_000 }),
      captureCanvas(`fit_width_${width}x${height}`, `capture-fit-width-${width}x${height}`),
      click(IDS.pdfFitPage, `fit-page-${width}x${height}`),
      ui("wait_idle", { step_id: `fit-page-${width}x${height}-idle`, timeout_ms: 30_000 }),
      captureCanvas(`fit_page_${width}x${height}`, `capture-fit-page-${width}x${height}`),
    ]),
    ...closeTab(1, "close-fit-real-pdf"),
  ];

  return [
    {
      file: attempt + "-pdf-16pp-navigation.aiui.json",
      scenario: makeScenario(
        attempt,
        exe,
        "16pp-navigation",
        "One process on the real 16-page Desktop PDF: verify page state 1→8→9→8→16→1, then issue 50/75/100/150/200% zoom sequences, Fit Width and Fit Page at 1500×900, 1000×700 and 760×600. Capture key pages and representative scale/fit states for human review. The public semantic tree exposes page number and positive canvas bounds, but not a PDF zoom-value control.",
        navigation16,
      ),
    },
    {
      file: attempt + "-pdf-60pp-cache-return.aiui.json",
      scenario: makeScenario(
        attempt,
        exe,
        "60pp-cache-return",
        "One process on the real 60-page, 31,662,062-byte Desktop PDF: page 30 must show a precise complexity error, NOT a rendered-page PASS. Capture the entire error window, recover via page 60 and page 1, then check 50/75/100/150/200% zoom and Fit Width/Page at three sizes. Semantic page values and positive canvas bounds are checked; visual content and the error text require review. Returning to page 1 does not prove cache eviction. No content OCR assertion is made.",
        cache60,
      ),
    },
    {
      file: attempt + "-pdf-two-document-isolation.aiui.json",
      scenario: makeScenario(
        attempt,
        exe,
        "two-document-isolation",
        "One process: open the real 16-page PDF, then the two-page hello_structure.pdf; capture both page-1 canvases, use separate zoom commands (150% and 50%) and positions (page 16 and page 2), switch between tabs, verify the active semantic page value, capture again, then close both tabs. Heavy page 8 remains separately required in the navigation workflow and is not reclassified as PASS here. A human must inspect the second PDF capture and confirm its Hello World page; exact zoom is not exposed semantically.",
        multiPdf,
      ),
    },
    {
      file: attempt + "-pdf-cross-format-lifecycle.aiui.json",
      scenario: makeScenario(attempt, exe, "cross-format-lifecycle", mixedIntent, mixedFormat),
    },
    {
      file: attempt + "-pdf-measured-fit.aiui.json",
      scenario: makeScenario(attempt, exe, "measured-fit", "Focused follow-up after an observed Fit viewport defect: real PDF page 1 must fit the actual measured canvas at 1500x900, 1000x700 and 760x600. Positive bounds and dispatch are checked; inspect all six screenshots for physical fit. No replay of navigation, other formats or historical regressions.", measuredFit),
    },
  ];
}

function readPublicCatalog() {
  let parsed;
  try {
    parsed = JSON.parse(fs.readFileSync(OPERATIONS_PATH, "utf8"));
  } catch (error) {
    fail("Cannot read the current Toolkit public operations catalog at " + OPERATIONS_PATH + ": " + error.message);
  }
  if (parsed.version !== 1 || !Array.isArray(parsed.operations)) {
    fail("Expected AI UI Test Tool public operations catalog version 1.");
  }
  return new Map(parsed.operations.map((operation) => [operation.name, operation]));
}

function validateAgainstPublicCatalog(entries, catalog) {
  for (const { file, scenario } of entries) {
    if (!Array.isArray(scenario.steps) || scenario.steps.length === 0) {
      fail(file + " has no steps.");
    }
    if (scenario.steps[0].op !== "session.open") {
      fail(file + " must begin with session.open.");
    }
    if (!scenario.teardown.some((step) => step.op === "session.close" && step.session === "app")) {
      fail(file + " must close its app session in teardown.");
    }
    for (const section of ["setup", "steps", "teardown"]) {
      for (const step of scenario[section] || []) {
        if (!step.op) {
          fail(file + " contains a step without an operation in " + section + ".");
        }
        if (!ALLOWED_OPERATIONS.has(step.op)) {
          fail(file + " uses an operation outside this bounded generator allowlist: " + step.op);
        }
        const definition = catalog.get(step.op);
        if (!definition) {
          fail(file + " uses an operation not exposed by the current public catalog: " + step.op);
        }
        for (const required of definition.required || []) {
          const sessionAlias = required === "session_id" && typeof step.session === "string";
          if (!sessionAlias && !Object.hasOwn(step, required)) {
            fail(file + " operation " + step.op + " is missing required argument " + required + ".");
          }
        }
        if (step.op !== "session.open" && step.op !== "session.close" && step.session !== "app") {
          fail(file + " operation " + step.op + " must target the single app session.");
        }
      }
    }
  }
}

function writeScenarioFiles(entries) {
  const relative = path.relative(PROJECT_ROOT, OUTPUT_DIR);
  if (relative.startsWith("..") || path.isAbsolute(relative)) {
    fail("Scenario output directory escaped the RudraOffice verification scope.");
  }
  fs.mkdirSync(OUTPUT_DIR, { recursive: true });
  const targets = entries.map(({ file }) => path.join(OUTPUT_DIR, file));
  const existing = targets.filter((target) => fs.existsSync(target));
  if (existing.length > 0) {
    fail("Refusing to overwrite existing scenario output(s):\n" + existing.join("\n") + "\nChoose a new --attempt.");
  }
  for (let index = 0; index < entries.length; index += 1) {
    fs.writeFileSync(targets[index], JSON.stringify(entries[index].scenario, null, 2) + "\n", {
      encoding: "utf8",
      flag: "wx",
    });
  }
  return targets;
}

function main() {
  const args = parseArguments(process.argv.slice(2));
  if (args.help) {
    printUsage();
    return;
  }
  const inputs = checkInputs();
  const catalog = readPublicCatalog();
  const scenarios = buildScenarios(args.attempt, args.exe, inputs.docxIncluded);
  validateAgainstPublicCatalog(scenarios, catalog);
  const written = writeScenarioFiles(scenarios);
  process.stdout.write(
    "Validated " + written.length + " scenarios against AI UI Test Tool public catalog v1.\n" +
      "Candidate path recorded only; no EXE, runner, GUI, document save, or clipboard operation was run.\n" +
      "Optional DOCX fixture included: " + (inputs.docxIncluded ? "yes" : "no") + ".\n" +
      written.map((file) => path.relative(PROJECT_ROOT, file)).join("\n") + "\n",
  );
}

try {
  main();
} catch (error) {
  process.stderr.write("PDF reliability scenario generation failed: " + error.message + "\n");
  process.exitCode = 1;
}
