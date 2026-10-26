// Consumer-owned validation adapter for the EXISTING native observer.
// No GUI operations, shell, expectation-to-actual conversion or acceptance writes.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {core} from './toolkit-verification-core.mjs';

const check=(ok,message)=>{if(!ok)throw Error('OFFICE_OBSERVER: '+message);};
const read=file=>JSON.parse(fs.readFileSync(file,'utf8').replace(/^\uFEFF/,''));
export const hash=file=>crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex').toUpperCase();
export function owned(root,ref){
 check(typeof ref==='string'&&ref.length>0,'missing path');
 const base=path.resolve(root),file=path.resolve(base,ref),relative=path.relative(base,file);
 check(relative!==''&&relative!=='..'&&!relative.startsWith('..'+path.sep)&&!path.isAbsolute(relative),'path escaped consumer');
 let ancestor=file;while(!fs.existsSync(ancestor)){const next=path.dirname(ancestor);check(next!==ancestor,'missing ancestor');ancestor=next;}
 const realRelative=path.relative(fs.realpathSync.native(base),fs.realpathSync.native(ancestor));
 check(realRelative!=='..'&&!realRelative.startsWith('..'+path.sep)&&!path.isAbsolute(realRelative),'linked path escaped consumer');
 return file;
}

export function verifyFrozenInputs(root,frozen){
 check(frozen.schema_version===2,'frozen input schema');
 const source=owned(root,frozen.source_freeze_path);
 check(hash(source)===frozen.source_digest,'source inventory changed');
 const entries=read(source);check(Array.isArray(entries)&&entries.length>0&&entries.length<=60000,'source inventory bounds');
 const seen=new Set();
 for(const entry of entries){const file=owned(root,entry.path);check(!seen.has(file),'duplicate source');seen.add(file);check(hash(file)===entry.sha256,'frozen source bytes changed: '+entry.path);}
 for(const name of ['plan','recipe','helper','readback_specs','artifact','runner']){
  check(hash(owned(root,frozen[name+'_path']))===frozen[name+'_sha256'],'frozen '+name+' changed');
 }
 const specs=owned(root,frozen.readback_specs_path);
 check(fs.statSync(specs).size<=1024*1024,'readback specs exceed bound');
 return entries.length;
}

export function execute(command,contextFile,ref){
 check(['prepare','fingerprint','validate'].includes(command),'unknown fixed helper command');
 const context=read(contextFile),root=path.resolve(context.consumer_root);
 check(context.schema_version===2&&context.producer_kind==='native-runtime-observer','trusted context schema');
 check(/^[a-zA-Z0-9_-]+$/.test(context.scenario_id),'invalid scenario ID');
 const expectedContext=owned(root,'verification/runs/'+context.scenario_id+'/producer-context.json');
 check(fs.realpathSync.native(contextFile)===fs.realpathSync.native(expectedContext),'context location mismatch');
 const frozenPath=owned(root,context.frozen_inputs_path);
 check(hash(frozenPath)===context.frozen_inputs_sha256,'frozen manifest changed');
 const frozen=read(frozenPath),run=read(owned(root,'verification/runs/'+context.scenario_id+'/begin.json'));
 check(context.run_id===run.run_id&&context.scenario_id===run.scenario_id&&context.plan_digest===run.plan_digest,'native run/context mismatch');
 check(context.artifact_sha256===run.artifact_before&&context.source_digest===frozen.source_digest&&run.source_freeze_before===frozen.source_digest,'artifact/source binding mismatch');
 check(owned(root,context.helper_path)===owned(root,frozen.helper_path)&&hash(owned(root,context.helper_path))===frozen.helper_sha256,'helper identity mismatch');
 if(command==='fingerprint')return {sha256:hash(owned(root,ref))};
 verifyFrozenInputs(root,frozen);
 const policy=core.loadProject(root),plan=read(owned(root,frozen.plan_path));
 core.validatePlan(policy,plan);
 check(policy.project_id===context.project_id&&plan.plan_digest===context.plan_digest,'project/plan mismatch');
 const scenario=policy.scenario_registry.find(s=>s.id===context.scenario_id);
 check(scenario&&plan.selected_scenario_ids.includes(context.scenario_id),'scenario not selected');
 const recipePath=owned(root,scenario.file),recipe=read(recipePath);
 check(recipePath===owned(root,frozen.recipe_path)&&hash(recipePath)===run.recipe_sha256,'recipe identity mismatch');
 check(owned(root,recipe.application.command)===owned(root,run.artifact_path)&&owned(root,run.artifact_path)===owned(root,frozen.artifact_path),'recipe/artifact mismatch');
 const steps=[...(recipe.setup??[]),...recipe.steps,...(recipe.teardown??[])];
 const expected_states=steps.map((step,index)=>core.expectedPostcondition(scenario,step,index));
 // The native observer parses the pinned specs with its strict, bounded schema.
 // This helper NEVER constructs actual state from these expected postconditions.
 const readback_specs=read(owned(root,frozen.readback_specs_path));
 check(Array.isArray(readback_specs),'readback spec list required');
 return {source_digest:frozen.source_digest,artifact_sha256:hash(owned(root,run.artifact_path)),recipe,expected_states,readback_specs};
}

if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 try{
  const [command,contextFlag,contextFile,pathFlag,ref]=process.argv.slice(2);
  check(contextFlag==='--context','fixed context argument required');
  check(command==='fingerprint'?pathFlag==='--path'&&ref&&process.argv.length===7:process.argv.length===5,'invalid fixed helper arguments');
  console.log(JSON.stringify(execute(command,contextFile,ref)));
 }catch(error){console.error(error.message);process.exitCode=1;}
}
