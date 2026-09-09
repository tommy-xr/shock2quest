import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { cycleToWeapon, fireOnce } from "./helpers/weapon.js";
import { aimVrHandAt, quatFromTo } from "./helpers/vr-hand.js";

const enabled = process.env.SHOCK2_E2E === "1";
for (const vr of [false, true]) for (const setting of [0, 1]) {
  test(`${vr ? "VR" : "flat"} proximity grenade setting ${setting} deploys one sensor and detonates once`,
    { skip: !enabled, timeout: 180_000 }, async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons", debugFlags: vr ? ["--vr"] : [] });
    try {
      await game.step({ frames: 5 });
      await game.player.setStats({ skills: { heavy_weapons: 6 } });
      const gun = await cycleToWeapon(game, e => e.template_id === -21, {settleFrames: vr ? 90 : 10});
      if (vr) {
        await aimVrHandAt(game, gun.position, 0.45, 1, 0, {hand:"right"});
        await game.step({frames:8});
        assert.equal((await game.info()).player.right_hand_entity_id,gun.id);
        await game.input.set("right_hand.position",[0,1,-2]);
        await game.input.set("right_hand.rotation",quatFromTo([0,0,-1],[-1,0,0]));
        await game.step({frames:3});
      }
      if (setting) { await game.input.trigger("CycleGunSetting"); await game.step({ frames: 2 }); }
      await game.input.trigger("EjectClip"); await game.step({ frames: 2 });
      await game.input.trigger("CycleAmmo"); await game.step({ frames: 2 });
      await game.player.spawnItem(-39);
      await game.input.trigger("Reload"); await game.step({ frames: 180 });
      await fireOnce(game);
      let triggers = await game.entities.byTemplate(-1268);
      for (let i = 0; i < 60 && !triggers.length; i++) {
        await game.step({ frames: 30 });
        triggers = await game.entities.byTemplate(-1268);
      }
      assert.equal(triggers.length, 1, "one authored sensor must arm");
      const mineTemplate = setting ? -3444 : -3758;
      const mines = await game.entities.byTemplate(mineTemplate);
      assert.equal(mines.length, 1);
      const bodies = (await game.physics.bodies({entityId: triggers[0]!.id})).bodies;
      assert.equal(bodies.length, 1);
      assert.equal(bodies[0]!.is_sensor, true);
      await game.step({frames: 120});
      assert.equal((await game.entities.byTemplate(-1268)).length, 1, "player and props do not trip the mine");
      if (setting === 0) {
        // The debug spawn is four units forward (-X in this scene). Put a
        // live AI just inside the deployed mine's authored trigger volume.
        const [x,y,z] = mines[0]!.position;
        await game.player.teleport({x:x+5.5,y:y+0.5,z});
        await game.input.set("head.look",[0,0]);
        await game.input.trigger("SpawnDebugMonster");
        await game.step({frames:1});
        assert.equal((await game.entities.byTemplate(-397)).length,1);
      } else {
        await game.entities.sendMessage(mines[0]!.id, {type: "Damage", amount: 1});
      }
      await game.step({frames: 1});
      assert.equal((await game.entities.byTemplate(mineTemplate)).length, 0);
      assert.equal((await game.entities.byTemplate(-1268)).length, 0);
      assert.equal((await game.entities.byTemplate(-3933)).length, 1, "one authored HE blast");
      assert.equal((await game.entities.byTemplate(-2720)).length, 0, "no second standard blast");
    } catch (error) { console.error(game.logs().slice(-30).join("\n")); throw error; }
  });
}

for (const setting of [0, 1]) {
  test(`deployed proximity setting ${setting} survives save/load at rest`,
    {skip: !enabled, timeout: 180_000}, async () => {
    await using game = await GameServer.launch({mission: "earth.mis"});
    try {
      await game.step({frames:30});
      await game.player.setStats({skills:{heavy_weapons:6}});
      await game.player.spawnItem("Gren Launcher");
      await game.input.trigger("EquipGrenadeLauncher"); await game.step({frames:5});
      if (setting) { await game.input.trigger("CycleGunSetting"); await game.step({frames:2}); }
      await game.input.trigger("EjectClip"); await game.step({frames:2});
      await game.input.trigger("CycleAmmo"); await game.step({frames:2});
      await game.player.spawnItem(-39);
      await game.input.trigger("Reload"); await game.step({frames:180});
      await fireOnce(game);
      let triggers = await game.entities.byTemplate(-1268);
      for (let i=0;i<80&&!triggers.length;i++) {
        await game.step({frames:30}); triggers = await game.entities.byTemplate(-1268);
      }
      assert.equal(triggers.length,1);
      const mineTemplate = setting ? -3444 : -3758;
      const [mine] = await game.entities.byTemplate(mineTemplate);
      assert.ok(mine);
      const save = `proximity_${setting}_${Date.now()}`;
      assert.equal((await game.save(save)).success,true);
      assert.equal((await game.load(save)).success,true);
      await game.step({frames:120});
      const [restored] = await game.entities.byTemplate(mineTemplate);
      assert.ok(restored,"deployed mine survives save/load");
      assert.equal((await game.entities.byTemplate(-1268)).length,1,"load must not duplicate sensor");
      assert.ok(Math.hypot(...restored.position.map((v,i)=>v-mine.position[i]!))<0.15,"mine must not relaunch on load");
      const [sensor] = await game.entities.byTemplate(-1268);
      assert.ok(sensor);
      assert.ok(Math.hypot(...sensor.position.map((v,i)=>v-triggers[0]!.position[i]!))<0.15);
      await game.entities.sendMessage(restored.id,{type:"Damage",amount:1});
      await game.step({frames:1});
      assert.equal((await game.entities.byTemplate(mineTemplate)).length,0);
      assert.equal((await game.entities.byTemplate(-1268)).length,0);
      assert.equal((await game.entities.byTemplate(-3933)).length,1);
    } catch(error) {console.error(game.logs().slice(-30).join("\n"));throw error;}
  });
}
