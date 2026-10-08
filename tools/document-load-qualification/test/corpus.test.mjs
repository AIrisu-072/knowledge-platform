import test from 'node:test';import assert from 'node:assert/strict';import {mkdtemp,writeFile,rm}from'node:fs/promises';import{tmpdir}from'node:os';import{join}from'node:path';import{createHash}from'node:crypto';
const {loadCorpus}=await import('../corpus.mjs').catch(()=>({}));
test('official PDF manifest validates origin, rights, acquired timestamp, bounded bytes and exact digest',async t=>{assert.equal(typeof loadCorpus,'function');const root=await mkdtemp(join(tmpdir(),'doc-corpus-'));t.after(()=>rm(root,{recursive:true,force:true}));const bytes=Buffer.from('%PDF-1.7\nfixture test only');await writeFile(join(root,'test.pdf'),bytes);const item={id:'notice-a',path:'test.pdf',url:'https://www.mhlw.go.jp/content/10800000/001472933.pdf',landingUrl:'https://www.mhlw.go.jp/stf/newpage_56768.html',title:'Official notice fixture',publicationDate:'2025-03-31',retrievedAt:'2026-10-08T06:32:55Z',license:'PDL-1.0',licenseUrl:'https://www.mhlw.go.jp/chosakuken/',rightsReview:'official-notice-no-third-party-material',sha256:createHash('sha256').update(bytes).digest('hex'),bytes:bytes.length};await writeFile(join(root,'manifest.json'),JSON.stringify({schemaVersion:1,assets:[item]}));const corpus=await loadCorpus(join(root,'manifest.json'));assert.equal(corpus.assets.length,1);assert.deepEqual(corpus.assets[0].bytes,bytes);for(const change of [{url:'https://evil.example/a.pdf'},{path:'../test.pdf'},{sha256:'0'.repeat(64)},{license:''},{retrievedAt:'not-date'},{bytes:null}]){await writeFile(join(root,'manifest.json'),JSON.stringify({schemaVersion:1,assets:[{...item,...change}]}));await assert.rejects(loadCorpus(join(root,'manifest.json')));} });

test('explicit negative source stays in provenance fingerprint but never in publication workload',async t=>{
 const root=await mkdtemp(join(tmpdir(),'doc-corpus-roles-'));t.after(()=>rm(root,{recursive:true,force:true}));
 const bytes=Buffer.from('%PDF-1.7\nrole fixture');await writeFile(join(root,'source.pdf'),bytes);
 const base={id:'positive',path:'source.pdf',url:'https://www.mhlw.go.jp/content/10800000/001472934.pdf',landingUrl:'https://www.mhlw.go.jp/stf/newpage_56768.html',title:'Official notice fixture',publicationDate:'2025-03-31',retrievedAt:'2026-10-08T06:32:55Z',license:'PDL-1.0',licenseUrl:'https://www.mhlw.go.jp/chosakuken/',rightsReview:'official-notice-no-third-party-material',sha256:createHash('sha256').update(bytes).digest('hex'),bytes:bytes.length};
 const write=assets=>writeFile(join(root,'manifest.json'),JSON.stringify({schemaVersion:1,assets}));
 await write([base,{...base,id:'negative',expectedOutcome:'reject-unsupported'}]);
 const corpus=await loadCorpus(join(root,'manifest.json'));
 assert.equal(corpus.assets.length,1);assert.equal(corpus.assets[0].id,'positive');
 assert.equal(corpus.negativeAssets.length,1);assert.equal(corpus.negativeAssets[0].id,'negative');
 await write([base,{...base,id:'negative',expectedOutcome:'publish'}]);
 assert.notEqual((await loadCorpus(join(root,'manifest.json'))).hash,corpus.hash);
 await write([{...base,expectedOutcome:'accept-any-failure'}]);await assert.rejects(loadCorpus(join(root,'manifest.json')),/provenance|outcome/i);
});
