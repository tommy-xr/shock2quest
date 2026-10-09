import {GameServer} from '/Users/bryphe/code/shock2quest-vr/tools/shock2-sdk/dist/src/index.js';
import {mkdir,writeFile} from 'node:fs/promises';
import {execFileSync} from 'node:child_process';
const out='/tmp/visibility-bounds-host-abba';await mkdir(out,{recursive:true});
const order=['A1','B1','B2','A2'];const rows=[];
for(const label of order){
 if(execFileSync('ps',['-Ao','comm='],{encoding:'utf8'}).split('\n').some(p=>/\/(rustc|cargo-apk)$/.test(p.trim())))throw Error('Compiler running; reject host timing');
 const binary=label[0]==='A'?'/tmp/shock2quest-visibility-optimized':'/tmp/shock2quest-visibility-bounds';
 const game=await GameServer.launch({runtimeBinary:binary,mission:'many.mis',experimental:['fixed_simulation'],debugFlags:['--vr','--benchmark-scene','/tmp/many-update-only.fixture.json'],repoRoot:'/Users/bryphe/code/shock2quest-vr'});
 try{await game.step({frames:600});const before=await game.scene.objects({limit:10000});const samples=[];
 for(let n=0;n<6;n++){const start=performance.now();await game.step({frames:120});samples.push((performance.now()-start)/120);
 if(execFileSync('ps',['-Ao','comm='],{encoding:'utf8'}).split('\n').some(p=>/\/(rustc|cargo-apk)$/.test(p.trim())))throw Error('Compilation resumed; reject host timing');}
 const after=await game.scene.objects({limit:10000});const row={label,binary,frames:720,warmup:600,ms_per_frame:samples,mean_ms:samples.reduce((a,b)=>a+b)/samples.length,before_count:before.total_count,after_count:after.total_count};rows.push(row);console.log(JSON.stringify(row));await writeFile(out+'/results.json',JSON.stringify({kind:'unpaced-rendered-host-throughput',note:'Includes game update, scene preparation, GPU rendering, visibility, swap and amortized HTTP overhead; not visibility-only or headset FPS.',rows},null,2));
 }finally{await game.shutdown();}
}
