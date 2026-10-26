// Compatibility capture imports; configured CLI additionally binds v2 observations.
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {captureCompat,core,consumerRoot,configuredRoot,hasBinding} from './toolkit-verification-core.mjs';
export const {beginEnvelope,finishEnvelope}=captureCompat;
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
  try{const args=process.argv.slice(2);if(hasBinding(configuredRoot(args)))core.runCaptureCli(args,consumerRoot);else captureCompat.runEnvelopeCli(args,consumerRoot);}
  catch(e){console.error(e.message);process.exitCode=1;}
}
