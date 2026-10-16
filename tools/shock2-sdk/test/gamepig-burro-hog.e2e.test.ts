import assert from "node:assert/strict";
import {test} from "node:test";
import {GameServer} from "../src/index.js";
import {openGamePig} from "./helpers/gamepig.js";

for (const vr of [false,true]) {
  test(`Burro Hog walks, jumps, climbs both courses and handles a collision (${vr ? "VR":"flat"})`, {
    skip:process.env.SHOCK2_E2E!=="1",timeout:300_000,
  },async()=>{
    await using game=await GameServer.launch({mission:"medsci1.mis",debugFlags:vr?["--vr"]:[]});
    const {handheld,click}=await openGamePig(game,vr,"Burro Hog Cart");
    const elements=async()=>(await game.ui.state()).active_panel!.elements;
    const control=async(label:string)=>{const found=(await elements()).find(e=>e.label===label);assert.ok(found,`missing ${label}`);return found;};
    await click(game,await control("game-11"));await game.step({frames:12});
    const field=await control("burro-field");
    const player=async()=>{const p=(await elements()).find(e=>e.texture?.startsWith("dhtim"));assert.ok(p);return {x:Math.round(p.rect[0]-field.rect[0]),y:Math.round(p.rect[1]-field.rect[1]),image:p.texture!};};
    const status=async()=>(await elements()).find(e=>/^L\d+ /.test(e.text??""))!.text!;
    const point=async(dx:number,dy:number)=>{const [x,y,w,h]=field.screen_rect;await game.input.set("pointer.position",[x+w/2+dx*w/140,y+h/2+dy*h/276]);};
    const steer=async(x:number,y:number)=>{
      if(vr) await game.input.set("left_hand.thumbstick",[x,-y]);
      else {
        await game.input.set("pointer.pressed",0);await game.step({frames:1});
        if(x||y){await point(0,0);await game.input.set("pointer.pressed",1);await game.step({frames:1});await point(x*30,y*35);}
      }
    };
    const walk=async(x:number)=>{
      let last=0;
      for(let i=0;i<100;i++){
        if((await elements()).some(e=>e.text==="Level clear!")){await steer(0,0);return;}
        const p=await player();if(Math.abs(p.x-x)<=2){await steer(0,0);await game.step({frames:3});return;}
        const direction=Math.sign(x-p.x);if(direction!==last){await steer(direction,0);last=direction;}
        await game.step({frames:3});
      }
      assert.fail(`could not walk to ${x}: ${JSON.stringify(await player())}`);
    };
    const climb=async(column:number)=>{
      await walk(column*8-2);const before=await player();await steer(0,-1);
      for(let i=0;i<100;i++){
        await game.step({frames:3});const p=await player();
        if(p.y<before.y && p.image.includes("dhtimw")){await steer(0,0);return;}
      }
      assert.fail(`could not climb column ${column}`);
    };
    await walk(60);const standing=await player();
    if(vr){await game.input.hold("LeftHandLowerButton");await game.step({frames:3});await game.input.release("LeftHandLowerButton");}
    else {await point(0,0);await game.input.set("pointer.secondary",1);await game.step({frames:3});await game.input.set("pointer.secondary",0);}
    await game.step({frames:6});assert.ok((await player()).y<standing.y,"primary held control / secondary pointer jumps");
    await game.step({frames:45});const stopped=await player();await game.step({frames:12});assert.equal((await player()).x,stopped.x,"released movement stops");
    let collided=false;
    for(let i=0;i<30;i++){await game.step({frames:120});if((await status()).includes("Lives:2")){collided=true;break;}}
    assert.ok(collided,"a natural rolling ham costs a life");
    await game.entities.sendMessage(handheld.entity_id,{type:"SetObjectState",state:"Hacked"});
    await game.step({frames:200});assert.match(await status(),/HACKED/);
    for(const [level,columns] of [[1,[12,3,12,9,12]],[2,[3,9,3,13,7]]] as const){
      assert.match(await status(),new RegExp(`^L${level} `));
      for(const column of columns) await climb(column);
      await walk(38);
      assert.ok((await elements()).some(e=>e.text==="Level clear!"));
      assert.ok(Number((await status()).split(" ").at(-1))>=level*500);
      await game.step({frames:250});
      assert.match(await status(),new RegExp(`^L${level+1} `),"clear advances to alternating course");
    }
  });
}
