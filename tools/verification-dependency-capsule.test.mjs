import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {inside, mapped, noLinks, transformManifest, graphSignature, createCapsule, verifyCapsule, hashFile} from './verification-dependency-capsule.mjs';
const root = path.resolve(os.tmpdir(), 'capsule-contract-only');
const output = path.join(root, 'Office', 'capsules', 'new');
test('path containment rejects sibling-prefix and traversal', () => {
  assert.equal(inside(root, root + '-other/a'), false);
  assert.equal(inside(root, path.join(root, '../outside')), false);
  assert.equal(inside(root, path.join(root, 'inside')), true);
});
test('mapping is deterministic and never relabels an outside input', () => {
  assert.equal(mapped(root, output, path.join(root, 'Toolkit/a')), path.join(output, 's/Toolkit/a'));
  assert.throws(() => mapped(root, output, path.join(root, '../a')), /unmapped/);
});
test('absolute Cargo dependency becomes a relative physical-copy reference', () => {
  const original = path.join(root, 'Office/Cargo.toml'), dependency = path.join(root, 'Toolkit/core');
  const result = transformManifest('[dependencies]\ncore={path="' + dependency.replaceAll('\\','/') + '"}\n', original, mapped(root, output, original), root, output, new Set([path.join(dependency,'Cargo.toml')]));
  assert.match(result.text, /path="\.\.\/Toolkit\/core"/);
  assert.equal(result.changes.length, 1);
});
test('external absolute dependency fails before staging', () => {
  const file = path.join(root,'Office/Cargo.toml');
  assert.throws(() => transformManifest('p={path="' + path.resolve(root,'../external').replaceAll('\\','/') + '"}', file, mapped(root,output,file), root,output,new Set()), /outside closure/);
});
test('uncopied absolute dependency cannot become a fake owned reference', () => {
  const file = path.join(root,'Office/Cargo.toml');
  assert.throws(() => transformManifest('p={path="' + path.join(root,'missing').replaceAll('\\','/') + '"}',file,mapped(root,output,file),root,output,new Set()), /not copied/);
});
test('workspace pruning preserves inherited fields and package member labels', () => {
  const file = path.join(root,'Toolkit/Cargo.toml');
  const text = '[workspace]\nmembers=["core", "absent"]\n[workspace.package]\nlicense="MIT"\n[package.metadata]\nmembers=["not-a-workspace-member"]\n';
  const result = transformManifest(text,file,mapped(root,output,file),root,output,new Set([path.join(root,'Toolkit/core/Cargo.toml')]));
  assert.match(result.text,/members = \["core"\]/);
  assert.match(result.text,/license="MIT"/);
  assert.match(result.text,/members=\["not-a-workspace-member"\]/);
});
test('dotted workspace and multiline comments are handled without guessing globs', () => {
  const file=path.join(root,'Toolkit/Cargo.toml');
  const result=transformManifest('workspace.members = [\n # comment\n "core",\n "absent"\n]\n',file,mapped(root,output,file),root,output,new Set([path.join(root,'Toolkit/core/Cargo.toml')]));
  assert.equal(result.text,'workspace.members = ["core"]\n');
  assert.throws(()=>transformManifest('workspace.members=["crates/*"]',file,mapped(root,output,file),root,output,new Set()),/glob requires explicit review/);
});
test('graph equivalence includes features, edge kinds and registry identity', () => {
  const metadata={packages:[{id:'a',name:'a',version:'1',source:null,manifest_path:'/old/a/Cargo.toml'},{id:'b',name:'b',version:'2',source:'registry+fixed',manifest_path:'/registry/b'}],resolve:{root:'a',nodes:[{id:'a',features:['z','a'],deps:[{name:'b',pkg:'b',dep_kinds:[{kind:null,target:null}]}]},{id:'b',features:[],deps:[]}]}};
  const other=structuredClone(metadata);other.packages[0].manifest_path='/new/a/Cargo.toml';
  assert.deepEqual(graphSignature(metadata),graphSignature(other,p=>p.replace('/new/','/old/')));
  other.resolve.nodes[0].features.push('unexpected');
  assert.notDeepEqual(graphSignature(metadata),graphSignature(other,p=>p.replace('/new/','/old/')));
});
test('graph equivalence includes source targets, declarations and workspace selection', () => {
  const value={packages:[{id:'a',name:'a',version:'1',source:null,manifest_path:'/old/a/Cargo.toml',targets:[{name:'a',src_path:'/old/a/lib.rs',kind:['lib']}],dependencies:[{name:'optional',optional:true,features:['flag']}]}],resolve:{root:'a',nodes:[{id:'a',features:[],deps:[]}]},workspace_members:['a'],workspace_default_members:['a'],workspace_root:'/old/a'};
  for(const mutate of [v=>v.packages[0].targets[0].src_path='/old/evil.rs',v=>v.packages[0].dependencies[0].optional=false,v=>v.workspace_default_members=[]]) {
    const changed=structuredClone(value);mutate(changed);assert.notDeepEqual(graphSignature(value),graphSignature(changed));
  }
});
test('actual copy is write-exclusive, hash-verified, and independent of later original mutation', () => {
  const temp=fs.mkdtempSync(path.join(os.tmpdir(),'office-capsule-test-'));
  const office=path.join(temp,'Office'), pkg=path.join(office,'pkg');fs.mkdirSync(pkg,{recursive:true});
  const manifest=path.join(pkg,'Cargo.toml');fs.writeFileSync(manifest,'[package]\nname="example"\nversion="1.0.0"\n');
  const source=path.join(pkg,'lib.rs');fs.writeFileSync(source,'pub fn value()->u8{1}');
  const meta=path.join(temp,'metadata.json'),baseline=path.join(temp,'freeze.json');
  fs.writeFileSync(meta,JSON.stringify({packages:[{name:'example',version:'1.0.0',id:'a',source:null,manifest_path:manifest,targets:[]}],resolve:{root:'a',nodes:[]}}));
  fs.writeFileSync(baseline,JSON.stringify([{path:source,sha256:hashFile(source)}]));
  const config={sourceRoot:temp,officeRoot:office,metadataFile:meta,baselineFreeze:baseline,outputRoot:path.join(office,'capsule')};
  assert.equal(createCapsule(config).files,2);
  fs.writeFileSync(source,'changed original');
  assert.equal(verifyCapsule(config.outputRoot,office).files,2);
  assert.throws(()=>createCapsule(config),/already exists/);
  const injected=path.join(config.outputRoot,'s/unmanifested.rs');fs.writeFileSync(injected,'extra');
  assert.throws(()=>verifyCapsule(config.outputRoot,office),/unmanifested/);
  // This exact file was created by this test; keep it outside the immutable sources.
  fs.renameSync(injected,path.join(temp,'unmanifested-forensics.rs'));
  fs.appendFileSync(mapped(temp,config.outputRoot,source),'corrupt');
  assert.throws(()=>verifyCapsule(config.outputRoot,office),/source drift/);
  // Test-owned temporary files retained for forensic inspection; no broad cleanup.
});
test('source link component is rejected', () => {
  const temp=fs.mkdtempSync(path.join(os.tmpdir(),'office-capsule-link-test-'));
  const actual=path.join(temp,'actual');fs.mkdirSync(actual);
  fs.symlinkSync(actual,path.join(temp,'linked'),process.platform==='win32'?'junction':'dir');
  assert.throws(()=>noLinks(temp,path.join(temp,'linked')),/linked source component/);
  assert.doesNotThrow(()=>noLinks(temp,path.join(temp,'linked'),[{path:path.join(temp,'linked'),target:actual}]));
  assert.throws(()=>noLinks(temp,path.join(temp,'linked'),[{path:path.join(temp,'linked'),target:temp}]),/linked source component/);
});
