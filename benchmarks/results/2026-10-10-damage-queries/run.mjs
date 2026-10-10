import {GameServer} from '../../../tools/shock2-sdk/dist/src/index.js';
import {fileURLToPath} from 'node:url';
const repoRoot=fileURLToPath(new URL('../../../',import.meta.url));
import {writeFile} from 'node:fs/promises';
const rows=[];
for(const mission of ['many.mis','medsci1.mis']) {
 const game=await GameServer.launch({runtimeBinary:repoRoot+'target/debug/debug_runtime',mission,debugFlags:['--vr'],rustLog:'error',repoRoot});
 try {
  for(let i=0;i<3;i++) await game.step({frames:600});
  const audits=game.logs().filter(s=>s.includes('SHOCK2QUEST_DAMAGE_QUERY_AUDIT ')).map(s=>JSON.parse(s.split('SHOCK2QUEST_DAMAGE_QUERY_AUDIT ')[1]));
  if(audits.length!==3)throw Error(`Expected 3 audit blocks: ${audits.length}`);
  const row={mission,audits}; rows.push(row); console.log(JSON.stringify(row));
  await writeFile(new URL('./results.json',import.meta.url),JSON.stringify(rows,null,2));
 } finally {await game.shutdown();}
}
