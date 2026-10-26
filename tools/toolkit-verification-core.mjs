// Copy this adapter, not the implementation, into consumer tools/.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {fileURLToPath,pathToFileURL} from 'node:url';
export const consumerRoot=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const binding=JSON.parse(fs.readFileSync(path.join(consumerRoot,'verification/toolkit-core.json'),'utf8'));
if(binding.schema_version!==1 || binding.core_version!=='1.0.0')throw new Error('unsupported Toolkit core binding');
export const toolkitRoot=path.resolve(process.env.RUST_MAKEPAD_TOOLKIT_ROOT??path.resolve(consumerRoot,binding.toolkit_relative_root));
const coreRoot=path.join(toolkitRoot,'verification-economy');
const pkg=JSON.parse(fs.readFileSync(path.join(coreRoot,'package.json'),'utf8'));
if(pkg.name!=='@rust-makepad-toolkit/verification-economy'||pkg.version!==binding.core_version)throw new Error('Toolkit core version mismatch');
const modules=['package.json','core/global-policy.json','core/global-policy.mjs','core/verification-policy.mjs','core/verification-run-envelope.mjs','core/executable-provenance.mjs','core/index.mjs'];
if(!binding.module_hashes || Object.keys(binding.module_hashes).length!==modules.length)throw new Error('versioned core integrity pins missing');
for(const file of modules){const hash=crypto.createHash('sha256').update(fs.readFileSync(path.join(coreRoot,file))).digest('hex').toUpperCase();if(hash!==binding.module_hashes[file])throw new Error(`Toolkit module identity mismatch: ${file}; explicit dependency upgrade required`);}
export const compat=await import(pathToFileURL(path.join(coreRoot,'core/verification-policy.mjs')));
export const captureCompat=await import(pathToFileURL(path.join(coreRoot,'core/verification-run-envelope.mjs')));
export const core=await import(pathToFileURL(path.join(coreRoot,'core/index.mjs')));
export function configuredRoot(args){const n=args.indexOf('--root');return n<0?consumerRoot:path.resolve(args[n+1]);}
export function hasBinding(root){return fs.existsSync(path.join(root,'verification/toolkit-core.json'));}
