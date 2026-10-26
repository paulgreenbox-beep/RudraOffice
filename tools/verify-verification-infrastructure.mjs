// Cheap policy/recipe/instruction validation only. Never executes GUI or Cargo.
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import {toolkitRoot as toolkit} from './toolkit-verification-core.mjs';
import crypto from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {spawnSync} from 'node:child_process';
import {readJson, validatePolicy, planVerification} from './verification-policy.mjs';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const checks=[];
const sha=file=>crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex').toUpperCase();
const check=(id,fn)=>{try{checks.push({id,status:'PASS',result:fn()});}catch(e){checks.push({id,status:'FAIL_TEST_INFRA',cause:e.message});}};
function command(exe,args){const r=spawnSync(exe,args,{cwd:root,encoding:'utf8',timeout:60000});if(r.status!==0)throw new Error((r.stderr||r.stdout||r.error?.message||`exit ${r.status}`).slice(0,3000));return r.stdout.trim();}
check('policy-node-tests',()=>command(process.execPath,['--test','verification/tests/verification-policy.test.mjs']));
const policy=readJson(path.join(root,'verification/verification-policy.json'));
check('preserve-existing-recipes',()=>policy.scenario_registry.map(s=>({id:s.id,sha256:sha(path.join(root,s.file))})));
check('manifest-semantic-validator',()=>validatePolicy(policy,root));
const runner=path.join(root,'reports/evidence/sheets-sort-autofilter-20261006/dpi-aware-public-runner/ai-ui-test.exe');
check('public-runner-provenance',()=>{const actual=sha(runner);if(actual!=='28784B57729A57E9E63201D232209C97D5C1EA92C90828C470A61234C9E6DE25')throw new Error('runner hash changed');return actual;});
if(checks.at(-1).status==='PASS')for(const s of policy.scenario_registry)check(`public-validate:${s.id}`,()=>command(runner,['validate',path.join(root,s.file)]));

const codex=process.env.CODEX_HOME??path.join(os.homedir(),'.codex');
const skillPairs=[
  [path.join(toolkit,'skills/rust-makepad-toolkit/SKILL.md'),path.join(codex,'skills/rust-makepad-toolkit/SKILL.md')],
  [path.join(toolkit,'plugins/rust-makepad-toolkit/skills/rust-makepad-windows-gui/SKILL.md'),path.join(codex,'plugins/cache/personal/rust-makepad-toolkit/0.1.0/skills/rust-makepad-windows-gui/SKILL.md')],
];
for(const [source,installed] of skillPairs){
  check(`installed-mirror:${path.basename(path.dirname(source))}`,()=>{if(sha(source)!==sha(installed))throw new Error('source and installed skill diverge');return {source,installed,sha256:sha(source)};});
  for(const file of [source,installed])check(`skill-instructions:${file}`,()=>{
    const text=fs.readFileSync(file,'utf8'), front=text.match(/^---\r?\n([\s\S]+?)\r?\n---\r?\n/);
    if(!front||!/^name: [a-z0-9-]+$/m.test(front[1])||!/^description: .+$/m.test(front[1]))throw new Error('invalid simple name/description frontmatter');
    const keys=front[1].split(/\r?\n/).map(l=>l.split(':')[0]);if(keys.some(k=>!['name','description'].includes(k)))throw new Error('unexpected skill frontmatter key');
    for(const term of ['Verification Economy Policy','verification/verification-policy.json','L4b','fifth accepted functional milestone','BLOCKED_ENVIRONMENT'])if(!text.includes(term))throw new Error(`missing instruction: ${term}`);
    if(/always (run|replay) (all|every).*historical/i.test(text))throw new Error('obsolete unconditional historical replay');
    return {frontmatter:'PASS (simple supported YAML subset)',policy_routing:'PASS'};
  });
}
check('cheap-compile-consumer-precedence',()=>{const text=fs.readFileSync(path.join(toolkit,'skills/rust-cheap-compile/SKILL.md'),'utf8');if(!text.includes('verification/verification-policy.json')||!text.includes('A cheap unit proof does not cancel periodic/risk GUI'))throw new Error('consumer precedence missing');return 'PASS';});
check('powershell-entrypoints',()=>command('powershell.exe',['-NoProfile','-Command',"$files=@('tools/plan-verification.ps1','tools/validate-verification-policy.ps1','tools/update-verification-state.ps1'); foreach($file in $files){$tokens=$null;$parseErrors=$null;[System.Management.Automation.Language.Parser]::ParseFile((Join-Path (Get-Location) $file),[ref]$tokens,[ref]$parseErrors)|Out-Null;if($parseErrors.Count){throw ($parseErrors|Out-String)}}; & './tools/validate-verification-policy.ps1'"]));
check('RC28-planner-wrapper',()=>command('powershell.exe',['-NoProfile','-File','tools/plan-verification.ps1','-InputFile','verification/inputs/RC28-closure.json','-OutputFile',path.join(root,'verification/plans/toolkit-rc28-preview.json')]));
check('RC28-current-plan',()=>command(process.execPath,['tools/verification-policy.mjs','plan','--input','verification/inputs/RC28-closure.json','--output','verification/plans/toolkit-rc28-preview.json']));
check('infrastructure-own-plan',()=>{
  const plan=planVerification(policy,{milestone_id:'VERIFICATION-ECONOMY-20261007',kind:'infrastructure',changed_modules:['verification-infrastructure','verification-skill']});
  if(plan.estimated_gui_runs!==0||plan.l4b_required)throw new Error('infra-only change unexpectedly requires GUI');
  const output=path.join(root,'verification/plans/toolkit-core-infrastructure.json');fs.writeFileSync(output,JSON.stringify(plan,null,2)+'\n');return {status:plan.status,estimated_gui_runs:plan.estimated_gui_runs,output};
});
for(const name of ['production','protected'])check(`unchanged:${name}`,()=>{
  const before=readJson(path.join(root,`verification/${name}-before.json`));
  const changes=before.filter(f=>!fs.existsSync(f.path)||sha(f.path)!==f.sha256);
  if(changes.length)throw new Error(JSON.stringify(changes));return {files_checked:before.length,changes:0};
});
const report={schema_version:1,date:new Date().toISOString(),status:checks.every(c=>c.status==='PASS')?'PASS':'FAIL_TEST_INFRA',checks,
  execution:{gui_runs:0,office_builds:0,cargo_runs:0,clipboard_access:false,acceptance_counter_updated:false},
  standard_skill_validator:{status:'BLOCKED_ENVIRONMENT',cause:'bundled Python lacks PyYAML; py registry points to absent C:/Python314/python.exe; no package installed',fallback:'simple name/description frontmatter + policy instructions + exact source/installed hashes + fresh worker self-check separately'},
  rc28_status:'PARTIAL'};
const output=path.join(root,'verification/validation/toolkit-core-infrastructure-checks.json');fs.mkdirSync(path.dirname(output),{recursive:true});fs.writeFileSync(output,JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify({status:report.status,checks:checks.length,failed:checks.filter(c=>c.status!=='PASS'),output,gui_runs:0}));
if(report.status!=='PASS')process.exitCode=1;
