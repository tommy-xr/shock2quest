import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { clickCanvas, pauseEntry, menuEntry, newGameEntry, panelPoint, AIM_AT_PANEL } from "./helpers/frontend-menu.js";
async function finishIntro(game:GameServer) {
  // Exercise the real hold-to-skip path rather than render the entire movie.
  await game.input.set("right_hand.trigger",1);
  await game.step({frames:125});
  await game.input.set("right_hand.trigger",0);
  await game.step({frames:5});
  assert.equal((await game.info()).mission,"earth.mis");
}

const enabled=process.env.SHOCK2_E2E === "1";
async function click(game:GameServer,point:[number,number],vr=false) {
  if (!vr) return clickCanvas(game,[point[0]*640,point[1]*480]);
  const [,y,z]=panelPoint(point);
  await game.input.set("right_hand.position",[0,y,z]);
  await game.input.set("right_hand.rotation",AIM_AT_PANEL);
  for (const trigger of [0,1,0]) {
    await game.input.set("right_hand.trigger",trigger);
    await game.step({frames:5});
  }
}
for (const difficulty of ["easy","normal","hard","impossible"] as const) {
  test(`new campaign: ${difficulty} selection survives the intro`,{skip:!enabled,timeout:120_000},async()=>{
    await using game=await GameServer.launch({mission:"main_menu",difficulty:"hard"});
    await game.step({frames:10});
    await click(game,menuEntry(0));
    assert.equal((await game.info()).mission,"main_menu","selection precedes the intro");
    if (difficulty === "normal") {
      await click(game,newGameEntry("hard")); // Change the pending selection, then cancel it.
      await click(game,newGameEntry("cancel"));
      await click(game,menuEntry(0));
      // Starting without another choice must use the Normal default.
    } else await click(game,newGameEntry(difficulty));
    await click(game,newGameEntry("options"));
    assert.equal((await game.info()).mission,"main_menu","unimplemented Options does not leave the selection page");
    await click(game,newGameEntry("start"));
    assert.equal((await game.info()).mission,"cs1.avi");
    await finishIntro(game);
    const info=await game.info();
    assert.equal(info.mission,"earth.mis");
    assert.equal(info.player.difficulty,difficulty);
    assert.equal(info.player.max_hit_points,{easy:55,normal:35,hard:27,impossible:10}[difficulty]);
  });
}
test("new campaign: VR controller selects Impossible",{skip:!enabled,timeout:120_000},async()=>{
  await using game=await GameServer.launch({mission:"main_menu",debugFlags:["--vr"]});
  await game.step({frames:10});
  await click(game,menuEntry(0),true);
  await click(game,newGameEntry("impossible"),true);
  await click(game,newGameEntry("start"),true);
  assert.equal((await game.info()).mission,"cs1.avi");
  await finishIntro(game);
  assert.equal((await game.info()).player.difficulty,"impossible");
});
test("new campaign: previous character and visited missions are cleared",{skip:!enabled,timeout:180_000},async()=>{
  await using game=await GameServer.launch({mission:"medsci1.mis",difficulty:"impossible"});
  await game.player.setStats({endurance:4,cyber_modules:99});
  assert.equal((await game.entities.byTemplate(1363)).length,0);
  await game.transitionLevel("earth.mis"); // Records the old filtered MedSci deck.
  await game.input.trigger("TogglePauseMenu");
  await game.step({frames:10});
  await clickCanvas(game,pauseEntry(4));
  await game.step({frames:10});
  assert.equal((await game.info()).mission,"main_menu");
  await click(game,menuEntry(0));
  await click(game,newGameEntry("easy")); // Easy
  await click(game,newGameEntry("start"));
  await finishIntro(game);
  const fresh=(await game.info()).player;
  assert.equal(fresh.difficulty,"easy");
  assert.equal(fresh.stats?.endurance,1);
  assert.equal(fresh.stats?.cyber_modules,0);
  await game.transitionLevel("medsci1.mis");
  assert.equal((await game.entities.byTemplate(1363)).length,1,"fresh Easy deck replaces the old Impossible snapshot");
  assert.equal((await game.info()).player.hit_points,55);
});
