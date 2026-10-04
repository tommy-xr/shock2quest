#!/usr/bin/env node
// Build first: cargo build -p debug_runtime; npm --prefix tools/shock2-sdk run build
// Run: node tools/shock2-sdk/scripts/equipment-shots.mjs [--url http://127.0.0.1:PORT] [--preview]
// An explicit URL must belong to your disposable debug_weapons --vr runtime.
// The script shuts its runtime down, including on capture/encoding failures.
import assert from 'node:assert/strict';
import { mkdir, writeFile, copyFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, resolve, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { GameServer, quatMultiply, quatRotate } from '../dist/src/index.js';
import { quatConjugate, quatNormalize, add, sub, scale } from '../dist/src/vec.js';
import { drawPersonalCard } from '../dist/test/helpers/vr-hand.js';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const output = join(repo, 'screenshots/equipment');
const framesRoot = join(tmpdir(), 'shock2quest-equipment-frames');
const args = process.argv.slice(2);
const url = args.includes('--url') ? args[args.indexOf('--url') + 1] : undefined;
const preview = args.includes('--preview');
const game = url ? await GameServer.connect(url) : await GameServer.launch({
  mission: 'debug_weapons', repoRoot: repo, debugFlags: ['--vr', '--window-size', '1200x900'],
});
const width = 1200, height = 900, fps = 15, duration = 4, fov = 50;
const forwardHand = [0, Math.SQRT1_2, 0, Math.SQRT1_2];
const backHand = [0, -Math.SQRT1_2, 0, Math.SQRT1_2];
let pawn;
let mfdMotion;
const world = local => add(pawn.position, quatRotate(pawn.rotation, local));
const normalize = v => scale(v, 1 / Math.hypot(...v));
const cross = (a,b) => [a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]];
const dot = (a,b) => a.reduce((sum,v,i) => sum+v*b[i],0);
function project(point, eye, target) {
  const front=normalize(sub(target,eye)), right=normalize(cross(front,[0,1,0])), up=cross(right,front);
  const delta=sub(point,eye), depth=dot(delta,front), tangent=Math.tan(fov*Math.PI/360);
  return {x:.5+dot(delta,right)/(2*depth*tangent*width/height), y:.5-dot(delta,up)/(2*depth*tangent)};
}
async function hands(left, right, leftRotation=forwardHand, rightRotation=forwardHand) {
  for (const [name,position,rotation] of [['left',left,leftRotation],['right',right,rightRotation]]) {
    await game.input.set(`${name}_hand.position`,position);
    await game.input.set(`${name}_hand.rotation`,rotation);
  }
  await game.step({frames:3});
}
async function drawMfd() {
  await drawPersonalCard(game,'left');
  await hands([-.35,.55,.25],[-.1,.3,-.25],forwardHand,forwardHand);
  const panel=(await game.ui.state()).panel_pose;
  assert.ok(panel,'Drawn MFD has a screen');
  // Preserve the authored grip: rotate the controller so its screen is upright,
  // with +Z facing the spectator in front of the player.
  const rotation=quatMultiply(quatMultiply(backHand,quatConjugate(panel.rotation)),forwardHand);
  await game.input.set('left_hand.rotation',rotation);
  await game.step({frames:3});
  assert.equal((await game.info()).player.hand_feedback.body_gear.personal_card.hand,0);
}
async function stowMfd() {
  await game.input.set('left_hand.squeeze',0);
  await hands([-.3,.42,.34],[-.3,.42,-.34]);
  assert.equal((await game.info()).player.hand_feedback.body_gear.personal_card.hand,null);
}
const metadata={version:1,width,height,fps,duration,overview:'overview.png',clips:[]};
try {
  await mkdir(output,{recursive:true});
  await game.camera.attach();
  await game.step({frames:120});
  await game.player.teleport({x:12,y:1.244,z:-12});
  await game.input.set('head.look',[0,0]);
  for (const [key,value] of [['vr_debug_body',1],['ambient_light_intensity',.5],['fov_override_deg',fov]]) await game.devParams.set(key,value);
  await hands([-.35,.45,.35],[-.3,.4,-.35]);
  await game.step({frames:60});
  pawn=(await game.info()).player;
  assert.ok(Math.abs(pawn.rotation[3]-1)<.001,'Use the debug_weapons default pawn heading');
  const pistol=await game.player.spawnItem(-17,{hand:'right'});
  await game.input.set('right_hand.squeeze',1); await game.step({frames:3});
  let player=(await game.info()).player;
  await game.input.set('right_hand.position',quatRotate(quatConjugate(player.rotation),sub(player.hand_feedback.holsters.centers[0],player.position)));
  await game.step({frames:3}); await game.input.set('right_hand.squeeze',0); await game.step({frames:4});
  assert.equal((await game.info()).player.hand_feedback.holsters.items[0],pistol.entity_id);
  await game.step({frames:180}); // Let the transient holster notification expire.
  await drawMfd();
  await game.camera.set({position:world([-3.05,1.1,-1.15]),lookAt:world([.3,.15,0])});
  await game.step({frames:1}); await game.screenshot(join(output,'overview.png'));
  if(args.includes('--pr-media')) {
    await mkdir(framesRoot,{recursive:true});
    await game.devParams.set('vr_debug_body',0); await game.step({frames:1});
    await game.screenshot(join(framesRoot,'foundation-before.png'));
    await game.devParams.set('vr_debug_body',1); await game.step({frames:1});
    await game.screenshot(join(framesRoot,'foundation-after.png'));
  }

  const clips=[
    {id:'mfd',label:'Handheld MFD',prepare:async()=>{
      await drawMfd();
      const end=(await game.input.state()).left_hand;
      const target=world((await game.ui.state()).panel_pose.center);
      await drawPersonalCard(game,'left');
      const start=(await game.input.state()).left_hand;
      mfdMotion={start,end,target};
    },
      target:async()=>world((await game.ui.state()).panel_pose.center),offset:[-1.25,.25,-.25],lookOffset:[.04,.05,-.04]},
    {id:'ammo',label:'Ammo pouch',prepare:stowMfd,
      target:async()=>(await game.info()).player.hand_feedback.ammo_pouch.center,offset:[-1.08,.25,-.42],lookOffset:[.04,.04,0]},
    {id:'holsters',label:'Thigh holsters',prepare:stowMfd,
      target:async()=>(await game.info()).player.hand_feedback.holsters.centers[0],offset:[-1.05,.22,-.66],lookOffset:[.03,.08,.08]},
    {id:'biometrics',label:'Wrist biometrics',prepare:async()=>{await stowMfd();await hands([-.3,.42,.34],[-.4,.75,-.28]);},
      // The implant target is on the pinky edge; move to the adjacent curved
      // health/psi band. This offset was checked against a rendered cuff.
      target:async()=>add((await game.info()).player.hand_feedback.implant_sockets.centers[1],[.05,0,-.05]),offset:[-.35,.5,-.55],lookOffset:[.03,.04,0]},
    {id:'inventory',label:'Backpack inventory',prepare:stowMfd,
      target:async()=>add((await game.scene.fromSource('vr_debug_backpack'))[0].position,[.15,0,0]),offset:[1.15,.35,1.3],lookOffset:[-.08,-.03,0]},
  ];
  for (const clip of clips) {
    await clip.prepare(); await game.step({frames:6});
    const point=clip.id==='mfd'?mfdMotion.target:await clip.target(), lookAt=add(point,clip.lookOffset);
    const folder=join(framesRoot,clip.id);await mkdir(folder,{recursive:true});
    const keyframes=[];
    const cameraAt=t=>{
      const angle=.12*Math.sin(t/duration*2*Math.PI), c=Math.cos(angle),s=Math.sin(angle);
      const [x,y,z]=clip.offset;
      return add(point,[x*c-z*s,y+.025*(1-Math.cos(t/duration*2*Math.PI)),x*s+z*c]);
    };
    for (let i=0;i<(preview?1:fps*duration);i++) {
      const time=i/fps, eye=cameraAt(time);
      if(clip.id==='mfd') {
        const t=Math.min(1,time/2), blend=t*t*(3-2*t), {start,end}=mfdMotion;
        const sign=dot(start.rotation,end.rotation)<0?-1:1;
        const rotation=quatNormalize(start.rotation.map((v,j)=>v*(1-blend)+end.rotation[j]*sign*blend));
        await game.input.set('left_hand.position',start.position.map((v,j)=>v*(1-blend)+end.position[j]*blend));
        await game.input.set('left_hand.rotation',rotation);
      }
      await game.camera.set({position:eye,lookAt}); await game.step({frames:4});
      const frame=join(folder,`frame-${String(i).padStart(3,'0')}.png`);
      await game.screenshot(frame);
      if(i===0) await copyFile(frame,join(output,`${clip.id}.png`));
      const actualTarget=await clip.target();
      const anchor=project(actualTarget,eye,lookAt);
      assert.ok(anchor.x>0&&anchor.x<1&&anchor.y>0&&anchor.y<1,`${clip.id} anchor in frame`);
      keyframes.push({time,...anchor});
    }
    if(!preview) {
      keyframes.push({time:duration,...project(await clip.target(),cameraAt(duration),lookAt)});
      execFileSync('ffmpeg',['-hide_banner','-loglevel','error','-y','-framerate',String(fps),'-i',join(folder,'frame-%03d.png'),'-c:v','libx264','-crf','22','-preset','slow','-pix_fmt','yuv420p','-movflags','+faststart',join(output,`${clip.id}.mp4`)]);
    }
    metadata.clips.push({id:clip.id,label:clip.label,video:`${clip.id}.mp4`,poster:`${clip.id}.png`,duration,keyframes,
      camera:{verticalFovDeg:fov,lookAt,target:point,offset:clip.offset,yawAmplitudeRadians:.12,elevationAmplitude:.025}});
    console.log(`Captured ${clip.id}${preview?' preview':''}`);
  }
  await writeFile(join(output,'callouts.json'),JSON.stringify(metadata,null,2)+'\n');
} finally {await game.shutdown();}
