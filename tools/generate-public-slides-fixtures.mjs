import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const WIDTH = 12_192_000;
const HEIGHT = 6_858_000;
const REL = {
  officeDocument: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument",
  slide: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide",
  slideMaster: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster",
  slideLayout: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout",
  image: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image",
};

function parseArgs(argv) {
  const args = { root: ROOT, check: false, force: false };
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === "--check") args.check = true;
    else if (argv[i] === "--force") args.force = true;
    else if (argv[i] === "--root" && argv[i + 1]) args.root = path.resolve(argv[++i]);
    else throw new Error("usage: node tools/generate-public-slides-fixtures.mjs [--root <RudraOffice>] [--check] [--force]");
  }
  if (args.check && args.force) throw new Error("--check and --force cannot be combined");
  return args;
}

function xmlEscape(value) {
  return String(value).replaceAll("&", "&amp;").replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;").replaceAll("\"", "&quot;").replaceAll("'", "&apos;");
}

const CRC_TABLE = (() => {
  const table = new Uint32Array(256);
  for (let n = 0; n < table.length; n += 1) {
    let crc = n;
    for (let bit = 0; bit < 8; bit += 1) crc = (crc & 1) ? (crc >>> 1) ^ 0xedb88320 : crc >>> 1;
    table[n] = crc >>> 0;
  }
  return table;
})();

function crc32(bytes) {
  let crc = 0xffffffff;
  for (const byte of bytes) crc = CRC_TABLE[(crc ^ byte) & 0xff] ^ (crc >>> 8);
  return (crc ^ 0xffffffff) >>> 0;
}

function adler32(bytes) {
  let a = 1;
  let b = 0;
  for (const byte of bytes) {
    a = (a + byte) % 65521;
    b = (b + a) % 65521;
  }
  return ((b << 16) | a) >>> 0;
}

function zlibStored(bytes) {
  const block = Buffer.alloc(5 + bytes.length);
  block[0] = 1;
  block.writeUInt16LE(bytes.length, 1);
  block.writeUInt16LE((~bytes.length) & 0xffff, 3);
  bytes.copy(block, 5);
  const adler = Buffer.alloc(4);
  adler.writeUInt32BE(adler32(bytes), 0);
  return Buffer.concat([Buffer.from([0x78, 0x01]), block, adler]);
}

function pngChunk(type, data) {
  const name = Buffer.from(type, "ascii");
  const chunk = Buffer.alloc(12 + data.length);
  chunk.writeUInt32BE(data.length, 0);
  name.copy(chunk, 4);
  data.copy(chunk, 8);
  chunk.writeUInt32BE(crc32(Buffer.concat([name, data])), 8 + data.length);
  return chunk;
}

function makeSyntheticPng() {
  const header = Buffer.alloc(13);
  header.writeUInt32BE(2, 0);
  header.writeUInt32BE(2, 4);
  header[8] = 8;
  header[9] = 6;
  const scanlines = Buffer.from([
    0, 230, 55, 45, 255, 45, 180, 95, 255,
    0, 55, 105, 220, 255, 240, 200, 40, 255,
  ]);
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    pngChunk("IHDR", header),
    pngChunk("IDAT", zlibStored(scanlines)),
    pngChunk("IEND", Buffer.alloc(0)),
  ]);
}

const PNG = makeSyntheticPng();

function position(x, y, cx, cy, prefix = "a") {
  return "<" + prefix + ":xfrm><a:off x=\"" + x + "\" y=\"" + y + "\"/><a:ext cx=\"" + cx + "\" cy=\"" + cy + "\"/></" + prefix + ":xfrm>";
}

function groupTree() {
  return "<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>" +
    "<p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/>" +
    "<a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>";
}

function textShape(id, name, text, x, y, cx, cy) {
  return "<p:sp><p:nvSpPr><p:cNvPr id=\"" + id + "\" name=\"" + xmlEscape(name) + "\"/>" +
    "<p:cNvSpPr txBox=\"1\"/><p:nvPr/></p:nvSpPr><p:spPr>" + position(x, y, cx, cy) +
    "<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr><p:txBody><a:bodyPr wrap=\"square\"/>" +
    "<a:lstStyle/><a:p><a:r><a:rPr lang=\"ru-RU\" sz=\"2000\"/><a:t>" + xmlEscape(text) +
    "</a:t></a:r><a:endParaRPr lang=\"ru-RU\"/></a:p></p:txBody></p:sp>";
}

function geometryShape() {
  return "<p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Synthetic blue rectangle\"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>" +
    "<p:spPr>" + position(914400, 1800000, 3048000, 1371600) +
    "<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val=\"2367A8\"/></a:solidFill></p:spPr></p:sp>";
}

function tableCell(text) {
  return "<a:tc><a:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang=\"ru-RU\" sz=\"1600\"/>" +
    "<a:t>" + xmlEscape(text) + "</a:t></a:r><a:endParaRPr lang=\"ru-RU\"/></a:p></a:txBody><a:tcPr/></a:tc>";
}

function tableFrame() {
  const rows = [["Проверка", "Ожидание"], ["PPTX-01", "Текст ячейки сохранён"]];
  return "<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id=\"2\" name=\"Synthetic 2x2 table\"/>" +
    "<p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x=\"914400\" y=\"1524000\"/>" +
    "<a:ext cx=\"8534400\" cy=\"1828800\"/></p:xfrm><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/table\">" +
    "<a:tbl><a:tblPr firstRow=\"1\"><a:tableStyleId>{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}</a:tableStyleId></a:tblPr>" +
    "<a:tblGrid><a:gridCol w=\"4267200\"/><a:gridCol w=\"4267200\"/></a:tblGrid>" +
    rows.map((row) => "<a:tr h=\"914400\">" + row.map(tableCell).join("") + "</a:tr>").join("") +
    "</a:tbl></a:graphicData></a:graphic></p:graphicFrame>";
}

function unknownGraphic() {
  return "<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id=\"2\" name=\"Synthetic unsupported graphic\"/>" +
    "<p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x=\"914400\" y=\"1524000\"/>" +
    "<a:ext cx=\"4572000\" cy=\"2286000\"/></p:xfrm><a:graphic><a:graphicData uri=\"urn:rudra-office:synthetic-unsupported\">" +
    "<synthetic:marker xmlns:synthetic=\"urn:rudra-office:synthetic-unsupported\"/></a:graphicData></a:graphic></p:graphicFrame>";
}

function pictureShape() {
  return "<p:pic><p:nvPicPr><p:cNvPr id=\"3\" name=\"Synthetic 2x2 PNG\"/><p:cNvPicPr><a:picLocks noChangeAspect=\"1\"/>" +
    "</p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed=\"rId2\"/><a:stretch><a:fillRect/></a:stretch></p:blipFill>" +
    "<p:spPr>" + position(914400, 1800000, 1219200, 1219200) + "<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr></p:pic>";
}

function slideXml(name, contents) {
  return "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>" +
    "<p:sld xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">" +
    "<p:cSld name=\"" + xmlEscape(name) + "\"><p:spTree>" + groupTree() + contents + "</p:spTree></p:cSld>" +
    "<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>";
}

function presentationXml(count) {
  let ids = "";
  for (let i = 0; i < count; i += 1) ids += "<p:sldId id=\"" + (256 + i) + "\" r:id=\"rId" + (i + 2) + "\"/>";
  return "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>" +
    "<p:presentation xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">" +
    "<p:sldMasterIdLst><p:sldMasterId id=\"2147483648\" r:id=\"rId1\"/></p:sldMasterIdLst><p:sldIdLst>" + ids + "</p:sldIdLst>" +
    "<p:sldSz cx=\"" + WIDTH + "\" cy=\"" + HEIGHT + "\"/><p:notesSz cx=\"" + WIDTH + "\" cy=\"" + HEIGHT + "\"/>" +
    "<p:defaultTextStyle/></p:presentation>";
}

function relationshipsXml(items) {
  const body = items.map((item) => "<Relationship Id=\"" + xmlEscape(item.id) + "\" Type=\"" +
    xmlEscape(item.type) + "\" Target=\"" + xmlEscape(item.target) + "\"/>").join("");
  return "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>" +
    "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">" + body + "</Relationships>";
}

function masterXml() {
  return "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>" +
    "<p:sldMaster xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">" +
    "<p:cSld><p:spTree>" + groupTree() + "</p:spTree></p:cSld>" +
    "<p:sldLayoutIdLst><p:sldLayoutId id=\"2147483649\" r:id=\"rId1\"/></p:sldLayoutIdLst></p:sldMaster>";
}

function layoutXml() {
  return "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>" +
    "<p:sldLayout xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" type=\"blank\" preserve=\"1\">" +
    "<p:cSld name=\"Blank\"><p:spTree>" + groupTree() + "</p:spTree></p:cSld>" +
    "<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>";
}

function packageEntries(slides) {
  const contentTypes = [
    "<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>",
    "<Default Extension=\"xml\" ContentType=\"application/xml\"/>",
  ];
  if (slides.some((slide) => slide.hasImage)) contentTypes.push("<Default Extension=\"png\" ContentType=\"image/png\"/>");
  contentTypes.push(
    "<Override PartName=\"/ppt/presentation.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml\"/>",
    "<Override PartName=\"/ppt/slideMasters/slideMaster1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml\"/>",
    "<Override PartName=\"/ppt/slideLayouts/slideLayout1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml\"/>",
  );
  for (let i = 0; i < slides.length; i += 1) contentTypes.push(
    "<Override PartName=\"/ppt/slides/slide" + (i + 1) + ".xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>",
  );

  const presentationRels = [{ id: "rId1", type: REL.slideMaster, target: "slideMasters/slideMaster1.xml" }];
  slides.forEach((_, index) => presentationRels.push({
    id: "rId" + (index + 2), type: REL.slide, target: "slides/slide" + (index + 1) + ".xml",
  }));

  const entries = [
    ["[Content_Types].xml", "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">" + contentTypes.join("") + "</Types>"],
    ["_rels/.rels", relationshipsXml([{ id: "rId1", type: REL.officeDocument, target: "ppt/presentation.xml" }])],
    ["ppt/presentation.xml", presentationXml(slides.length)],
    ["ppt/_rels/presentation.xml.rels", relationshipsXml(presentationRels)],
    ["ppt/slideMasters/slideMaster1.xml", masterXml()],
    ["ppt/slideMasters/_rels/slideMaster1.xml.rels", relationshipsXml([
      { id: "rId1", type: REL.slideLayout, target: "../slideLayouts/slideLayout1.xml" },
    ])],
    ["ppt/slideLayouts/slideLayout1.xml", layoutXml()],
    ["ppt/slideLayouts/_rels/slideLayout1.xml.rels", relationshipsXml([
      { id: "rId1", type: REL.slideMaster, target: "../slideMasters/slideMaster1.xml" },
    ])],
  ];

  slides.forEach((slide, index) => {
    const number = index + 1;
    entries.push(["ppt/slides/slide" + number + ".xml", slide.xml]);
    const rels = [{ id: "rId1", type: REL.slideLayout, target: "../slideLayouts/slideLayout1.xml" }];
    if (slide.hasImage) {
      rels.push({ id: "rId2", type: REL.image, target: "../media/image" + number + ".png" });
      entries.push(["ppt/media/image" + number + ".png", PNG]);
    }
    entries.push(["ppt/slides/_rels/slide" + number + ".xml.rels", relationshipsXml(rels)]);
  });
  return entries;
}

function zipStore(entries) {
  const sorted = [...entries].sort(([left], [right]) => (left < right ? -1 : left > right ? 1 : 0));
  const localParts = [];
  const centralParts = [];
  let offset = 0;
  for (const [entryName, value] of sorted) {
    const name = Buffer.from(entryName, "utf8");
    const data = Buffer.isBuffer(value) ? value : Buffer.from(value, "utf8");
    const checksum = crc32(data);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);
    local.writeUInt16LE(0x0800, 6);
    local.writeUInt16LE(0, 8);
    local.writeUInt16LE(0, 10);
    local.writeUInt16LE(0x0021, 12);
    local.writeUInt32LE(checksum, 14);
    local.writeUInt32LE(data.length, 18);
    local.writeUInt32LE(data.length, 22);
    local.writeUInt16LE(name.length, 26);
    local.writeUInt16LE(0, 28);
    localParts.push(local, name, data);

    const central = Buffer.alloc(46);
    central.writeUInt32LE(0x02014b50, 0);
    central.writeUInt16LE(20, 4);
    central.writeUInt16LE(20, 6);
    central.writeUInt16LE(0x0800, 8);
    central.writeUInt16LE(0, 10);
    central.writeUInt16LE(0, 12);
    central.writeUInt16LE(0x0021, 14);
    central.writeUInt32LE(checksum, 16);
    central.writeUInt32LE(data.length, 20);
    central.writeUInt32LE(data.length, 24);
    central.writeUInt16LE(name.length, 28);
    central.writeUInt16LE(0, 30);
    central.writeUInt16LE(0, 32);
    central.writeUInt16LE(0, 34);
    central.writeUInt16LE(0, 36);
    central.writeUInt32LE(0, 38);
    central.writeUInt32LE(offset, 42);
    centralParts.push(central, name);
    offset += local.length + name.length + data.length;
  }
  const directory = Buffer.concat(centralParts);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(0, 4);
  end.writeUInt16LE(0, 6);
  end.writeUInt16LE(sorted.length, 8);
  end.writeUInt16LE(sorted.length, 10);
  end.writeUInt32LE(directory.length, 12);
  end.writeUInt32LE(offset, 16);
  end.writeUInt16LE(0, 20);
  return Buffer.concat([...localParts, directory, end]);
}

function verifyStoredZip(bytes, expectedNames) {
  const found = [];
  let offset = 0;
  while (bytes.readUInt32LE(offset) === 0x04034b50) {
    const checksum = bytes.readUInt32LE(offset + 14);
    const size = bytes.readUInt32LE(offset + 18);
    const nameSize = bytes.readUInt16LE(offset + 26);
    const extraSize = bytes.readUInt16LE(offset + 28);
    if (bytes.readUInt16LE(offset + 6) !== 0x0800 || bytes.readUInt16LE(offset + 8) !== 0 || size !== bytes.readUInt32LE(offset + 22)) {
      throw new Error("fixture ZIP is not deterministic UTF-8 STORE format");
    }
    const nameAt = offset + 30;
    const name = bytes.subarray(nameAt, nameAt + nameSize).toString("utf8");
    const dataAt = nameAt + nameSize + extraSize;
    if (crc32(bytes.subarray(dataAt, dataAt + size)) !== checksum) throw new Error("fixture ZIP CRC mismatch: " + name);
    found.push(name);
    offset = dataAt + size;
  }
  if (bytes.readUInt32LE(offset) !== 0x02014b50) throw new Error("fixture ZIP central directory is missing");
  const expected = [...expectedNames].sort();
  if (found.join("\n") !== expected.join("\n")) throw new Error("fixture ZIP parts differ from generated package parts");
}

function fixtureSlides() {
  return [
    {
      name: "controlled-projection.pptx",
      slides: [
        { hasImage: false, xml: slideXml("Synthetic geometry and text", geometryShape() + textShape(3, "Projection title", "Синтетический слайд с геометрией", 914400, 685800, 7620000, 685800)) },
        { hasImage: false, xml: slideXml("Synthetic table", tableFrame()) },
        { hasImage: false, xml: slideXml("Unsupported synthetic graphic", unknownGraphic()) },
      ],
    },
    {
      name: "supported-roundtrip.pptx",
      slides: [
        { hasImage: false, xml: slideXml("Supported text one", textShape(2, "Text slide one", "PUBLIC-SYNTHETIC-01", 914400, 685800, 7620000, 685800)) },
        { hasImage: true, xml: slideXml("Supported text and PNG", textShape(2, "Text slide two", "PUBLIC-SYNTHETIC-02", 914400, 685800, 7620000, 685800) + pictureShape()) },
      ],
    },
  ];
}

function generate() {
  return fixtureSlides().map(({ name, slides }) => {
    const entries = packageEntries(slides);
    const bytes = zipStore(entries);
    if (!bytes.equals(zipStore(packageEntries(slides)))) throw new Error(name + " generation is not byte-deterministic");
    verifyStoredZip(bytes, entries.map(([entryName]) => entryName));
    return { name, bytes };
  });
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const outputDirectory = path.join(args.root, "fixtures", "public", "slides");
  const fixtures = generate();
  if (!args.check) await fs.mkdir(outputDirectory, { recursive: true });
  for (const fixture of fixtures) {
    const output = path.join(outputDirectory, fixture.name);
    let existing;
    try {
      existing = await fs.readFile(output);
    } catch (error) {
      if (error?.code !== "ENOENT") throw error;
    }
    if (args.check) {
      if (!existing) throw new Error("missing fixture: " + output);
      if (!existing.equals(fixture.bytes)) throw new Error("fixture differs from generator: " + output);
      console.log("verified " + fixture.name + " (" + fixture.bytes.length + " bytes)");
    } else if (existing && existing.equals(fixture.bytes)) {
      console.log("unchanged " + fixture.name + " (" + fixture.bytes.length + " bytes)");
    } else {
      if (existing && !args.force) throw new Error("refusing to overwrite " + output + "; pass --force to regenerate");
      await fs.writeFile(output, fixture.bytes);
      console.log("wrote " + fixture.name + " (" + fixture.bytes.length + " bytes)");
    }
  }
}

main().catch((error) => {
  console.error(error.message);
  process.exitCode = 1;
});
