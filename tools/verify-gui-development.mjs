// A project workflow contract validator, not a GUI runner, MCP client or proof producer.
import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
export function validateGuiDevelopment(value){
 const check=(condition,message)=>{if(!condition)throw Error(message);};
 const tools=['windows_get_control_tree','windows_get_element_state','windows_get_automation_id_state','windows_capture_window','windows_get_evidence'];
 check(value?.schema==='rudra-office-gui-development-v1','unsupported GUI development schema');
 check(value.connection==='toolkit-gui-remote-canary','legacy or arbitrary GUI connection refused');
 check(value.mode==='READ_ONLY_CANARY','mutable GUI rollout is not established');
 check(Array.isArray(value.allowed_tools)&&value.allowed_tools.length===tools.length&&new Set(value.allowed_tools).size===tools.length&&tools.every(t=>value.allowed_tools.includes(t)),'exact five-tool read-only contract required');
 check(value.target_binding==='WINDOW_ID_FROM_THIS_CANARY_OWNER_ONLY','cross-connection window IDs are not transferable');
 check(value.automatic_legacy_fallback===false,'automatic legacy fallback refused');
 check(value.global_input_allowed===false,'global input is not authorized by this profile');
 check(value.evidence_call_is_gui_acceptance===false,'evidence-only acknowledgement is not GUI acceptance');
 check(value.mutating_gui_acceptance==='BLOCKED_NOT_EXPOSED','unexposed input cannot pass acceptance');
 check(value.historical_recipes==='PRESERVE_WITHOUT_REPLAY_OR_PROMOTION','historical proof must be preserved');
 check(value.runtime_libraries==='EXISTING_CARGO_DEPENDENCIES_UNCHANGED','MCP workflow migration is not a Makepad SDK rewrite');
 return {status:'PROFILE_VALID',connection:value.connection,tools:tools.length,gui_accepted:false,mutating_gui_acceptance:value.mutating_gui_acceptance};
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 try{const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');console.log(JSON.stringify(validateGuiDevelopment(JSON.parse(fs.readFileSync(path.join(root,'verification/gui-development.json'),'utf8')))));}
 catch(error){console.error(error.message);process.exitCode=1;}
}
