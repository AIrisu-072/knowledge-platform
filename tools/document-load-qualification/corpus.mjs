import {readFile,lstat,realpath} from 'node:fs/promises';
import {resolve,dirname,relative,isAbsolute} from 'node:path';
import {createHash} from 'node:crypto';
export const sha256=value=>createHash('sha256').update(value).digest('hex');
const official=value=>{try{const u=new URL(value);return u.protocol==='https:' && u.hostname==='www.mhlw.go.jp' && !u.username && !u.password && !u.hash;}catch{return false;}};
export async function loadCorpus(path) {
 const root=await realpath(dirname(resolve(path)));const info=await lstat(path);
 if(!info.isFile() || info.size>1024*1024)throw Error('Invalid corpus manifest');
 const manifest=JSON.parse(await readFile(path,'utf8'));
 if(manifest.schemaVersion!==1 || !Array.isArray(manifest.assets) || !manifest.assets.length || manifest.assets.length>20)throw Error('Invalid corpus manifest');
 const ids=new Set(),assets=[];let total=0;
 for(const item of manifest.assets){
  if(!item || !/^[a-z0-9-]+$/.test(item.id) || ids.has(item.id) || !official(item.url) || !official(item.landingUrl) || !official(item.licenseUrl)
   || item.license!=='PDL-1.0' || item.rightsReview!=='official-notice-no-third-party-material' || typeof item.title!=='string' || !item.title
   || !/^\d{4}-\d{2}-\d{2}$/.test(item.publicationDate) || !Number.isFinite(Date.parse(item.retrievedAt))
   || !/^[0-9a-f]{64}$/.test(item.sha256) || !Number.isSafeInteger(item.bytes) || item.bytes<1 || item.bytes>32*1024*1024
   || typeof item.path!=='string' || isAbsolute(item.path) || item.path.split(/[\\/]/).includes('..'))throw Error('Invalid official PDF provenance');
  const file=resolve(root,item.path),stat=await lstat(file);const actual=await realpath(file);
  if(!stat.isFile() || stat.isSymbolicLink() || relative(root,actual).startsWith('..') || stat.size!==item.bytes || (total+=stat.size)>64*1024*1024)throw Error('Invalid corpus file boundary');
  const bytes=await readFile(actual);if(!bytes.subarray(0,5).equals(Buffer.from('%PDF-')) || sha256(bytes)!==item.sha256)throw Error('Corpus PDF digest mismatch');
  ids.add(item.id);assets.push({...item,bytes,filename:`${item.id}.pdf`,mediaType:'application/pdf'});
 }
 return {manifest,hash:sha256(JSON.stringify(manifest.assets.map(({path,retrievedAt,...asset})=>asset))),assets};
}
