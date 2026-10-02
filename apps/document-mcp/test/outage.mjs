// Invoked only after the owned real-runtime harness has stopped the Agent server.
import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {dirname,join} from 'node:path';
import {connect} from 'node:net';
import {Client} from '@modelcontextprotocol/client';
import {StdioClientTransport} from '@modelcontextprotocol/client/stdio';
async function run(){
  const path=process.env.KP_POC_RUNTIME_CONTEXT;
  assert.ok(path);
  const context=JSON.parse(await readFile(path,'utf8'));
  assert.ok(context.runId&&context.agent);
  const origin=new URL(context.agent);
  assert.equal(origin.protocol,'http:');assert.equal(origin.hostname,'127.0.0.1');
  // A live endpoint with bad identity/HTTP errors must never count as stopped.
  await new Promise((resolve,reject)=>{
    const socket=connect({host:origin.hostname,port:Number(origin.port||80)});
    socket.setTimeout(1000);
    socket.once('connect',()=>{socket.destroy();reject(new Error('Agent listener is still live'));});
    socket.once('timeout',()=>{socket.destroy();reject(new Error('Listener refusal was not observed'));});
    socket.once('error',error=>{socket.destroy();if(error.code==='ECONNREFUSED')resolve();else reject(error);});
  });
  const client=new Client({name:'document-real-outage-acceptance',version:'0.0.0'});
  const transport=new StdioClientTransport({command:process.execPath,args:[new URL('../dist/main.cjs',import.meta.url).pathname],env:{KP_DOCUMENT_API_BASE_URL:context.agent},stderr:'pipe'});
  let stderr='';transport.stderr.on('data',chunk=>stderr+=chunk);
  try {
    await assert.rejects(client.connect(transport,{timeout:5000}),error=>!/timeout|timed out/i.test(error.message));
  } finally {await client.close();}
  assert.match(stderr,/Document MCP startup failed:/);
  assert.ok(!stderr.includes(context.agent));
  await writeFile(join(dirname(path),'agent-outage.json'),JSON.stringify({runId:context.runId,status:'PASS',actualStdio:true,stoppedAgentRejected:true})+'\n');
  console.log('PASS actual stdio rejects stopped Agent server');
}
run().catch(()=>{console.error('Document MCP outage acceptance failed');process.exitCode=1;});
