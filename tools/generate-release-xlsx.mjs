import fs from "node:fs/promises";
import path from "node:path";
import { SpreadsheetFile, Workbook } from "@oai/artifact-tool";

const OUTPUT_NAME = "controlled-acceptance.xlsx";

function options(argv) {
  const parsed = new Map();
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === "--force") parsed.set("force", true);
    if (argv[i] === "--root") parsed.set("root", argv[++i]);
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

function title(range) {
  range.format.fill = "#17365D";
  range.format.font = { bold: true, color: "#FFFFFF", size: 14 };
  range.format.horizontalAlignment = "center";
  range.format.verticalAlignment = "center";
}

function header(range) {
  range.format.fill = "#D9EAF7";
  range.format.font = { bold: true, color: "#17365D" };
  range.format.horizontalAlignment = "center";
  range.format.verticalAlignment = "center";
  range.format.borders = { preset: "all", style: "thin", color: "#94A3B8" };
}

function grid(range) {
  range.format.borders = { preset: "all", style: "thin", color: "#CBD5E1" };
  range.format.verticalAlignment = "center";
}

async function main() {
  const args = options(process.argv.slice(2));
  const root = args.get("root");
  if (!root) throw new Error("usage: node generate-release-xlsx.mjs --root <RudraOffice> [--force]");

  const directory = path.resolve(root, "fixtures", "release-acceptance", "xlsx");
  const output = path.join(directory, OUTPUT_NAME);
  await fs.mkdir(directory, { recursive: true });
  await requireNew(output, args.get("force") === true);

  const workbook = Workbook.create();
  const summary = workbook.worksheets.add("Сводка");
  const data = workbook.worksheets.add("Данные");
  const sparse = workbook.worksheets.add("Разреженный");

  summary.showGridLines = false;
  summary.getRange("A1:F1").merge();
  summary.getRange("A1").values = [["Rudra Office — контролируемая приёмка XLSX"]];
  title(summary.getRange("A1:F1"));
  summary.getRange("A1:F1").format.rowHeight = 28;
  summary.getRange("A3:B7").values = [
    ["Параметр", "Значение"],
    ["Листов", 3],
    ["Строк с данными", 6],
    ["Итоговая сумма", null],
    ["Проверка формул", null],
  ];
  summary.getRange("B6").formulas = [["=SUM('Данные'!D2:D7)"]];
  summary.getRange("B7").formulas = [["=IF(B6=SUM('Данные'!D2:D7),\"OK\",\"ОШИБКА\")"]];
  header(summary.getRange("A3:B3"));
  grid(summary.getRange("A4:B7"));
  summary.getRange("B6").format.numberFormat = "#,##0.00 ₽";
  summary.getRange("A3:B7").format.columnWidth = 22;
  summary.getRange("A3:A7").format.wrapText = true;

  data.getRange("A1:D1").values = [["Код", "Наименование", "Количество", "Сумма, ₽"]];
  data.getRange("A2:C7").values = [
    ["A-001", "Кабель ВВГнг-LS 3×2,5", 120],
    ["A-002", "Светильник аварийный", 8],
    ["A-003", "Щит распределительный", 1],
    ["A-004", "Лоток перфорированный", 45],
    ["A-005", "Крепёж и аксессуары", 16],
    ["A-006", "Маркировка и документация", 1],
  ];
  data.getRange("D2").formulas = [["=C2*1250"]];
  data.getRange("D2:D7").fillDown();
  header(data.getRange("A1:D1"));
  grid(data.getRange("A2:D7"));
  data.getRange("D2:D7").format.numberFormat = "#,##0.00 ₽";
  data.getRange("A:D").format.columnWidth = 19;
  data.getRange("B:B").format.columnWidth = 34;
  data.getRange("A1:D7").format.wrapText = true;
  data.freezePanes.freezeRows(1);

  sparse.getRange("A1:B2").values = [["Проверка", "Разреженные данные"], ["Ранняя ячейка", "A2"]];
  sparse.getRange("Z100").values = [["Поздняя ячейка Z100"]];
  header(sparse.getRange("A1:B1"));
  grid(sparse.getRange("A2:B2"));
  sparse.getRange("A:B").format.columnWidth = 26;
  sparse.getRange("Z:Z").format.columnWidth = 24;

  const inspection = await workbook.inspect({ kind: "workbook,sheet,formula,region", maxChars: 6000, options: { maxResults: 80 } });
  if (!inspection.ndjson.includes("Данные") || !inspection.ndjson.includes("SUM")) {
    throw new Error("fixture inspection did not contain expected sheets and formulas");
  }
  const file = await SpreadsheetFile.exportXlsx(workbook);
  await file.save(output);
  console.log(output);
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
