import assert from "node:assert/strict";
import {test} from "node:test";
import {GameServer} from "../src/index.js";
import {openGamePig} from "./helpers/gamepig.js";

for(const vr of [false,true]){
  test(`Golf loads terrain, aims, swings and permits one hacked retry (${vr?"VR":"flat"})`,{skip:process.env.SHOCK2_E2E!=="1",timeout:120_000},async()=>{
    await using game=await GameServer.launch({mission:"medsci1.mis",debugFlags:vr?["--vr"]:[]});
    const {handheld,click}=await openGamePig(game,vr,"Golf Cart");
    const elements=async()=>(await game.ui.state()).active_panel!.elements;
    const control=async(label:string)=>{const found=(await elements()).find(e=>e.label===label);assert.ok(found,`missing ${label}`);return found;};
    await click(game,await control("game-12"));
    const field=await control("golf-course");
    assert.ok((await elements()).some(e=>e.texture==="gfhol001.pcx"),"authored first course loaded");
    const status=async()=>(await elements()).find(e=>/^Hole 1\n/.test(e.text??""))!.text!;
    assert.match(await status(),/Shot 1/);
    const golfer=async()=>(await elements()).find(e=>e.texture==="gfgolf2.pcx")!.rect;
    const tee=await golfer();
    const press=async(action:"LeftHandLowerButton"|"LeftHandUpperButton")=>{await game.input.hold(action);await game.step({frames:3});await game.input.release(action);await game.step({frames:3});};
    const primary=async()=>vr?press("LeftHandLowerButton"):click(game,await control("golf-swing"));
    const selectClub=async()=>{
      await click(game,await control("golf-club-0"));
      if(vr) await press("LeftHandUpperButton");else await click(game,await control("golf-club-1"));
      assert.ok((await elements()).some(e=>e.kind==="fill"&&Math.abs(e.rect[0]-(field.rect[0]+28))<0.01&&e.rect[2]===28&&e.rect[3]===1),"club selection outline follows control");
    };
    await selectClub();
    if(vr){
      const rotation=(await game.info()).player.rotation;
      await game.input.set("left_hand.thumbstick",[0,1]);await game.step({frames:3});await game.input.set("left_hand.thumbstick",[0,0]);await game.step({frames:3});
      assert.deepEqual((await game.info()).player.rotation,rotation,"held stick aims without turning player");
    } else {
      const [x,y,w,h]=field.screen_rect;await click(game,{...field,screen_rect:[x+w*0.6,y+h*0.15,0,0]});
    }
    const ball=async()=>(await elements()).find(e=>e.kind==="fill"&&Math.abs(e.rect[2]-2)<0.01&&Math.abs(e.rect[3]-2)<0.01)?.rect;
    const before=await ball();assert.ok(before);
    await primary();await game.step({frames:18});await primary();await game.step({frames:18});await primary();await game.step({frames:18});
    assert.notDeepEqual(await ball(),before,"three clicks launch the ball");
    await game.step({frames:1000});assert.notDeepEqual(await golfer(),tee,"shot advances the golfer to the resting ball");
    assert.match(await status(),/Shot [23]/);
    assert.ok(!(await elements()).some(e=>e.label==="golf-retry"));
    await game.entities.sendMessage(handheld.entity_id,{type:"SetObjectState",state:"Hacked"});await game.step({frames:2});
    await click(game,await control("golf-retry"));
    assert.deepEqual(await golfer(),tee,"hacked retry returns to the authored tee");assert.match(await status(),/Shot 1/);
    assert.ok(!(await elements()).some(e=>e.label==="golf-retry"),"retry is spent until next hole");
    await selectClub();await primary();await game.step({frames:18});await primary();await game.step({frames:18});await primary();await game.step({frames:1000});
    const resting=await golfer();assert.notDeepEqual(resting,tee);
    if(!vr){const [x,y,w,h]=field.screen_rect;await game.input.set("pointer.position",[x+w/2,y+h/2]);await game.input.set("pointer.secondary",1);await game.step({frames:2});await game.input.set("pointer.secondary",0);await game.step({frames:2});assert.deepEqual(await golfer(),resting,"secondary cannot retry twice");}
  });
}
