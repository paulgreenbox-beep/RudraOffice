import fs from "node:fs/promises";
import path from "node:path";
import { Presentation, PresentationFile } from "@oai/artifact-tool";

const OUTPUT_NAME = "controlled-acceptance.pptx";
const SIZE = { width: 1280, height: 720 };

function options(argv) {
  const parsed = new Map();
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === "--force") parsed.set("force", true);
    if (argv[i] === "--root" || argv[i] === "--preview-dir") parsed.set(argv[i].slice(2), argv[++i]);
  }
  return parsed;
}

async function requireNew(output, force) {
  try {
    await fs.access(output);
    if (!force) throw new Error(`refusing to overwrite ${output}; pass --force to regenerate`);
  } catch (error) {
    if (error?.code !== "ENOENT") throw error;
  }
}

function textbox(slide, name, text, position, style) {
  const shape = slide.shapes.add({
    geometry: "textbox",
    name,
    position,
    fill: "none",
    line: { style: "solid", fill: "none", width: 0 },
  });
  shape.text = text;
  shape.text.style = style;
  return shape;
}

function title(slide, text, page) {
  textbox(slide, `slide-${page}-title`, text, { left: 72, top: 58, width: 930, height: 70 }, {
    fontSize: 38,
    bold: true,
    color: "slate-950",
  });
  textbox(slide, `slide-${page}-footer`, `Rudra Office • controlled acceptance • ${page} / 3`, {
    left: 72, top: 670, width: 760, height: 22,
  }, { fontSize: 12, color: "slate-500" });
}

async function writeBlob(file, blob) {
  await fs.writeFile(file, new Uint8Array(await blob.arrayBuffer()));
}

async function main() {
  const args = options(process.argv.slice(2));
  const root = args.get("root");
  if (!root) throw new Error("usage: node generate-release-pptx.mjs --root <RudraOffice> [--preview-dir <directory>] [--force]");
  const outputDirectory = path.resolve(root, "fixtures", "release-acceptance", "pptx");
  const output = path.join(outputDirectory, OUTPUT_NAME);
  await fs.mkdir(outputDirectory, { recursive: true });
  await requireNew(output, args.get("force") === true);

  const presentation = Presentation.create({ slideSize: SIZE });

  const overview = presentation.slides.add();
  overview.background.fill = "slate-50";
  title(overview, "Контролируемая приёмка PPTX", 1);
  textbox(overview, "overview-subtitle", "Кириллица, фигуры, таблица, диаграмма и несколько слайдов", {
    left: 72, top: 144, width: 860, height: 42,
  }, { fontSize: 22, color: "slate-600" });
  overview.shapes.add({
    geometry: "roundRect",
    name: "overview-accent-panel",
    position: { left: 72, top: 240, width: 470, height: 280 },
    fill: "#17365D",
    line: { style: "solid", fill: "#17365D", width: 1 },
    borderRadius: "rounded-2xl",
  });
  textbox(overview, "overview-accent-heading", "Viewer-only RC", {
    left: 112, top: 290, width: 360, height: 54,
  }, { fontSize: 30, bold: true, color: "white" });
  textbox(overview, "overview-accent-copy", "Этот файл не является презентацией продукта. Он служит стабильным входом для проверки просмотра PPTX.", {
    left: 112, top: 360, width: 350, height: 120,
  }, { fontSize: 18, color: "white" });
  for (const [index, label] of ["3 слайда", "Русский текст", "Визуальные объекты"].entries()) {
    overview.shapes.add({
      geometry: "roundRect",
      name: `overview-check-${index + 1}`,
      position: { left: 630, top: 238 + index * 88, width: 480, height: 60 },
      fill: "white",
      line: { style: "solid", fill: "slate-200", width: 1 },
      borderRadius: "rounded-xl",
      shadow: "shadow-sm",
    });
    textbox(overview, `overview-check-label-${index + 1}`, label, {
      left: 660, top: 254 + index * 88, width: 380, height: 28,
    }, { fontSize: 19, bold: true, color: "slate-800" });
  }

  const tableSlide = presentation.slides.add();
  tableSlide.background.fill = "white";
  title(tableSlide, "Таблица должна оставаться читаемой", 2);
  textbox(tableSlide, "table-subtitle", "Вторая страница намеренно содержит таблицу с русскими заголовками.", {
    left: 72, top: 130, width: 920, height: 30,
  }, { fontSize: 19, color: "slate-600" });
  const table = tableSlide.tables.add({
    rows: 5,
    columns: 4,
    left: 72,
    top: 200,
    width: 1136,
    height: 300,
    values: [
      ["Проверка", "Объект", "Ожидание", "Статус"],
      ["PPTX-01", "Текст", "Кириллица видима", "Готово"],
      ["PPTX-02", "Таблица", "Границы и строки", "Готово"],
      ["PPTX-03", "Диаграмма", "Данные и подписи", "Готово"],
      ["PPTX-04", "Навигация", "Три страницы", "Готово"],
    ],
  });
  table.styleOptions = { headerRow: true, bandedRows: true };
  table.borders.assign({ style: "solid", fill: "slate-200", width: 1 });
  table.getCell(0, 0).fill = "#17365D";
  table.getCell(0, 1).fill = "#17365D";
  table.getCell(0, 2).fill = "#17365D";
  table.getCell(0, 3).fill = "#17365D";

  const chartSlide = presentation.slides.add();
  chartSlide.background.fill = "slate-50";
  title(chartSlide, "Графика и структура слайда", 3);
  textbox(chartSlide, "chart-subtitle", "Диаграмма даёт отдельный сигнал о пути рендеринга объектов PPTX.", {
    left: 72, top: 130, width: 900, height: 30,
  }, { fontSize: 19, color: "slate-600" });
  chartSlide.shapes.add({
    geometry: "roundRect",
    name: "chart-frame",
    position: { left: 72, top: 205, width: 750, height: 385 },
    fill: "white",
    line: { style: "solid", fill: "slate-200", width: 1 },
    borderRadius: "rounded-2xl",
    shadow: "shadow-sm",
  });
  chartSlide.charts.add("bar", {
    position: { left: 120, top: 255, width: 650, height: 255 },
    categories: ["Текст", "Таблица", "Графика"],
    series: [{ name: "Покрытие", values: [3, 3, 3], fill: "accent1" }],
    hasLegend: false,
    dataLabels: { showValue: true, position: "outEnd" },
    yAxis: { majorGridlines: { style: "solid", fill: "slate-200", width: 1 } },
  });
  chartSlide.shapes.add({
    geometry: "roundRect",
    name: "chart-callout",
    position: { left: 890, top: 245, width: 290, height: 230 },
    fill: "#EAF2F8",
    line: { style: "solid", fill: "#9CBAD3", width: 1 },
    borderRadius: "rounded-xl",
  });
  textbox(chartSlide, "chart-callout-heading", "Контроль", {
    left: 925, top: 285, width: 210, height: 34,
  }, { fontSize: 24, bold: true, color: "#17365D" });
  textbox(chartSlide, "chart-callout-copy", "Переход между слайдами и отображение сложных объектов проверяются автоматизированно.", {
    left: 925, top: 335, width: 210, height: 100,
  }, { fontSize: 17, color: "slate-700" });

  const inspection = await presentation.inspect({ kind: "slide,textbox,shape,table,chart", maxChars: 8000 });
  if (!inspection.ndjson.includes("Контролируемая приёмка PPTX") || !inspection.ndjson.includes("Таблица")) {
    throw new Error("fixture inspection did not contain expected slide content");
  }
  if (args.get("preview-dir")) {
    const previewDirectory = path.resolve(args.get("preview-dir"));
    await fs.mkdir(previewDirectory, { recursive: true });
    for (const [index, slide] of presentation.slides.items.entries()) {
      await writeBlob(path.join(previewDirectory, `slide-${index + 1}.png`), await presentation.export({ slide, format: "png", scale: 1 }));
    }
  }
  const file = await PresentationFile.exportPptx(presentation);
  await file.save(output);
  console.log(output);
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
