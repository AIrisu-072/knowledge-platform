import {mkdir,lstat,realpath,open} from 'node:fs/promises';
import {join} from 'node:path';
import {MAX_RECEIPT_BYTES,projectQualificationReceipt} from './receipt.mjs';

/** One fixed small-stage CI artifact. Never writes the private source report. */
export async function writeSmallReceipt(root,report,expectedCode){
 if(typeof expectedCode!=='string'||!/^[a-f0-9]{40}$/.test(expectedCode)||report?.fingerprint?.code!==expectedCode)throw Error('Receipt source code mismatch');
 const envelope=projectQualificationReceipt(report);
 const bytes=Buffer.from(JSON.stringify(envelope)+'\n');
 if(bytes.length>MAX_RECEIPT_BYTES)throw Error('Receipt byte limit exceeded');
 let directory=await realpath(root);
 for(const part of ['tools','document-poc-runtime','.state']){
  directory=join(directory,part);
  try{await mkdir(directory,{mode:0o700});}catch(error){if(error.code!=='EEXIST')throw error;}
  const info=await lstat(directory);
  if(!info.isDirectory()||info.isSymbolicLink()||await realpath(directory)!==directory)throw Error('Receipt directory boundary invalid');
 }
 directory=join(directory,'document-load-export');
 // A fresh directory and exclusive regular file prevent reuse of stale evidence.
 await mkdir(directory,{mode:0o700});
 const file=await open(join(directory,'qualification.json'),'wx',0o600);
 try{await file.writeFile(bytes);await file.sync();}finally{await file.close();}
 return {sha256:envelope.sha256,byteLength:bytes.length};
}
