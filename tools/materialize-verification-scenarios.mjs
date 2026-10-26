// Writes test recipes only. Never launches an app, changes clipboard or runs GUI.
import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {readJson} from './verification-policy.mjs';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const policy = readJson(path.join(root, 'verification/verification-policy.json'));
const artifact = path.join(root, 'releases/rc28/rudra-office-app.exe');
const evidence = path.join(root, 'reports/evidence/sheets-clipboard-editing-20261007/scenarios-attempt-07');
let steps = [];
const step = (op, data = {}) => steps.push({op, session: 'app', ...data});
const click = id => step('click', {id});
const set = (id, value) => step('set_value', {id, value});
const wait = (id, value) => step('wait_for_value', {id, value});
const inspect = () => step('inspect_ui', {save_as: `inspect-${steps.length}`});
const key = keys => {step('activate_window'); step('hotkey', {keys});};
const grid = (doc = 1) => {step('activate_window'); step('focus', {id: `rudra-office/document:${doc}/sheets/grid`});};
function start() {steps = [{op: 'session.open', save_session_as: 'app'}]; inspect();}
function book() {start(); click('rudra-office/home/new-xlsx'); step('wait_for_automation_id', {id: 'rudra-office/document:1/sheets/grid'}); inspect();}
function range(value, doc = 1) {
  click('rudra-office/menu/data-trigger'); set('rudra-office/sheets/address-input', value);
  click('rudra-office/ribbon/go-to-range');
  wait(`rudra-office/document:${doc}/sheets/selection`, /^[A-Z]+\d+$/.test(value) ? `${value}:${value}` : value);
  click('rudra-office/menu/home-trigger');
}
function write(address, value, doc = 1) {range(address, doc); set('rudra-office/home/sheets/cell-value', value); click('rudra-office/home/sheets/cell-apply'); wait('rudra-office/home/sheets/cell-value', value);}
function cell(address, raw, doc = 1) {range(address, doc); wait('rudra-office/home/sheets/cell-value', raw);}
function open(file, doc, canvas = 'sheets/grid') {
  click('rudra-office/menu/file-trigger'); step('wait_idle', {timeout_ms: 5000});
  step('wait_enabled', {id: 'rudra-office/action/open'}); set('rudra-office/action/open', path.join(root, file));
  wait('rudra-office/action/open', path.join(root, file)); click('rudra-office/action/open');
  step('wait_for_automation_id', {id: `rudra-office/document:${doc}/tab`});
  if (canvas === 'sheets/grid') step('wait_for_automation_id', {id: `rudra-office/document:${doc}/sheets/grid`});
  else step('wait_for_automation_id', {id: canvas});
  inspect();
}
function finish(id) {
  inspect(); step('screenshot_window', {step: 'final'}); step('get_runtime_health'); step('get_runtime_stderr'); step('windows_export_evidence');
  const scenario = {version: 1, name: `rudra-policy-${id}`, application: {command: artifact, window_title: 'Rudra Office'}, options: {stop_on_error: true, evidence_on_failure: true}, steps, teardown: [{op: 'session.close', session: 'app'}]};
  const target = policy.scenario_registry.find(s => s.id === id);
  fs.mkdirSync(path.dirname(path.join(root, target.file)), {recursive: true});
  fs.writeFileSync(path.join(root, target.file), JSON.stringify(scenario, null, 2) + '\n');
}
function sourceBody(name, {dropBegin = false, dropEnd = true} = {}) {
  const source = readJson(path.join(evidence, `${name}.aiui.json`));
  let body = structuredClone(source.steps);
  if (dropBegin) {
    const ready = body.findIndex(s => s.op === 'inspect_ui' && s.save_as === 'new-book-controls');
    if (ready < 0) throw new Error(`source begin boundary missing ${name}`);
    body = body.slice(ready + 1);
  }
  if (dropEnd) {
    const end = body.findIndex(s => s.op === 'inspect_ui' && s.save_as === 'final-state');
    if (end < 0) throw new Error(`source end boundary missing ${name}`);
    body = body.slice(0, end);
  }
  return body;
}
function selectionHistory() {
  write('A1', '5'); write('B2', '9'); range('A1:B2'); grid();
  pointerRange();
  click('rudra-office/menu/data-trigger'); click('rudra-office/ribbon/clear-range');
  click('rudra-office/menu/home-trigger'); cell('A1', ''); cell('B2', '');
  click('rudra-office/home/sheets/undo'); cell('A1', '5'); cell('B2', '9');
  click('rudra-office/home/sheets/redo'); cell('A1', ''); cell('B2', '');
}
function pointerRange() {
  const first = 'rudra-office/document:1/cell:1:0:0';
  const last = 'rudra-office/document:1/cell:1:1:1';
  step('inspect_spatial_target', {id: first}); step('inspect_spatial_target', {id: last});
  step('pointer_down', {id: first, position: {x: 0.5, y: 0.5}, button: 'left'});
  step('read', {id: 'rudra-office/document:1/sheets/selection'});
  step('move_pointer', {id: last, position: {x: 0.5, y: 0.5}});
  step('read', {id: 'rudra-office/document:1/sheets/selection'}); step('pointer_up', {button: 'left'});
  wait('rudra-office/document:1/sheets/selection', 'A1:B2');
}
function dataOperations() {
  for (const [address, value] of Object.entries({A1: 'ID', B1: 'Status', C1: 'Formula', A2: '3', B2: 'Active', C2: '=A2*10', A3: '1', B3: 'Inactive', C3: '=A3*10', A4: '2', B4: 'Active', C4: '=A4*10'})) write(address, value);
  range('A1:C4'); click('rudra-office/menu/data-trigger');
  set('rudra-office/sheets/key-column', 'A'); set('rudra-office/sheets/has-header', 'Есть заголовок');
  click('rudra-office/ribbon/sort-ascending'); click('rudra-office/menu/home-trigger');
  cell('A2', '1'); cell('B2', 'Inactive'); cell('C2', '=A2*10');
  range('A1:C4'); click('rudra-office/menu/data-trigger'); set('rudra-office/sheets/key-column', 'B');
  set('rudra-office/sheets/filter-kind', 'Равно'); set('rudra-office/sheets/filter-value', 'Active');
  click('rudra-office/ribbon/apply-auto-filter'); wait('rudra-office/document:1/sheets/filter-hidden-rows', '2');
  range('A1'); grid(); step('press', {key: 'DOWN'}); wait('rudra-office/document:1/sheets/selection/active', 'A3');
  click('rudra-office/menu/data-trigger'); click('rudra-office/ribbon/clear-auto-filter'); wait('rudra-office/document:1/sheets/filter-hidden-rows', '');
  click('rudra-office/menu/home-trigger');
}
function structural() {
  write('G1', '10'); write('H1', '=G1*2'); range('G1');
  click('rudra-office/menu/insert-trigger'); click('rudra-office/ribbon/insert-rows'); click('rudra-office/menu/home-trigger');
  cell('G2', '10'); cell('H2', '=G2*2'); click('rudra-office/home/sheets/undo'); cell('G1', '10'); cell('H1', '=G1*2');
  range('G1'); click('rudra-office/menu/insert-trigger'); click('rudra-office/ribbon/insert-columns'); click('rudra-office/menu/home-trigger');
  cell('H1', '10'); cell('I1', '=H1*2'); click('rudra-office/home/sheets/undo'); cell('G1', '10'); cell('H1', '=G1*2');
}
function multiDocument({pendingRedo = false} = {}) {
  write('K1', 'Alpha');
  if (pendingRedo) click('rudra-office/home/sheets/undo');
  open('reports/evidence/sheets-selection-undo-20261006/cross-document-copy.xlsx', 2);
  range('A1', 2); step('read', {id: 'rudra-office/home/sheets/cell-value', save_as: 'doc2_baseline'});
  set('rudra-office/home/sheets/cell-value', 'Beta'); click('rudra-office/home/sheets/cell-apply'); wait('rudra-office/home/sheets/cell-value', 'Beta');
  click('rudra-office/home/sheets/undo'); wait('rudra-office/home/sheets/cell-value', '${doc2_baseline.value}');
  step('select', {id: 'rudra-office/document:1/tab'}); step('wait_selected', {id: 'rudra-office/document:1/tab'});
  if (pendingRedo) {click('rudra-office/home/sheets/redo'); cell('K1', 'Alpha');}
  else {cell('K1', 'Alpha'); click('rudra-office/home/sheets/undo'); cell('K1', ''); click('rudra-office/home/sheets/redo'); cell('K1', 'Alpha');}
  step('select', {id: 'rudra-office/document:2/tab'}); step('wait_selected', {id: 'rudra-office/document:2/tab'});
  range('A1', 2); wait('rudra-office/home/sheets/cell-value', '${doc2_baseline.value}');
  click('rudra-office/home/sheets/redo'); wait('rudra-office/home/sheets/cell-value', 'Beta');
  click('rudra-office/document:1/tab/close'); step('wait_hidden', {id: 'rudra-office/document:1/tab'});
  step('wait_selected', {id: 'rudra-office/document:2/tab'}); wait('rudra-office/home/sheets/cell-value', 'Beta');
}

book(); selectionHistory(); finish('golden-selection-history');
book(); dataOperations(); finish('golden-data-operations');
book(); structural(); finish('golden-structural');
book(); multiDocument(); finish('golden-multi-document');
book(); selectionHistory(); dataOperations();
range('A3:C4'); click('rudra-office/home/sheets/copy'); range('A8'); click('rudra-office/home/sheets/paste');
cell('A8', '2'); cell('C8', '=A8*10'); structural(); multiDocument({pendingRedo: true}); finish('expanded-sheets-composition');

// One native clipboard launch: over-cap seed is consumed/rejected first, then
// the existing same-text overwrite + external quoted TSV boundary runs in it.
steps = sourceBody('oversized-external-clipboard-keyboard-reject');
steps.push(...sourceBody('external-tsv-and-identical-overwrite', {dropBegin: true})); finish('clipboard-native-system');
steps = sourceBody('rich-copy-formula-format-history');
steps.push(...sourceBody('cross-book-copy-cut-reject', {dropBegin: true})); finish('clipboard-rich-cross-book');
steps = sourceBody('native-textinput-focus-routing');
click('rudra-office/menu/data-trigger'); set('rudra-office/sheets/address-input', 'B5');
step('activate_window'); step('focus', {id: 'rudra-office/sheets/address-input'});
key(['CTRL', 'A']); key(['CTRL', 'C']); key(['CTRL', 'X']); wait('rudra-office/sheets/address-input', '');
key(['CTRL', 'V']); wait('rudra-office/sheets/address-input', 'B5');
click('rudra-office/ribbon/go-to-range'); wait('rudra-office/document:1/sheets/selection', 'B5:B5');
click('rudra-office/menu/home-trigger'); range('A1'); click('rudra-office/home/sheets/copy'); finish('clipboard-focus-routing');
steps = sourceBody('cut-deferred-atomic-escape'); finish('clipboard-deferred-cut');
steps = sourceBody('real-xlsx-copy-only'); finish('sheets-real-copy-only');
book(); write('A1', 'Русский текст'); write('B2', 'visible'); range('A1:B2'); grid(); pointerRange();
step('windows_set_window_bounds', {x: 80, y: 60, width: 1000, height: 760}); inspect(); step('screenshot_window', {step: 'small-viewport'});
wait('rudra-office/document:1/sheets/selection', 'A1:B2');
step('windows_set_window_bounds', {x: 60, y: 40, width: 1550, height: 975}); inspect(); finish('grid-focused-runtime');

const docx = 'fixtures/release-acceptance/docx/writer-layout-a4-multipage.docx';
const pdf = 'vendor/pdf-oxide/tests/fixtures/hello_structure.pdf';
start(); open(docx, 1, 'rudra-office/writer/canvas'); open(pdf, 2, 'rudra-office/pdf/canvas');
step('select', {id: 'rudra-office/document:1/tab'}); step('wait_selected', {id: 'rudra-office/document:1/tab'}); step('wait_for_automation_id', {id: 'rudra-office/writer/canvas'});
click('rudra-office/document:2/tab/close'); step('wait_hidden', {id: 'rudra-office/document:2/tab'}); step('wait_selected', {id: 'rudra-office/document:1/tab'});
click('rudra-office/document:1/tab/close'); step('wait_hidden', {id: 'rudra-office/document:1/tab'});
click('rudra-office/home/new-pptx'); step('wait_for_automation_id', {id: 'rudra-office/document:3/tab'}); step('wait_for_automation_id', {id: 'rudra-office/slides/canvas'});
click('rudra-office/document:3/tab/close'); step('wait_hidden', {id: 'rudra-office/document:3/tab'});
click('rudra-office/home/new-xlsx'); step('wait_for_automation_id', {id: 'rudra-office/document:4/sheets/grid'}); click('rudra-office/document:4/tab/close'); step('wait_hidden', {id: 'rudra-office/document:4/tab'}); finish('expanded-office-sessions');
start(); open(pdf, 1, 'rudra-office/pdf/canvas'); finish('pdf-parser-focused');
start(); open(docx, 1, 'rudra-office/writer/canvas'); finish('writer-parser-focused');
// Deliberately requires fixture review in the manifest; not real-PPTX proof.
start(); click('rudra-office/home/new-pptx'); step('wait_for_automation_id', {id: 'rudra-office/slides/canvas'}); finish('slides-parser-focused');
book(); wait('rudra-office/document:1/sheets/selection', 'A1:A1'); finish('frozen-artifact-smoke');
console.log(JSON.stringify({recipes: policy.scenario_registry.length, gui_run: false, office_build: false, clipboard_changed: false, public_script_validation_still_required: true}));
