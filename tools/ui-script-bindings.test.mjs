// Source-only regression; does not evaluate Splash or prove GUI health.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';

const appUi = fs.readFileSync(
  new URL('../workspace/crates/rudra-office-app/src/app_ui.rs', import.meta.url),
  'utf8',
);
const popupCount = 6;
const popupEnum = '(?:makepad_widgets\\s*::\\s*drop_down\\s*::\\s*)?PopupMenuPosition';
const directRustValue = new RegExp('^#\\(\\s*' + popupEnum + '\\s*::\\s*BelowInput\\s*\\)$');

function blank(text) { return text.replace(/[^\r\n]/g, ' '); }
function codeOnly(source) {
  return source
    .replace(/"(?:\\.|[^"\\])*"/gs, blank)
    .replace(/\/\*[\s\S]*?\*\//g, blank)
    .replace(/\/\/[^\r\n]*/g, blank);
}

function scriptBody(source) {
  const code = codeOnly(source);
  const marker = code.indexOf('script_mod!');
  const open = code.indexOf('{', marker);
  if (marker < 0 || open < 0) return '';
  let depth = 0;
  for (let i = open; i < code.length; i++) {
    if (code[i] === '{') depth++;
    else if (code[i] === '}' && --depth === 0) return code.slice(open + 1, i);
  }
  return '';
}

function popupConsumers(code) {
  return [...code.matchAll(/\bpopup_menu_position\s*:/g)].map(match => {
    const rhs = code.slice(match.index + match[0].length);
    const value = /^\s*(#\([^)]*\)|[A-Za-z_]\w*(?:\s*\.\s*[A-Za-z_]\w*)*)/.exec(rhs);
    return { index: match.index, value: value ? value[1].replace(/\s+/g, '') : '' };
  });
}

function hasEnumBindingBefore(code, index, alias, kind) {
  if (!/^[A-Za-z_]\w*$/.test(alias)) return false;
  const suffix = kind === 'value'
    ? 'BelowInput'
    : 'script_api\\s*\\(\\s*vm\\s*\\)';
  const re = new RegExp(
    '\\blet\\s+' + alias + '\\s*=\\s*#\\s*\\(\\s*' + popupEnum + '\\s*::\\s*' + suffix + '\\s*\\)',
  );
  return re.test(code.slice(0, index));
}

function sourceIssues(source) {
  const code = scriptBody(source);
  const issues = [];
  const registration = /\bmod\s*\.\s*widgets\s*\.\s*RudraDocumentTabBar\s*=/.exec(code);
  const tabUses = [...code.matchAll(/\btab_bar\s*:\s*(mod\s*\.\s*widgets\s*\.\s*)?RudraDocumentTabBar\s*\{/g)];
  const imports = [...code.matchAll(/^[ \t]*use[ \t]+mod[ \t]*\.[ \t]*widgets[ \t]*\.[ \t]*RudraDocumentTabBar[ \t]*$/gm)];
  if (!registration) issues.push('RudraDocumentTabBar registration is missing');
  if (!tabUses.length) issues.push('tab_bar has no RudraDocumentTabBar consumer');
  for (const use of tabUses) {
    if (registration && registration.index > use.index) issues.push('RudraDocumentTabBar is used before registration');
    if (!use[1] && !imports.some(item => registration && item.index > registration.index && item.index < use.index))
      issues.push('unqualified RudraDocumentTabBar has no preceding local import');
  }

  const consumers = popupConsumers(code);
  if (consumers.length < popupCount)
    issues.push('expected at least ' + popupCount + ' popup consumers, found ' + consumers.length);
  for (const consumer of consumers) {
    const value = consumer.value;
    if (directRustValue.test(value)) continue;
    const valueAlias = /^([A-Za-z_]\w*)$/.exec(value);
    const apiAlias = /^([A-Za-z_]\w*)\.BelowInput$/.exec(value);
    if (valueAlias && hasEnumBindingBefore(code, consumer.index, valueAlias[1], 'value')) continue;
    if (apiAlias && hasEnumBindingBefore(code, consumer.index, apiAlias[1], 'api')) continue;
    issues.push('popup_menu_position is not bound to PopupMenuPosition::BelowInput before use');
  }
  return issues;
}

function fixture({ tab = 'import', popup = 'alias', count = popupCount, declaration, late = false, decoys = false } = {}) {
  const variants = {
    alias: ['let BelowInput = #(makepad_widgets::drop_down::PopupMenuPosition::BelowInput)', 'BelowInput'],
    api: ['let PopupMenuPosition = #(makepad_widgets::drop_down::PopupMenuPosition::script_api(vm))', 'PopupMenuPosition.BelowInput'],
    apiShort: ['let PopupMenuPosition = #(PopupMenuPosition::script_api(vm))', 'PopupMenuPosition.BelowInput'],
    direct: ['', '#(makepad_widgets::drop_down::PopupMenuPosition::BelowInput)'],
    bare: ['', 'BelowInput'],
    qualified: ['', 'PopupMenuPosition.BelowInput'],
  };
  const [defaultBinding, value] = variants[popup];
  const binding = declaration ?? defaultBinding;
  const lines = ['script_mod! {', 'mod.widgets.RudraDocumentTabBar = mod.widgets.TabBarFlat {}'];
  if (tab === 'import') lines.push('use mod.widgets.RudraDocumentTabBar');
  if (decoys) lines.push('// use mod.widgets.RudraDocumentTabBar', '// let BelowInput = #(makepad_widgets::drop_down::PopupMenuPosition::BelowInput)', 'let note = "use mod.widgets.RudraDocumentTabBar let BelowInput";');
  if (!late && binding) lines.push(binding);
  lines.push('DocumentTabStrip { tab_bar: ' + (tab === 'qualified' ? 'mod.widgets.' : '') + 'RudraDocumentTabBar {} }');
  lines.push(...Array(count).fill('DropDown { popup_menu_position: ' + value + ' }'));
  if (late && binding) lines.push(binding);
  lines.push('}');
  return lines.join('\n') + '\n';
}

test('app_ui source binds its tab bar and all six typed popup consumers', () => {
  assert.deepEqual(sourceIssues(appUi), []);
});

test('accepts local import, qualified tab widget, Rust enum aliases, API aliases, and direct enum values', () => {
  for (const input of [
    fixture(),
    fixture({ tab: 'qualified', popup: 'api' }),
    fixture({ tab: 'qualified', popup: 'apiShort' }),
    fixture({ tab: 'qualified', popup: 'direct' }),
  ]) assert.deepEqual(sourceIssues(input), []);
});

test('accepts additional valid popup consumers while checking every value', () => {
  assert.deepEqual(sourceIssues(fixture({ popup: 'apiShort', count: popupCount + 1 })), []);
});

test('rejects an unrecognized popup value instead of skipping it', () => {
  const invalidExtra = fixture().replace(/\n}\n$/, '\nDropDown { popup_menu_position: 42 }\n}\n');
  assert.match(sourceIssues(invalidExtra).join('\n'), /popup_menu_position/);
});

test('in-memory removal of either binding is rejected', () => {
  const good = fixture();
  assert.match(sourceIssues(good.replace('use mod.widgets.RudraDocumentTabBar\n', '')).join('\n'), /unqualified RudraDocumentTabBar/);
  assert.match(sourceIssues(good.replace('let BelowInput = #(makepad_widgets::drop_down::PopupMenuPosition::BelowInput)\n', '')).join('\n'), /popup_menu_position/);
});

test('rejects wrong enum variant or type and late enum binding', () => {
  assert.match(sourceIssues(fixture({ declaration: 'let BelowInput = #(makepad_widgets::drop_down::PopupMenuPosition::OnSelected)\n' })).join('\n'), /popup_menu_position/);
  assert.match(sourceIssues(fixture({ declaration: 'let BelowInput = #(makepad_widgets::drop_down::OtherPosition::BelowInput)\n' })).join('\n'), /popup_menu_position/);
  assert.match(sourceIssues(fixture({ late: true })).join('\n'), /popup_menu_position/);
});

test('rejects unbound enum qualification and comment/string decoys', () => {
  assert.match(sourceIssues(fixture({ popup: 'qualified' })).join('\n'), /popup_menu_position/);
  const deceptive = fixture({ tab: 'missing', popup: 'bare', decoys: true });
  assert.match(sourceIssues(deceptive).join('\n'), /unqualified RudraDocumentTabBar/);
  assert.match(sourceIssues(deceptive).join('\n'), /popup_menu_position/);
});

test('rejects a drop below the six existing popup consumers', () => {
  assert.match(sourceIssues(fixture({ count: popupCount - 1 })).join('\n'), /expected at least 6 popup consumers, found 5/);
});
