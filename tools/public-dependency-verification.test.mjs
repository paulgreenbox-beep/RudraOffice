import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {verifyPublicDependencies, bootstrapPublicDependencies, inspectRepository} from './public-dependency-verification.mjs';

// Synthetic dependency metadata only: not public-build or GUI acceptance.
const modules = ['package.json', 'core/global-policy.json', 'core/global-policy.mjs',
  'core/verification-policy.mjs', 'core/verification-run-envelope.mjs',
  'core/executable-provenance.mjs', 'core/index.mjs'];
const hash = text => crypto.createHash('sha256').update(text).digest('hex').toUpperCase();
const write = (p, text) => {fs.mkdirSync(path.dirname(p), {recursive:true});fs.writeFileSync(p,text);};
function fixture(t) {
  const base=fs.mkdtempSync(path.join(os.tmpdir(),'rudra-public-dependencies-'));
  const root=path.join(base,'RudraOffice'), toolkit=path.join(base,'Rust_Makepad_Toolkit'), makepad=path.join(toolkit,'.deps/makepad');
  t.after(()=>{
    assert.equal(fs.realpathSync(path.dirname(base)),fs.realpathSync(os.tmpdir()));
    assert.ok(path.basename(base).startsWith('rudra-public-dependencies-'));
    fs.rmSync(base,{recursive:true,force:true});
  });
  const config={schema_version:1,status:'reviewed',
    toolkit:{repository:'https://github.com/paulgreenbox-beep/rust-makepad-toolkit.git',reviewed_revision:'a'.repeat(40),required_files:['crates/rudra-runtime/Cargo.toml']},
    makepad:{repository:'https://github.com/makepad/makepad.git',reviewed_revision:'b'.repeat(40),relative_root:'.deps/makepad',required_files:['widgets/Cargo.toml']}};
  const binding={schema_version:1,core_version:'1.0.0',toolkit_relative_root:'../Rust_Makepad_Toolkit',module_hashes:{}};
  for(const name of modules){
    const value=name==='package.json'?JSON.stringify({name:'@rust-makepad-toolkit/verification-economy',version:'1.0.0'}):'synthetic: '+name;
    binding.module_hashes[name]=hash(value);write(path.join(toolkit,'verification-economy',name),value);
  }
  write(path.join(toolkit,'crates/rudra-runtime/Cargo.toml'),'[package]\nname="rudra-runtime"\n');
  write(path.join(makepad,'widgets/Cargo.toml'),'[package]\nname="makepad-widgets"\nversion="2.0.0"\n');
  const save=()=>{write(path.join(root,'verification/public-dependencies.json'),JSON.stringify(config));write(path.join(root,'verification/toolkit-core.json'),JSON.stringify(binding));};
  save();const reads=[];
  const run=(cwd,args)=>{
    reads.push({cwd,args});const key=args.join(' '), expected=cwd===toolkit?config.toolkit:config.makepad;
    if(key==='rev-parse --show-toplevel')return{ok:true,text:cwd};
    if(key==='rev-parse HEAD')return{ok:true,text:expected.reviewed_revision};
    if(args[0]==='status')return{ok:true,text:''};
    if(args[0]==='ls-files')return{ok:true,text:'H Cargo.toml\0'};
    if(key==='remote get-url origin')return{ok:true,text:expected.repository};
    throw Error('Unexpected operation in read-only test: '+key);
  };
  return{base,root,toolkit,makepad,config,binding,save,run,reads};
}
const codes=result=>result.issues.map(issue=>issue.code);
test('reviewed exact metadata passes source admission, not build/GUI',t=>{
  const f=fixture(t),r=verifyPublicDependencies(f.root,{run:f.run});
  assert.equal(r.status,'SOURCE_DEPENDENCIES_VERIFIED_ONLY');assert.deepEqual(r.issues,[]);
  assert.equal(r.windows_build_verified,false);assert.equal(r.gui_verified,false);assert.equal(r.runs_network,false);
});
test('pending owner review cannot produce a passing baseline',t=>{
  const f=fixture(t);f.config.status='pending_owner_review';f.config.makepad.reviewed_revision=null;f.save();
  assert.ok(codes(verifyPublicDependencies(f.root,{run:f.run})).includes('OWNER_REVIEW_PENDING'));
});
test('unreviewed pins refuse bootstrap before reads, writes or network',t=>{
  const f=fixture(t);f.config.toolkit.reviewed_revision=null;f.save();let clones=0;
  assert.throws(()=>bootstrapPublicDependencies(f.root,{run:f.run,clone:()=>clones++}),/OWNER_REVIEW_PENDING/);
  assert.equal(clones,0);assert.equal(f.reads.length,0);
});
test('missing pinned Core file blocks admission',t=>{
  const f=fixture(t);fs.unlinkSync(path.join(f.toolkit,'verification-economy/core/index.mjs'));
  assert.ok(codes(verifyPublicDependencies(f.root,{run:f.run})).includes('MISSING_PINNED_MODULE'));
});
test('mismatched Core bytes are not silently re-pinned',t=>{
  const f=fixture(t);write(path.join(f.toolkit,'verification-economy/core/index.mjs'),'changed');
  const pin=f.binding.module_hashes['core/index.mjs'];
  assert.ok(codes(verifyPublicDependencies(f.root,{run:f.run})).includes('MODULE_HASH_MISMATCH'));
  assert.equal(JSON.parse(fs.readFileSync(path.join(f.root,'verification/toolkit-core.json'))).module_hashes['core/index.mjs'],pin);
});
test('enclosing repository identity is not attributed to Makepad',t=>{
  const f=fixture(t),run=(cwd,args)=>cwd===f.makepad&&args[1]==='--show-toplevel'?{ok:true,text:f.toolkit}:f.run(cwd,args);
  const r=verifyPublicDependencies(f.root,{run});assert.ok(codes(r).includes('NOT_INDEPENDENT_GIT_ROOT'));assert.equal(r.makepad.actual_revision,undefined);
});
test('actual Git nested-directory root is rejected before reading foreign HEAD',t=>{
  const f=fixture(t),init=spawnSync('git',['init','-q',f.toolkit],{encoding:'utf8',windowsHide:true});
  assert.equal(init.status,0,'Git required for root-boundary unit');
  const r=inspectRepository(f.makepad,f.config.makepad);assert.deepEqual(r.issues,['NOT_INDEPENDENT_GIT_ROOT']);assert.equal(r.actual_revision,undefined);
});
test('a different commit cannot satisfy a reviewed revision',t=>{
  const f=fixture(t),run=(cwd,args)=>args.join(' ')==='rev-parse HEAD'?{ok:true,text:'c'.repeat(40)}:f.run(cwd,args);
  assert.ok(codes(verifyPublicDependencies(f.root,{run})).includes('REVISION_MISMATCH'));
});
test('dirty source trees are refused',t=>{
  const f=fixture(t),run=(cwd,args)=>args[0]==='status'?{ok:true,text:' M src/lib.rs'}:f.run(cwd,args);
  assert.ok(codes(verifyPublicDependencies(f.root,{run})).includes('DIRTY_OR_UNREADABLE_TREE'));
});
test('bootstrap preserves an existing dirty dependency without cloning',t=>{
  const f=fixture(t),run=(cwd,args)=>args[0]==='status'?{ok:true,text:'?? draft.rs'}:f.run(cwd,args);let clones=0;
  assert.throws(()=>bootstrapPublicDependencies(f.root,{run,clone:()=>clones++}),/DIRTY_OR_UNREADABLE_TREE/);assert.equal(clones,0);
});
test('Core rejection occurs before any SDK download',t=>{
  const f=fixture(t);fs.unlinkSync(path.join(f.toolkit,'verification-economy/core/index.mjs'));let clones=0;
  assert.throws(()=>bootstrapPublicDependencies(f.root,{run:f.run,clone:()=>clones++}),/TOOLKIT_OR_CORE_REJECTED/);assert.equal(clones,0);
});
test('credential-bearing remote is rejected without diagnostic leakage',t=>{
  const f=fixture(t),run=(cwd,args)=>args[0]==='remote'?{ok:true,text:'https://synthetic-value@github.com/makepad/makepad.git'}:f.run(cwd,args);
  const r=verifyPublicDependencies(f.root,{run});assert.ok(codes(r).includes('ORIGIN_MISMATCH'));assert.ok(!JSON.stringify(r).includes('synthetic-value'));
});
test('SSH origin is accepted for the same GitHub repository',t=>{
  const f=fixture(t),run=(cwd,args)=>args[0]==='remote'?{ok:true,text:(cwd===f.toolkit?f.config.toolkit.repository:f.config.makepad.repository).replace('https://github.com/','git@github.com:')}:f.run(cwd,args);
  assert.deepEqual(verifyPublicDependencies(f.root,{run}).issues,[]);
});
test('matching package version does not prove SDK identity',t=>{
  const f=fixture(t),run=(cwd,args)=>cwd===f.makepad&&args[0]==='rev-parse'?{ok:false,text:''}:f.run(cwd,args);
  assert.ok(codes(verifyPublicDependencies(f.root,{run})).includes('NOT_INDEPENDENT_GIT_ROOT'));
});
test('empty integrity bindings never authorize a source PASS',t=>{
  const f=fixture(t);f.binding.module_hashes={};f.save();assert.throws(()=>verifyPublicDependencies(f.root,{run:f.run}),/INVALID_CORE_INTEGRITY_PINS/);
});
test('required file paths cannot escape the declared repository',t=>{
  const f=fixture(t);f.config.toolkit.required_files=['../private.key'];f.save();assert.throws(()=>verifyPublicDependencies(f.root,{run:f.run}),/PATH_ESCAPES_ROOT/);
});
test('missing required manifests produce a precise controlled issue',t=>{
  const f=fixture(t);f.config.toolkit.required_files.push('crates/missing/Cargo.toml');f.save();
  assert.ok(verifyPublicDependencies(f.root,{run:f.run}).issues.some(i=>i.code==='MISSING_REQUIRED_FILE'&&i.file==='crates/missing/Cargo.toml'));
});
test('CLI rejects unknown arguments and missing root values',()=>{
  const script=fileURLToPath(new URL('./public-dependency-verification.mjs',import.meta.url));
  for(const args of [['--unknown'],['--root'],['--root','--bootstrap']]){
    const r=spawnSync(process.execPath,[script,...args],{encoding:'utf8',windowsHide:true});assert.equal(r.status,2);assert.equal(JSON.parse(r.stderr).error,'INVALID_ARGUMENTS');
  }
});
test('assume-unchanged cannot hide a modified tracked dependency file',t=>{
  const f=fixture(t),repo=path.join(f.base,'masked-repo');fs.mkdirSync(repo);
  const git=(...args)=>{const r=spawnSync('git',['-C',repo,...args],{encoding:'utf8',windowsHide:true});assert.equal(r.status,0,r.stderr);return r.stdout.trim();};
  git('init','-q');write(path.join(repo,'Cargo.toml'),'before\n');git('add','Cargo.toml');
  git('-c','user.name=Synthetic Test','-c','user.email=synthetic@example.invalid','commit','-qm','Synthetic fixture');
  git('remote','add','origin',f.config.makepad.repository);const head=git('rev-parse','HEAD');
  git('update-index','--assume-unchanged','Cargo.toml');write(path.join(repo,'Cargo.toml'),'after\n');
  assert.equal(git('status','--porcelain=v1'),'','establish the actual status blind spot');
  const r=inspectRepository(repo,{repository:f.config.makepad.repository,reviewed_revision:head});
  assert.ok(r.issues.includes('FORBIDDEN_OR_UNREADABLE_INDEX_FLAGS'));assert.equal(r.clean,false);
});
test('skip-worktree flags also fail admission',t=>{
  const f=fixture(t),run=(cwd,args)=>args[0]==='ls-files'?{ok:true,text:'S Cargo.toml\0'}:f.run(cwd,args);
  assert.ok(codes(verifyPublicDependencies(f.root,{run})).includes('FORBIDDEN_OR_UNREADABLE_INDEX_FLAGS'));
});
test('SDK parent junction outside Toolkit is rejected before bootstrap cloning',t=>{
  const f=fixture(t),old=path.join(f.toolkit,'.deps'),external=path.join(f.base,'external-deps');
  assert.ok(old.startsWith(f.base+path.sep)&&external.startsWith(f.base+path.sep));fs.renameSync(old,external);
  fs.symlinkSync(external,old,process.platform==='win32'?'junction':'dir');let clones=0;
  assert.throws(()=>verifyPublicDependencies(f.root,{run:f.run}),/PHYSICAL_PATH_ESCAPES_ROOT/);
  assert.throws(()=>bootstrapPublicDependencies(f.root,{run:f.run,clone:()=>clones++}),/PHYSICAL_PATH_ESCAPES_ROOT/);assert.equal(clones,0);
});
test('Toolkit checkout link outside sibling parent is rejected',t=>{
  const f=fixture(t),other=fixture(t),renamed=path.join(f.base,'original-toolkit');
  assert.ok(renamed.startsWith(f.base+path.sep));fs.renameSync(f.toolkit,renamed);
  fs.symlinkSync(other.toolkit,f.toolkit,process.platform==='win32'?'junction':'dir');
  assert.throws(()=>verifyPublicDependencies(f.root,{run:f.run}),/PHYSICAL_PATH_ESCAPES_ROOT/);
});
test('drive-relative and UNC required-file paths are rejected portably',t=>{
  const f=fixture(t);
  for(const value of ['D:somewhere','C:private.key','\\\\server\\share\\file']){
    f.config.toolkit.required_files=[value];f.save();assert.throws(()=>verifyPublicDependencies(f.root,{run:f.run}),/INVALID_RELATIVE_PATH/);
  }
});
test('pinned Core bytes in an external junction are not Toolkit-owned source',t=>{
  const f=fixture(t),core=path.join(f.toolkit,'verification-economy'),external=path.join(f.base,'external-core');
  assert.ok(core.startsWith(f.base+path.sep)&&external.startsWith(f.base+path.sep));fs.renameSync(core,external);
  fs.symlinkSync(external,core,process.platform==='win32'?'junction':'dir');
  assert.throws(()=>verifyPublicDependencies(f.root,{run:f.run}),/PHYSICAL_PATH_ESCAPES_ROOT/);
});
