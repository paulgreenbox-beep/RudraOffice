// Legacy library exports stay compatible. Configured consumer CLI uses strict v2.
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {compat,core,consumerRoot,configuredRoot,hasBinding} from './toolkit-verification-core.mjs';
export const {LEVELS,STATUSES,OUTCOMES,readJson,validatePolicy,planVerification,classifyObservation,recordAcceptedMilestone,verifyAcceptanceProof}=compat;
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
  try{const args=process.argv.slice(2);if(hasBinding(configuredRoot(args)))core.runCli(args,consumerRoot);else compat.runLegacyCli(args,consumerRoot);}
  catch(e){console.error(e.message);process.exitCode=1;}
}
