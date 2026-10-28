// SPDX-License-Identifier: MIT
//
// Deterministic, self-contained CID word-spacing fixtures. Every PDF object,
// font table and glyph outline below is authored here; no external font or PDF
// is read. The generated font and documents are covered by the repository's MIT
// license. Requires only Node.js; it never reads the private regression input.
//
//   node tools/generate-public-pdf-fixtures.mjs
//   node tools/generate-public-pdf-fixtures.mjs --check
//   node tools/generate-public-pdf-fixtures.mjs --output-dir <directory>
//
// Existing different files are never overwritten. --check only compares bytes.

import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const u16 = (value) => { const b = Buffer.alloc(2); b.writeUInt16BE(value); return b; };
const i16 = (value) => { const b = Buffer.alloc(2); b.writeInt16BE(value); return b; };
const u32 = (value) => { const b = Buffer.alloc(4); b.writeUInt32BE(value >>> 0); return b; };
const u64 = (value) => { const b = Buffer.alloc(8); b.writeBigUInt64BE(BigInt(value)); return b; };
const ascii = (value) => Buffer.from(value, 'ascii');
const join = (...parts) => Buffer.concat(parts.flat());
const pad4 = (bytes) => join(bytes, Buffer.alloc((4 - bytes.length % 4) % 4));

function checksum(bytes) {
  const padded = pad4(bytes);
  let sum = 0;
  for (let i = 0; i < padded.length; i += 4) sum = (sum + padded.readUInt32BE(i)) >>> 0;
  return sum;
}

function rectangle(x0, y0, x1, y1) {
  // One simple TrueType contour with four on-curve points, no instructions.
  const points = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
  const dx = [], dy = [];
  let x = 0, y = 0;
  for (const [nextX, nextY] of points) {
    dx.push(i16(nextX - x)); dy.push(i16(nextY - y));
    x = nextX; y = nextY;
  }
  return pad4(join(i16(1), i16(x0), i16(y0), i16(x1), i16(y1),
    u16(3), u16(0), Buffer.from([1, 1, 1, 1]), dx, dy));
}

function nameTable() {
  const names = [[1, 'Rudra Synthetic CID'], [2, 'Regular'],
    [4, 'Rudra Synthetic CID Regular'], [6, 'RudraSyntheticCID-Regular']];
  const records = [], strings = [];
  let offset = 0;
  for (const [id, text] of names) {
    const value = Buffer.from(text, 'utf16le').swap16();
    records.push(join(u16(3), u16(1), u16(0x0409), u16(id), u16(value.length), u16(offset)));
    strings.push(value);
    offset += value.length;
  }
  return join(u16(0), u16(names.length), u16(6 + names.length * 12), records, strings);
}

function makeFont() {
  // GIDs 0,1,2,3 = empty .notdef, synthetic A, synthetic B, empty space.
  // Different solid quadrilaterals make both sides independently observable.
  const glyphA = rectangle(0, 0, 450, 700);
  const glyphB = rectangle(50, 50, 350, 650);
  const glyf = join(glyphA, glyphB);
  const loca = join([0, 0, glyphA.length, glyf.length, glyf.length].map(u32));

  // Only Mac Roman format 0 is provided. A Unicode cmap would select the
  // renderer's shaped-Unicode path, a different and separately unproven case.
  const macGlyphs = Buffer.alloc(256);
  macGlyphs[0x20] = 3; macGlyphs[0x41] = 1; macGlyphs[0x42] = 2;
  const cmap = join(u16(0), u16(1), u16(1), u16(0), u32(12),
    u16(0), u16(262), u16(0), macGlyphs);
  const head = join(u32(0x00010000), u32(0x00010000), u32(0), u32(0x5f0f3cf5),
    u16(0x000b), u16(1000), u64(2082844800), u64(2082844800),
    i16(0), i16(0), i16(450), i16(700), u16(0), u16(8), i16(2), i16(1), i16(0));
  const hhea = join(u32(0x00010000), i16(800), i16(-200), i16(0), u16(600),
    i16(0), i16(150), i16(450), i16(1), i16(0), i16(0),
    i16(0), i16(0), i16(0), i16(0), i16(0), u16(4));
  const maxp = join(u32(0x00010000), [4, 4, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0].map(u16));
  const hmtx = join([0, 1, 2, 3].map((gid) => join(u16(600), i16(gid === 2 ? 50 : 0))));
  const os2 = join(u16(0), i16(600), u16(400), u16(5), u16(0),
    [650, 699, 0, 140, 650, 699, 0, 479, 50, 250, 0].map(i16), Buffer.alloc(10),
    [0, 0, 0, 0].map(u32), ascii('RDOF'), u16(0x40), u16(0x20), u16(0x42),
    i16(800), i16(-200), i16(0), u16(800), u16(200));
  const post = join(u32(0x00030000), u32(0), i16(-100), i16(50),
    u32(0), u32(0), u32(0), u32(0), u32(0));
  assert.equal(head.length, 54); assert.equal(hhea.length, 36);
  assert.equal(maxp.length, 32); assert.equal(os2.length, 78); assert.equal(post.length, 32);

  const tables = new Map(Object.entries({ 'OS/2': os2, cmap, glyf, head, hhea, hmtx,
    loca, maxp, name: nameTable(), post }));
  const tags = [...tables.keys()].sort();
  const num = tags.length;
  const power = 2 ** Math.floor(Math.log2(num));
  let offset = 12 + num * 16;
  const entries = [], contents = [];
  let headOffset = 0;
  for (const tag of tags) {
    const data = tables.get(tag);
    if (tag === 'head') headOffset = offset;
    entries.push(join(ascii(tag), u32(checksum(data)), u32(offset), u32(data.length)));
    contents.push(pad4(data));
    offset += pad4(data).length;
  }
  const font = join(u32(0x00010000), u16(num), u16(power * 16),
    u16(Math.log2(power)), u16(num * 16 - power * 16), entries, contents);
  font.writeUInt32BE((0xb1b0afba - checksum(font)) >>> 0, headOffset + 8);
  assert.equal(checksum(font), 0xb1b0afba);
  return font;
}

function stream(data, extra = '') {
  return join(ascii(`<< /Length ${data.length}${extra} >>\nstream\n`), data, ascii('\nendstream'));
}

function makePdf(wordSpacing, font, firstUnicode = '0041', lastUnicode = '0042') {
  const cmap = ascii(`/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n` +
    `/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n` +
    `/CMapName /RudraSyntheticUnicode def\n/CMapType 2 def\n` +
    `1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n` +
    `3 beginbfchar\n<0001> <${firstUnicode}>\n<0003> <0020>\n<0002> <${lastUnicode}>\nendbfchar\n` +
    `endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n`);
  const pageContent = ascii(`BT /F0 20 Tf ${wordSpacing} Tw 20 60 Td <000100030002> Tj ET\n`);
  const objects = [
    ascii('<< /Type /Catalog /Pages 2 0 R >>'),
    ascii('<< /Type /Pages /Kids [3 0 R] /Count 1 >>'),
    ascii('<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /Font << /F0 5 0 R >> >> /Contents 4 0 R >>'),
    stream(pageContent),
    ascii('<< /Type /Font /Subtype /Type0 /BaseFont /RudraSyntheticCID-Regular /Encoding /Identity-H /DescendantFonts [6 0 R] /ToUnicode 9 0 R >>'),
    ascii('<< /Type /Font /Subtype /CIDFontType2 /BaseFont /RudraSyntheticCID-Regular /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor 7 0 R /CIDToGIDMap /Identity /DW 600 >>'),
    ascii('<< /Type /FontDescriptor /FontName /RudraSyntheticCID-Regular /Flags 4 /FontBBox [0 0 450 700] /ItalicAngle 0 /Ascent 800 /Descent -200 /CapHeight 700 /StemV 80 /FontFile2 8 0 R >>'),
    stream(font, ` /Length1 ${font.length}`),
    stream(cmap),
  ];
  const parts = [ascii('%PDF-1.4\n%\xE2\xE3\xCF\xD3\n')];
  const offsets = [0];
  let offset = parts[0].length;
  for (let i = 0; i < objects.length; i++) {
    offsets.push(offset);
    const object = join(ascii(`${i + 1} 0 obj\n`), objects[i], ascii('\nendobj\n'));
    parts.push(object); offset += object.length;
  }
  const xref = offset;
  parts.push(ascii(`xref\n0 ${offsets.length}\n0000000000 65535 f \n`));
  for (const position of offsets.slice(1)) parts.push(ascii(`${String(position).padStart(10, '0')} 00000 n \n`));
  parts.push(ascii(`trailer\n<< /Size ${offsets.length} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`));
  return join(parts);
}

function verifyFixtureStructure(bytes, tw) {
  const text = bytes.toString('latin1');
  for (const token of ['/Subtype /Type0', '/Encoding /Identity-H', '/Subtype /CIDFontType2',
    '/CIDToGIDMap /Identity', '<0003> <0020>', '<000100030002> Tj', `${tw} Tw`]) {
    assert.ok(text.includes(token), `missing ${token}`);
  }
  const match = /startxref\n(\d+)\n%%EOF\n$/.exec(text);
  assert.ok(match); assert.equal(text.slice(Number(match[1]), Number(match[1]) + 5), 'xref\n');
  const entries = /xref\n0 (\d+)\n([\s\S]*?)trailer\n/.exec(text);
  assert.ok(entries);
  const rows = entries[2].split('\n');
  for (let id = 1; id < Number(entries[1]); id++) {
    const start = Number(rows[id].slice(0, 10));
    assert.equal(text.slice(start, start + `${id} 0 obj\n`.length), `${id} 0 obj\n`);
  }
}

function generate() {
  const font = makeFont();
  const control = makePdf('00.000', font);
  const trigger = makePdf('40.000', font);
  verifyFixtureStructure(control, '00.000'); verifyFixtureStructure(trigger, '40.000');
  assert.equal(trigger.length, control.length);
  const changes = [];
  for (let i = 0; i < trigger.length; i++) if (trigger[i] !== control[i]) changes.push(i);
  assert.equal(changes.length, 1, 'the Tw operand must be the only changed byte');
  const cyrillic = makePdf('00.000', font, '0410', '0411');
  verifyFixtureStructure(cyrillic, '00.000');
  assert.ok(cyrillic.includes(ascii('<0001> <0410>')));
  assert.ok(cyrillic.includes(ascii('<0002> <0411>')));
  return new Map([['cid-unicode-space-control.pdf', control], ['cid-unicode-space-tw.pdf', trigger], ['cid-cyrillic.pdf', cyrillic]]);
}

function main() {
  const args = process.argv.slice(2);
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
  let output = path.join(root, 'fixtures', 'public', 'pdf');
  let check = false;
  for (let i = 0; i < args.length; i++) {
    if (args[i] === '--check') check = true;
    else if (args[i] === '--output-dir' && args[i + 1]) output = path.resolve(args[++i]);
    else throw new Error(`unknown or incomplete argument: ${args[i]}`);
  }
  const fixtures = generate();
  if (!check) fs.mkdirSync(output, { recursive: true });
  // Preflight every target first so a mismatch cannot leave a partial pair.
  for (const [name, bytes] of fixtures) {
    const target = path.join(output, name);
    if (fs.existsSync(target)) {
      assert.ok(fs.readFileSync(target).equals(bytes), `refusing to overwrite differing ${target}`);
    } else if (check) throw new Error(`missing ${target}`);
  }
  if (!check) for (const [name, bytes] of fixtures) {
    const target = path.join(output, name);
    if (!fs.existsSync(target)) fs.writeFileSync(target, bytes, { flag: 'wx' });
  }
  for (const [name, bytes] of fixtures) console.log(`${check ? 'checked' : 'ready'} ${name}: ${bytes.length} bytes`);
}

main();
