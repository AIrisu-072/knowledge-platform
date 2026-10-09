/** Qualified runtime has already reached human generation3 / agent generation2. */
export function createLoadRestarter({current,stop,start,replace,maxStages=2}){
 if(maxStages!==2&&maxStages!==3)throw Error('Invalid qualification restart stage limit');
 let completed=0;
 return async()=>{
  if(completed>=maxStages)throw Error('Qualification restart limit exceeded');
  const {human,agent}=current();
  await stop(human);await stop(agent);
  const generation=++completed;
  const nextHuman=await start('poc-human',3+generation);
  const nextAgent=await start('poc-agent',2+generation);
  replace({human:nextHuman,agent:nextAgent});
 };
}
