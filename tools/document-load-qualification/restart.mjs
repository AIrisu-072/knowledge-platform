/** Qualified runtime has already reached human generation3 / agent generation2. */
export function createLoadRestarter({current,stop,start,replace}){
 let completed=0;
 return async()=>{
  if(completed>=2)throw Error('Qualification restart limit exceeded');
  const {human,agent}=current();
  await stop(human);await stop(agent);
  const generation=++completed;
  const nextHuman=await start('poc-human',3+generation);
  const nextAgent=await start('poc-agent',2+generation);
  replace({human:nextHuman,agent:nextAgent});
 };
}
