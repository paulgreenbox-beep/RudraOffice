import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {validateGuiDevelopment} from './verify-gui-development.mjs';
const original=JSON.parse(fs.readFileSync(new URL('../verification/gui-development.json',import.meta.url),'utf8'));
test('explicit safe canary profile validates without claiming GUI acceptance',()=>{const r=validateGuiDevelopment(original);assert.equal(r.status,'PROFILE_VALID');assert.equal(r.gui_accepted,false);});
for(const [name,patch] of [
 ['legacy connection',{connection:'rust-makepad-toolkit-windows-gui'}],
 ['silent fallback',{automatic_legacy_fallback:true}],
 ['global input',{global_input_allowed:true}],
 ['foreign window IDs',{target_binding:'IMPORTED_FROM_OLD_SESSION'}],
 ['extra mutating tool',{allowed_tools:[...original.allowed_tools,'windows_type_text']}],
 ['duplicate tool',{allowed_tools:original.allowed_tools.map(()=>original.allowed_tools[0])}],
 ['missing tool',{allowed_tools:original.allowed_tools.slice(1)}],
 ['fake input acceptance',{mutating_gui_acceptance:'PASS'}],
 ['evidence-only acceptance',{evidence_call_is_gui_acceptance:true}]
])test('rejects '+name,()=>assert.throws(()=>validateGuiDevelopment({...original,...patch})));
