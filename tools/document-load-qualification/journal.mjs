import { open, mkdir, unlink } from 'node:fs/promises';
import { dirname } from 'node:path';
import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { createInterface } from 'node:readline';
const canonical = value => JSON.stringify(value, (_, item) => item && typeof item === 'object' && !Array.isArray(item) ? Object.fromEntries(Object.entries(item).sort(([a],[b])=>a.localeCompare(b))) : item);
const digest = value => createHash('sha256').update(canonical(value)).digest('hex');
export class Journal {
  static async open(path, identity) {
    await mkdir(dirname(path), { recursive:true, mode:0o700 });
    const lock = await open(`${path}.lock`, 'wx', 0o600).catch(()=>{throw Error('Journal locked; verify prior owner stopped before removing lock');});
    await lock.writeFile(`${process.pid}\n`); await lock.sync();
    const journal = new Journal(path, lock);
    try {
      journal.file = await open(path,'a+',0o600);
      const info = await journal.file.stat();
      if(info.size) {
        const tail=Buffer.alloc(1);await journal.file.read(tail,0,1,info.size-1);
        if(tail[0]!==10) throw Error('Journal truncated; preserve evidence and stop');
        const lines=createInterface({input:createReadStream(path),crlfDelay:Infinity});
        for await(const line of lines) {
          const row=JSON.parse(line); const {hash,...record}=row;
          if(journal.sequence===0 && record.kind!=='identity')throw Error('Journal identity header missing');
          if(record.sequence!==journal.sequence || record.previous!==journal.previous || digest(record)!==hash) throw Error('Journal integrity mismatch');
          if(record.kind==='identity') { if(journal.sequence!==0 || canonical(record.value)!==canonical(identity)) throw Error('Journal identity drift'); }
          else if(record.kind==='pending') { if(journal.states.has(record.key))throw Error('Journal duplicate pending'); journal.states.set(record.key,{request:record.request,recoverable:record.recoverable}); }
          else if(record.kind==='result') { const state=journal.states.get(record.key);if(!state || 'result' in state)throw Error('Journal result without pending');state.result=record.result; }
          else throw Error('Journal unknown record');
          journal.sequence++; journal.previous=hash;
        }
      } else { await journal.append({kind:'identity',value:identity}); const parent=await open(dirname(path),'r');try{await parent.sync();}finally{await parent.close();} }
      return journal;
    } catch(error) { await journal.close(); throw error; }
  }
  constructor(path,lock) { this.path=path;this.lock=lock;this.sequence=0;this.previous=null;this.states=new Map();this.busy=false; }
  async append(value) { const record={...value,sequence:this.sequence,previous:this.previous};const hash=digest(record);await this.file.writeFile(JSON.stringify({...record,hash})+'\n');await this.file.sync();this.sequence++;this.previous=hash; }
  get(key) { const state=this.states.get(key);return state ? structuredClone(state) : undefined; }
  async perform(key,request,action,{recoverable=true}={}) {
    if(this.busy)throw Error('Journal supports one serial mutation at a time');
    this.busy=true;
    try {
      let state=this.states.get(key);
      if(state) {
        if(canonical(state.request)!==canonical(request) || state.recoverable!==recoverable)throw Error('Journal request drift');
        if('result' in state)return structuredClone(state.result);
        if(!recoverable)throw Error('Initial create outcome unknown; never repeat POST');
      } else { state={request:structuredClone(request),recoverable};await this.append({kind:'pending',key,...state});this.states.set(key,state); }
      const result=await action(structuredClone(state.request));
      if(result===undefined)throw Error('Mutation result missing; outcome unknown');
      await this.append({kind:'result',key,result});state.result=structuredClone(result);return result;
    } finally { this.busy=false; }
  }
  async close() { if(this.file)await this.file.close();if(this.lock){await this.lock.close();await unlink(`${this.path}.lock`);this.lock=undefined;} }
}
