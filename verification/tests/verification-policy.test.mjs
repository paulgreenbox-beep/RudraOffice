import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {spawnSync} from 'node:child_process';
import {readJson, validatePolicy, planVerification, recordAcceptedMilestone, verifyAcceptanceProof, classifyObservation, LEVELS} from '../../tools/verification-policy.mjs';
import {beginEnvelope, finishEnvelope} from '../../tools/verification-run-envelope.mjs';

const root = fileURLToPath(new URL('../../', import.meta.url));
const original = readJson(path.join(root, 'verification/verification-policy.json'));
const sha = v => crypto.createHash('sha256').update(v).digest('hex').toUpperCase();
const fixedHash = 'A'.repeat(64);
// Synthetic KNOWN history for cadence tests; never substitutes the real legacy state.
function known(count = 0) {
  const p = structuredClone(original);
  p.milestone_history = [{milestone: 'SYNTHETIC-BASE', status: 'ACCEPTED', kind: 'functional', expanded_gui: true, artifact_hash: fixedHash, date: '2026-10-01'}];
  for (let n = 1; n <= count; n++) p.milestone_history.push({milestone: `SYNTHETIC-${n}`, status: 'ACCEPTED', kind: 'functional', expanded_gui: false, artifact_hash: fixedHash, date: '2026-10-02'});
  p.periodic_regression = {...p.periodic_regression, state: 'KNOWN', last_full_gui_milestone: 'SYNTHETIC-BASE', milestones_since_last_full: count};
  return p;
}
const input = (module, extra = {}) => ({milestone_id: 'SYNTHETIC-NEXT', changed_modules: [module], ...extra});
const pure = p => planVerification(p, input('sheets-formula-helper'));
const event = (plan, extra = {}) => ({milestone: plan.milestone, status: 'ACCEPTED', kind: 'functional', expanded_gui: plan.required.L4b.length > 0, artifact_hash: fixedHash, date: '2026-10-07', verified_proof: true, ...extra});

test('live manifest validates references; RC28 and unknown legacy state are not promoted', () => {
  assert.equal(validatePolicy(original, root).status, 'PASS');
  assert.equal(original.current_candidate.status, 'PARTIAL');
  assert.equal(original.current_candidate.counted, false);
  assert.equal(original.periodic_regression.state, 'UNKNOWN_LEGACY_BASELINE');
});
test('A pure formula helper: L0/L1, zero routine GUI below cadence', () => {
  const p = pure(known());
  assert.equal(p.status, 'PLANNED');
  assert.deepEqual(p.required.L0.map(t => t.id), ['formula-unit']);
  assert.deepEqual(p.required.L1.map(t => t.id), ['native-crate']);
  assert.equal(p.estimated_gui_runs, 0); assert.equal(p.l4b_required, false);
});
test('B grid widget: focused grid and representative workbook; immediate rendering L4b', () => {
  const p = planVerification(known(), input('sheets-grid-widget'));
  assert.ok(p.selected_scenario_ids.includes('grid-focused-runtime'));
  assert.ok(p.selected_scenario_ids.includes('sheets-real-copy-only'));
  assert.ok(p.required.L4b.length > 0); assert.ok(p.risk_triggers.includes('viewport-layout-rendering'));
});
test('C Windows clipboard retains native, rich/cross-book, focus, cut and real Copy-only', () => {
  const p = planVerification(known(), input('sheets-native-clipboard'));
  for (const id of ['clipboard-native-system','clipboard-rich-cross-book','clipboard-focus-routing','clipboard-deferred-cut','sheets-real-copy-only']) assert.ok(p.selected_scenario_ids.includes(id), id);
  assert.equal(p.l4b_required, true); assert.ok(p.required.L4b.length > 0);
});
test('D fifth accepted functional milestone forces composition even for pure helper', () => {
  const p = pure(known(4));
  assert.equal(p.periodic_counter.prospective_after_acceptance, 5);
  assert.equal(p.periodic_counter.due, true); assert.ok(p.required.L4b.length > 0);
  assert.equal(p.periodic_counter.advance_on_planning, false);
});
test('E shared runtime on milestone 2 requires both relevant compositions early', () => {
  const p = planVerification(known(1), input('shared-document-runtime'));
  assert.equal(p.periodic_counter.due, false); assert.equal(p.l4b_required, true);
  assert.ok(p.selected_scenario_ids.includes('expanded-office-sessions'));
  assert.ok(p.selected_scenario_ids.includes('expanded-sheets-composition'));
});
test('explicit shared-runtime risk selects Office composition even with pure helper tags', () => {
  const p=planVerification(known(),input('sheets-formula-helper',{risk_override:['shared-document-runtime']}));
  assert.ok(p.selected_scenario_ids.includes('expanded-office-sessions')); assert.equal(p.l4b_required,true);
});
test('flaky candidate escalates to risk composition without claiming PASS', () => {
  const p=known(); p.scenario_registry.find(s=>s.id==='pdf-parser-focused').stability='flaky_candidate';
  const plan=planVerification(p,input('pdf-parser')); assert.equal(plan.l4b_required,true);
  assert.ok(plan.risk_triggers.includes('unexplained-gui-flake'));
});
test('unknown legacy functional baseline forces calibration without inventing count', () => {
  const p = pure(original); assert.equal(p.l4b_required, true);
  assert.equal(p.periodic_counter.prospective_after_acceptance, null);
});
for (const kind of ['documentation','infrastructure']) test(`${kind} does not service functional cadence or count`, () => {
  const p = planVerification(original, input('verification-infrastructure', {kind}));
  assert.equal(p.l4b_required, false); assert.equal(p.estimated_gui_runs, 0);
  assert.equal(recordAcceptedMilestone(original, event(p, {kind}), p).changed, false);
});
test('RC28 optimized plan remains NOT_RUN and covers four goldens within coherent L4b', () => {
  const p = planVerification(original, readJson(path.join(root, 'verification/inputs/RC28-closure.json')));
  assert.equal(p.status, 'PLANNED'); assert.equal(p.estimated_gui_runs, 9);
  assert.equal(p.budget.status, 'JUSTIFIED_OVERRIDE'); assert.equal(p.required.L4a.length, 0);
  assert.deepEqual(p.budget.violations.map(v => v.key), ['focused_new_feature_gui']);
  for (const id of original.golden_scenarios) assert.ok(p.skipped_periodic_tests.some(s => s.id === id && s.covered_by === 'expanded-sheets-composition'));
  for (const l of ['L3','L4a','L4b','L5']) for (const s of p.required[l]) assert.equal(s.execution_status, 'NOT_RUN');
  assert.equal(p.execution.runs_gui, false); assert.equal(p.execution.updates_acceptance, false);
});
test('budget overflow needs explicit justification, never truncates required scenarios', () => {
  const i = readJson(path.join(root, 'verification/inputs/RC28-closure.json')); delete i.budget_override_reason;
  const p = planVerification(original, i); assert.equal(p.status, 'BUDGET_JUSTIFICATION_REQUIRED'); assert.equal(p.estimated_gui_runs, 9);
  assert.ok(p.required.L4b.length > 0);
});
test('unmapped input requires impact review; unknown tag and escaping path rejected', () => {
  assert.equal(planVerification(known(), {milestone_id:'X',changed_files:['new/unknown.rs']}).status, 'NEEDS_IMPACT_REVIEW');
  assert.throws(() => planVerification(known(), input('sheets-formula-helper',{feature_tags:['typo']})), /unknown feature/);
  assert.throws(() => planVerification(known(), {milestone_id:'X',changed_files:['../escape.rs']}), /relative/);
});
test('real PPTX parser cannot pretend a blank slideshow is parser acceptance', () => {
  const p = planVerification(known(), input('slides-parser'));
  assert.equal(p.status, 'NEEDS_FIXTURE_REVIEW'); assert.ok(p.fixture_review.includes('slides-parser-focused'));
});
test('quarantine blocks acceptance rather than removing the required scenario', () => {
  const p = known(); p.scenario_registry.find(s => s.id === 'grid-focused-runtime').stability = 'quarantined';
  const plan = planVerification(p, input('sheets-grid-widget'));
  assert.equal(plan.status, 'BLOCKED_TEST_INVESTIGATION'); assert.ok(plan.selected_scenario_ids.includes('grid-focused-runtime'));
});
const mutations = [
  ['duplicate scenario IDs', p => p.scenario_registry.push(structuredClone(p.scenario_registry[0]))],
  ['missing owner', p => p.scenario_registry[0].owner = ''],
  ['invalid periodic interval', p => p.periodic_regression.interval = 0],
  ['counter disagrees', p => p.periodic_regression.milestones_since_last_full = 3],
  ['invalid verification levels', p => delete p.levels.L4b],
  ['unknown lower-level', p => p.scenario_registry[0].lower_level_coverage = ['absent']],
  ['incomplete composition', p => p.scenario_registry.find(s => s.id === 'expanded-sheets-composition').features_proven = ['selection']],
  ['unknown risk tag', p => p.scenario_registry[0].risk_tags = ['typo']],
  ['invalid budget', p => p.budgets.focused_new_feature_gui = -1],
  ['invalid scenario level', p => p.scenario_registry[0].level = 'L9'],
  ['incomplete last PASS', p => p.scenario_registry[0].last_passed_artifact = fixedHash],
];
for (const [message, mutate] of mutations) test(`validator rejects ${message}`, () => {const p=known(); mutate(p); assert.throws(() => validatePolicy(p, root), new RegExp(message));});
test('validator rejects a missing scenario recipe', () => {const p=known(); p.scenario_registry[0].file='verification/no-such-script.aiui.json'; assert.throws(() => validatePolicy(p, root), /does not exist/);});
test('outcomes distinguish interference/infra/product/not-run; none becomes automatic PASS', () => {
  for (const key of ['foreground_lost','clipboard_interference','desktop_unavailable']) assert.equal(classifyObservation({[key]:true,postconditions_verified:true,runtime_healthy:true}), 'BLOCKED_ENVIRONMENT');
  for (const key of ['stale_uia_element','dpi_runner_mismatch','invalid_test_script']) assert.equal(classifyObservation({[key]:true}), 'FAIL_TEST_INFRA');
  assert.equal(classifyObservation({product_failure:true}), 'FAIL_PRODUCT');
  assert.equal(classifyObservation({}), 'NOT_RUN'); assert.equal(classifyObservation({not_run:true}), 'NOT_RUN');
  assert.equal(classifyObservation({postconditions_verified:true,runtime_healthy:true}), 'PASS');
});
test('PARTIAL and retry do not advance history; planning mutates no policy', () => {
  const p=known(), before=JSON.stringify(p), plan=pure(p);
  assert.equal(recordAcceptedMilestone(p, event(plan,{status:'PARTIAL'}), plan).changed,false);
  assert.equal(JSON.stringify(p),before);
});
test('accepted pure milestone increments exactly once; expanded risk resets early', () => {
  const p=known(1), plan=pure(p), next=recordAcceptedMilestone(p,event(plan),plan).policy;
  assert.equal(next.periodic_regression.milestones_since_last_full,2); assert.equal(p.periodic_regression.milestones_since_last_full,1);
  assert.throws(() => pure(next), /already recorded/);
  const risk=planVerification(p,input('shared-document-runtime'));
  const reset=recordAcceptedMilestone(p,event(risk),risk).policy;
  assert.equal(reset.periodic_regression.milestones_since_last_full,0); assert.equal(reset.periodic_regression.last_full_gui_milestone,risk.milestone);
  validatePolicy(reset,root);
});
test('required expanded and proof cannot be omitted; tampered plan cannot drop coverage', () => {
  const p=known(4), plan=pure(p);
  assert.throws(() => recordAcceptedMilestone(p,event(plan,{expanded_gui:false}),plan), /L4b not passed/);
  assert.throws(() => recordAcceptedMilestone(p,event(plan,{verified_proof:false}),plan), /verified acceptance/);
  const modified=structuredClone(plan); modified.required.L4b=[]; modified.selected_scenario_ids=[];
  assert.throws(() => recordAcceptedMilestone(p,event(modified),modified), /coverage removed/);
});
test('infrastructure plan cannot be relabeled as accepted functional milestone', () => {
  const p=known(4), plan=planVerification(p,input('verification-infrastructure',{kind:'infrastructure'}));
  assert.throws(()=>recordAcceptedMilestone(p,event(plan),plan),/functional plan/);
});

function proofFixture(t) {
  const dir=fs.mkdtempSync(path.join(os.tmpdir(),'rudra-policy-test-'));
  t.after(() => fs.rmSync(dir,{recursive:true,force:true})); // Only this exact newly-created test sandbox.
  const p=known(), plan=planVerification(p,input('sheets-formula-helper',{release_candidate:true}));
  const write=(file,value) => {const target=path.join(dir,file); fs.mkdirSync(path.dirname(target),{recursive:true}); fs.writeFileSync(target, typeof value==='string'?value:JSON.stringify(value));};
  write('artifact.exe','synthetic bytes - never executable'); const artifactHash=sha(fs.readFileSync(path.join(dir,'artifact.exe')));
  write('src/example.rs','synthetic source'); write('freeze.json',[{path:'src/example.rs',sha256:sha('synthetic source')}]);
  write('quality.json',{artifact:{sha256:artifactHash},source_freeze_sha256:sha(fs.readFileSync(path.join(dir,'freeze.json'))),completed_at:'2026-10-07T07:59:00Z',gates:LEVELS.slice(0,3).flatMap(l=>plan.required[l]).map(g=>({id:g.id,exit_code:0}))});
  const proofs=[];
  for (const id of plan.selected_scenario_ids) {
    const scenario=p.scenario_registry.find(s=>s.id===id), recipe=readJson(path.join(root,scenario.file)); recipe.application.command=path.join(dir,'artifact.exe'); write(scenario.file,recipe);
    const body=[...(recipe.setup??[]),...recipe.steps];
    const trace=[...body,...recipe.teardown].map((s,index)=>({index,step:s.step_id??(index<body.length?`step-${index}`:`teardown-${index-body.length}`),operation:s.op,status:'PASS',response:{result:s.op==='get_runtime_health'?{healthy:true,process_alive:true}:s.op==='get_runtime_stderr'?[]:true}}));
    const report={test:recipe.name,status:'PASSED',summary:{passed:trace.length,total:trace.length,failed:0,skipped:0},steps:trace};
    const file=`evidence/${id}.json`; write(file,report);
    const manifest=`evidence/${id}-run.json`;
    write(manifest,{status:'CAPTURED',milestone:plan.milestone,policy_digest:plan.policy_digest,scenario_id:id,artifact_before:artifactHash,artifact_after:artifactHash,source_freeze_before:sha(fs.readFileSync(path.join(dir,'freeze.json'))),source_freeze_after:sha(fs.readFileSync(path.join(dir,'freeze.json'))),recipe_sha256:sha(fs.readFileSync(path.join(dir,scenario.file))),report_sha256:sha(fs.readFileSync(path.join(dir,file))),started_at:scenario.level==='L5'?'2026-10-07T08:02:00Z':'2026-10-07T08:00:00Z',completed_at:scenario.level==='L5'?'2026-10-07T08:03:00Z':'2026-10-07T08:01:00Z'});
    proofs.push({id,status:'PASS',artifact_hash:artifactHash,report:file,run_manifest:manifest});
  }
  const e=event(plan,{artifact_hash:artifactHash,artifact_path:'artifact.exe',quality_report:'quality.json',source_freeze:'freeze.json',scenario_results:proofs});
  return {p,plan,e,dir,write};
}
test('proof gate accepts only actual frozen bytes, source and complete required raw reports', t => {
  const {p,plan,e,dir}=proofFixture(t); assert.equal(verifyAcceptanceProof(p,e,plan,dir).verified_proof,true);
});
test('proof gate rejects changed artifact, changed source, omitted gate and blocked GUI', t => {
  const f=proofFixture(t);
  assert.throws(() => verifyAcceptanceProof(f.p,{...f.e,artifact_hash:fixedHash},f.plan,f.dir),/artifact hash/);
  f.write('src/example.rs','changed'); assert.throws(() => verifyAcceptanceProof(f.p,f.e,f.plan,f.dir),/source changed/);
  f.write('src/example.rs','synthetic source'); f.write('quality.json',{artifact:{sha256:f.e.artifact_hash},gates:[{id:'unrelated',exit_code:0}]});
  assert.throws(() => verifyAcceptanceProof(f.p,f.e,f.plan,f.dir),/lower-level gate missing/);
});
test('incomplete, wrong-scenario, unhealthy and stderr GUI evidence is never PASS', t => {
  const f=proofFixture(t), proof=f.e.scenario_results[0], file=path.join(f.dir,proof.report), baseline=readJson(file);
  for (const mutate of [r=>r.summary.skipped=1,r=>r.test='different',r=>r.steps.find(s=>s.operation==='get_runtime_health').response.result.healthy=false,r=>r.steps.find(s=>s.operation==='get_runtime_stderr').response.result=['warning'],r=>{r.steps=r.steps.slice(-2);r.summary.passed=r.summary.total=2;}]) {
    const r=structuredClone(baseline); mutate(r); f.write(proof.report,r);
    assert.throws(() => verifyAcceptanceProof(f.p,f.e,f.plan,f.dir));
  }
  f.write(proof.report,baseline); proof.status='BLOCKED_ENVIRONMENT';
  assert.throws(() => verifyAcceptanceProof(f.p,f.e,f.plan,f.dir),/not PASS/);
});
test('old/misbound run envelope is rejected and full step sequence is mandatory', t => {
  const f=proofFixture(t), proof=f.e.scenario_results[0], file=path.join(f.dir,proof.run_manifest), run=readJson(file);
  f.write(proof.run_manifest,{...run,artifact_before:fixedHash});
  assert.throws(()=>verifyAcceptanceProof(f.p,f.e,f.plan,f.dir),/run artifact/);
  const report=readJson(path.join(f.dir,proof.report)); report.steps=report.steps.slice(-2);report.summary.passed=report.summary.total=2;f.write(proof.report,report);
  f.write(proof.run_manifest,{...run,report_sha256:sha(fs.readFileSync(path.join(f.dir,proof.report)))});
  assert.throws(()=>verifyAcceptanceProof(f.p,f.e,f.plan,f.dir),/recipe steps/);
});
test('CLI rejects unknown flag before writing plan', () => {
  const r=spawnSync(process.execPath,[path.join(root,'tools/verification-policy.mjs'),'validate','--typo','true'],{encoding:'utf8'});
  assert.equal(r.status,1); assert.match(r.stderr,/unknown command/);
});
test('identity capture never executes or accepts and refuses repeated finalization', t => {
  const f=proofFixture(t), proof=f.e.scenario_results[0], run=beginEnvelope(f.dir,f.p,f.plan,proof.id,'freeze.json');
  assert.equal(run.status,'RUNNING');assert.equal(run.gui_execution,'not performed by this tool');
  f.write(proof.report,readJson(path.join(f.dir,proof.report))); // Simulate producer output after begin; no GUI.
  const done=finishEnvelope(f.dir,run,proof.report);assert.equal(done.status,'CAPTURED');assert.equal(done.artifact_after,run.artifact_before);
  assert.throws(()=>finishEnvelope(f.dir,done,proof.report),/already finalized/);
});
test('identity capture refuses changed recipe and source', t => {
  const f=proofFixture(t), proof=f.e.scenario_results[0], run=beginEnvelope(f.dir,f.p,f.plan,proof.id,'freeze.json');
  f.write(run.recipe_path,'{}');assert.throws(()=>finishEnvelope(f.dir,run,proof.report),/recipe changed/);
  f.write('src/example.rs','changed');assert.throws(()=>finishEnvelope(f.dir,run,proof.report),/source freeze changed/);
});
test('identity capture refuses an old raw report instead of relabeling it as a fresh run', t => {
  const f=proofFixture(t), proof=f.e.scenario_results[0], run=beginEnvelope(f.dir,f.p,f.plan,proof.id,'freeze.json');
  fs.utimesSync(path.join(f.dir,proof.report),new Date('2020-01-01'),new Date('2020-01-01'));
  assert.throws(()=>finishEnvelope(f.dir,run,proof.report),/predates this run/);
});
test('smoke cannot predate quality and feature acceptance', t => {
  const f=proofFixture(t), smoke=f.e.scenario_results.find(s=>s.id==='frozen-artifact-smoke'), run=readJson(path.join(f.dir,smoke.run_manifest));
  f.write(smoke.run_manifest,{...run,started_at:'2026-10-07T07:58:00Z'});
  assert.throws(()=>verifyAcceptanceProof(f.p,f.e,f.plan,f.dir),/smoke must follow/);
});
test('quality must bind the same source freeze as GUI', t => {
  const f=proofFixture(t), q=readJson(path.join(f.dir,'quality.json'));q.source_freeze_sha256=fixedHash;f.write('quality.json',q);
  assert.throws(()=>verifyAcceptanceProof(f.p,f.e,f.plan,f.dir),/quality source/);
});
test('CLI accepted update persists counter/history once; duplicate replay is rejected', t => {
  const f=proofFixture(t);
  for(const s of f.p.scenario_registry) if(!fs.existsSync(path.join(f.dir,s.file))) f.write(s.file,readJson(path.join(root,s.file)));
  f.write('verification/verification-policy.json',f.p);f.write('verification/plan.json',f.plan);f.write('verification/event.json',f.e);
  const args=[path.join(root,'tools/verification-policy.mjs'),'record','--root',f.dir,'--acceptance',path.join(f.dir,'verification/event.json'),'--plan',path.join(f.dir,'verification/plan.json')];
  const result=spawnSync(process.execPath,args,{encoding:'utf8'});assert.equal(result.status,0,result.stderr);
  const updated=readJson(path.join(f.dir,'verification/verification-policy.json'));
  assert.equal(updated.periodic_regression.milestones_since_last_full,1);assert.equal(updated.milestone_history.at(-1).milestone,f.plan.milestone);
  assert.equal(updated.scenario_registry.find(s=>s.id==='frozen-artifact-smoke').last_passed_artifact,f.e.artifact_hash);
  const again=spawnSync(process.execPath,args,{encoding:'utf8'});assert.equal(again.status,1);
  assert.equal(readJson(path.join(f.dir,'verification/verification-policy.json')).milestone_history.length,updated.milestone_history.length);
  assert.equal(fs.existsSync(path.join(f.dir,'verification/verification-policy.json.acceptance.lock')),false);
});
