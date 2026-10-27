#!/usr/bin/env node

import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const stagedRoot = path.resolve(scriptDir, '..');
const encoder = new TextEncoder();

const W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main';
const R = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const REL = 'http://schemas.openxmlformats.org/package/2006/relationships';
const CT = 'http://schemas.openxmlformats.org/package/2006/content-types';
const WP = 'http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing';
const A = 'http://schemas.openxmlformats.org/drawingml/2006/main';
const PIC = 'http://schemas.openxmlformats.org/drawingml/2006/picture';

const crcTable = new Uint32Array(256);
for (let n = 0; n < 256; n += 1) {
  let value = n;
  for (let bit = 0; bit < 8; bit += 1) {
    value = (value & 1) ? (0xedb88320 ^ (value >>> 1)) : (value >>> 1);
  }
  crcTable[n] = value >>> 0;
}

function crc32(bytes) {
  let value = 0xffffffff;
  for (const byte of bytes) {
    value = crcTable[(value ^ byte) & 0xff] ^ (value >>> 8);
  }
  return (value ^ 0xffffffff) >>> 0;
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

function pngChunk(type, data) {
  const typeBytes = Buffer.from(type, 'ascii');
  const body = Buffer.concat([typeBytes, data]);
  const result = Buffer.alloc(12 + data.length);
  result.writeUInt32BE(data.length, 0);
  body.copy(result, 4);
  result.writeUInt32BE(crc32(body), 8 + data.length);
  return result;
}

function syntheticPng() {
  // A one-pixel RGBA image. Its zlib stream uses a stored DEFLATE block so
  // the bytes do not depend on the host zlib implementation or any asset file.
  const scanline = Buffer.from([0x00, 0x4b, 0x91, 0xd8, 0xff]);
  const deflateBlock = Buffer.alloc(5 + scanline.length);
  deflateBlock[0] = 0x01; // Final stored block, byte aligned.
  deflateBlock.writeUInt16LE(scanline.length, 1);
  deflateBlock.writeUInt16LE((~scanline.length) & 0xffff, 3);
  scanline.copy(deflateBlock, 5);
  const checksum = Buffer.alloc(4);
  checksum.writeUInt32BE(adler32(scanline));
  const zlibStream = Buffer.concat([Buffer.from([0x78, 0x01]), deflateBlock, checksum]);

  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(1, 0);
  ihdr.writeUInt32BE(1, 4);
  ihdr[8] = 8; // Bit depth.
  ihdr[9] = 6; // RGBA.
  ihdr[10] = 0;
  ihdr[11] = 0;
  ihdr[12] = 0;

  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    pngChunk('IHDR', ihdr),
    pngChunk('IDAT', zlibStream),
    pngChunk('IEND', Buffer.alloc(0)),
  ]);
}

function xmlEscape(text) {
  return text
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&apos;');
}

function run(text, { bold = false, italic = false, underline = false, color = null } = {}) {
  const properties = [];
  if (bold) properties.push('<w:b/>');
  if (italic) properties.push('<w:i/>');
  if (underline) properties.push('<w:u w:val="single"/>');
  if (color) properties.push(`<w:color w:val="${color}"/>`);
  const rPr = properties.length ? `<w:rPr>${properties.join('')}</w:rPr>` : '';
  return `<w:r>${rPr}<w:t xml:space="preserve">${xmlEscape(text)}</w:t></w:r>`;
}

function pageBreakParagraph() {
  // Keep the page break in its own paragraph and run. The following body
  // paragraph then remains independent of the break representation.
  return '<w:p><w:r><w:br w:type="page"/></w:r></w:p>';
}

function imageParagraph() {
  const cx = 2_286_000;
  const cy = 1_333_500;
  return `<w:p><w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="${cx}" cy="${cy}"/><wp:effectExtent l="0" t="0" r="0" b="0"/><wp:docPr id="1" name="Synthetic public sample" descr="Generated one-pixel color sample"/><wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect="1"/></wp:cNvGraphicFramePr><a:graphic><a:graphicData uri="${PIC}"><pic:pic><pic:nvPicPr><pic:cNvPr id="0" name="synthetic.png"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rId3"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="${cx}" cy="${cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>`;
}

function tableXml() {
  const cell = (text) => `<w:tc><w:tcPr><w:tcW w:w="3600" w:type="dxa"/></w:tcPr><w:p>${run(text)}</w:p></w:tc>`;
  return `<w:tbl><w:tblPr><w:tblW w:w="7200" w:type="dxa"/></w:tblPr><w:tblGrid><w:gridCol w:w="3600"/><w:gridCol w:w="3600"/></w:tblGrid><w:tr>${cell('Тестовая ячейка')}${cell('42')}</w:tr></w:tbl>`;
}

function documentXml({ rich }) {
  const p1 = `<w:p>${run('Рассвет ', { bold: true, color: '0057B8' })}${run('🙂на орбите', { italic: true })}</w:p>`;
  const p2 = `<w:p>${run('Учебная ')}${run('страница ', { underline: true })}${run('😀', { bold: true, color: '0057B8' })}</w:p>`;
  const extras = rich ? `${tableXml()}${imageParagraph()}` : '';
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="${W}" xmlns:r="${R}" xmlns:wp="${WP}" xmlns:a="${A}" xmlns:pic="${PIC}"><w:body>${p1}${pageBreakParagraph()}${p2}${extras}<w:sectPr><w:headerReference w:type="default" r:id="rId1"/><w:footerReference w:type="default" r:id="rId2"/><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="720" w:footer="720" w:gutter="0"/></w:sectPr></w:body></w:document>`;
}

function contentTypesXml({ rich }) {
  const pngDefault = rich ? '<Default Extension="png" ContentType="image/png"/>' : '';
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="${CT}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>${pngDefault}<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/><Override PartName="/word/footer1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/></Types>`;
}

function rootRelationshipsXml() {
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="${REL}"><Relationship Id="rId1" Type="${R}/officeDocument" Target="word/document.xml"/></Relationships>`;
}

function documentRelationshipsXml({ rich }) {
  const imageRelationship = rich
    ? `<Relationship Id="rId3" Type="${R}/image" Target="media/synthetic.png"/>`
    : '';
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="${REL}"><Relationship Id="rId1" Type="${R}/header" Target="header1.xml"/><Relationship Id="rId2" Type="${R}/footer" Target="footer1.xml"/>${imageRelationship}</Relationships>`;
}

function headerXml() {
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:hdr xmlns:w="${W}"><w:p>${run('Синтетический верх')}</w:p></w:hdr>`;
}

function footerXml() {
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:ftr xmlns:w="${W}"><w:p>${run('Синтетический низ')}</w:p></w:ftr>`;
}

function u16(value) {
  const result = Buffer.alloc(2);
  result.writeUInt16LE(value);
  return result;
}

function u32(value) {
  const result = Buffer.alloc(4);
  result.writeUInt32LE(value >>> 0);
  return result;
}

function zipStored(entries) {
  const localParts = [];
  const centralParts = [];
  let localOffset = 0;
  const dosDate = 0x0021; // 1980-01-01; DOS time is 00:00:00.

  for (const { name, data } of entries) {
    const nameBytes = Buffer.from(name, 'ascii');
    const checksum = crc32(data);
    const localHeader = Buffer.concat([
      u32(0x04034b50), u16(20), u16(0), u16(0), u16(0), u16(dosDate),
      u32(checksum), u32(data.length), u32(data.length), u16(nameBytes.length), u16(0),
      nameBytes,
    ]);
    localParts.push(localHeader, data);

    const centralHeader = Buffer.concat([
      u32(0x02014b50), u16(20), u16(20), u16(0), u16(0), u16(0), u16(dosDate),
      u32(checksum), u32(data.length), u32(data.length), u16(nameBytes.length), u16(0),
      u16(0), u16(0), u16(0), u32(0), u32(localOffset), nameBytes,
    ]);
    centralParts.push(centralHeader);
    localOffset += localHeader.length + data.length;
  }

  const centralDirectory = Buffer.concat(centralParts);
  const endRecord = Buffer.concat([
    u32(0x06054b50), u16(0), u16(0), u16(entries.length), u16(entries.length),
    u32(centralDirectory.length), u32(localOffset), u16(0),
  ]);
  return Buffer.concat([...localParts, centralDirectory, endRecord]);
}

function part(name, content) {
  return { name, data: Buffer.isBuffer(content) ? content : Buffer.from(content, 'utf8') };
}

function makeDocx({ rich }) {
  const entries = [
    part('[Content_Types].xml', contentTypesXml({ rich })),
    part('_rels/.rels', rootRelationshipsXml()),
    part('word/document.xml', documentXml({ rich })),
    part('word/_rels/document.xml.rels', documentRelationshipsXml({ rich })),
    part('word/header1.xml', headerXml()),
    part('word/footer1.xml', footerXml()),
  ];
  if (rich) entries.push(part('word/media/synthetic.png', syntheticPng()));
  return zipStored(entries);
}

function parseArguments(argv) {
  let check = false;
  let outputDir = path.join(stagedRoot, 'fixtures', 'public', 'writer');
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === '--check') {
      check = true;
    } else if (arg === '--output-dir') {
      if (!argv[index + 1]) throw new Error('--output-dir requires a directory');
      outputDir = path.resolve(argv[++index]);
    } else if (arg.startsWith('--output-dir=')) {
      outputDir = path.resolve(arg.slice('--output-dir='.length));
    } else {
      throw new Error(`Unknown argument: ${arg}`);
    }
  }
  const relative = path.relative(stagedRoot, outputDir);
  if (relative === '..' || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) {
    throw new Error(`Output directory must remain inside the staged workspace: ${outputDir}`);
  }
  return { check, outputDir };
}

function main() {
  const { check, outputDir } = parseArguments(process.argv.slice(2));
  const fixtures = [
    ['synthetic-basic.docx', makeDocx({ rich: false })],
    ['synthetic-rich.docx', makeDocx({ rich: true })],
  ];

  if (!check) mkdirSync(outputDir, { recursive: true });
  // Preflight the complete pair before creating any file; never overwrite edits.
  if (!check) {
    for (const [name, expected] of fixtures) {
      const target = path.join(outputDir, name);
      try {
        const actual = readFileSync(target);
        if (!actual.equals(expected)) throw new Error(`Refusing to overwrite differing fixture: ${target}`);
      } catch (error) {
        if (error.code !== 'ENOENT') throw error;
      }
    }
  }
  let mismatch = false;
  for (const [name, expected] of fixtures) {
    const target = path.join(outputDir, name);
    if (check) {
      let actual;
      try {
        actual = readFileSync(target);
      } catch {
        console.error(`MISSING ${target}`);
        mismatch = true;
        continue;
      }
      if (!actual.equals(expected)) {
        console.error(`DIFFERS ${target}`);
        mismatch = true;
        continue;
      }
      console.log(`MATCH ${name} sha256=${createHash('sha256').update(actual).digest('hex')}`);
    } else {
      try { writeFileSync(target, expected, { flag: 'wx' }); }
      catch (error) { if (error.code !== 'EEXIST') throw error; }
      console.log(`WROTE ${target} sha256=${createHash('sha256').update(expected).digest('hex')}`);
    }
  }
  if (mismatch) process.exitCode = 1;
}

main();
