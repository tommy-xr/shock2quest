import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { switchCourtLights } from "./helpers/rec1-lights.js";
import type { SceneObjectSummary, Vec3 } from "../src/types.js";
import { aimVrHandAt, quatConjugate, quatFromTo, quatMultiply, quatRotate, sub } from "./helpers/vr-hand.js";
import { cycleToWeapon } from "./helpers/weapon.js";

const enabled = process.env.SHOCK2_E2E === "1";

for (const vr of [false, true]) {
  for (const lit of [true, false]) {
    test(`player models follow room lighting (${vr ? "VR" : "flat"}, flag ${lit ? "on" : "off"})`,
      { skip: !enabled, timeout: 180_000 }, async () => {
        await using game = await GameServer.launch({
          mission: "rec1.mis", debugFlags: vr ? ["--vr"] : [],
          experimental: lit ? ["object_lighting"] : [],
        });
        await game.step({ frames: 2 });
        await game.player.teleport({ x: -2, y: 0.5, z: -213 });
        await game.input.lookAtWorldPoint([2, 0.5, -213]);
        await game.step({ frames: 2 });
        await game.player.setStats({ skills: { standard_weapons: 1 } });
        let heldId: number | undefined;
        if (vr) {
          const pistol = await cycleToWeapon(game, e => e.template_id === -17);
          await aimVrHandAt(game, pistol.position, 0.3, 0, 0, { lookAtTarget: false });
          await game.input.set("right_hand.squeeze", 1);
          await game.step({ frames: 8 });
          heldId = pistol.id;
          assert.equal((await game.info()).player.right_hand_entity_id, heldId);
          const player = (await game.info()).player;
          const inverse = quatConjugate(player.rotation);
          const rotation = quatMultiply(inverse, quatFromTo([0, 0, -1], [1, -0.2, 0]));
          for (const [hand, z] of [["left", -213.42], ["right", -212.58]] as const) {
            const position: Vec3 = [-0.8, 1.3, z];
            await game.input.set(`${hand}_hand.position`, quatRotate(inverse, sub(position, player.position)));
            await game.input.set(`${hand}_hand.rotation`, rotation);
          }
          await game.camera.set({ position: [-2, 2, -213], lookAt: [2, 0.5, -213] });
        } else {
          await game.player.spawnItem("Pistol");
          await game.input.trigger("EquipPistol");
        }
        // Finish the standing-eye transition after teleport/equip so gain
        // comparisons sample the same world positions.
        await game.step({ frames: 120 });
        async function playerMeshes(): Promise<SceneObjectSummary[]> {
          const { objects } = await game.scene.objects();
          // Scene tags retain Shipyard generation bits; entity API IDs are
          // the low 32-bit index, including for this freshly spawned gun.
          const weapons = objects.filter(o => vr ? (o.entity_id != null && o.entity_id % (2 ** 32) === heldId) : o.source === "viewmodel");
          assert.ok(weapons.length > 0, "held weapon/viewmodel must actually be rendered");
          // Mesh provenance distinguishes gloves from the hand HUD/pointers.
          const gloves = vr ? objects.filter(o => o.model === "vr_glove_model.glb") : [];
          if (vr) assert.ok(gloves.length > 0, "glove meshes must actually be rendered");
          return [...weapons, ...gloves].sort((a, b) =>
            `${a.source}:${a.model}:${a.name}`.localeCompare(`${b.source}:${b.model}:${b.name}`));
        }
        await switchCourtLights(game, "TurnOn");
        const on = await playerMeshes();
        if (!lit) {
          assert.ok(on.every(o => o.lighting == null), "flag off preserves legacy shading");
          return;
        }
        assert.ok(on.every(o => o.lighting && o.lighting.received > 0.01),
          `both the held model and glove must receive authored light: ${JSON.stringify(on.map(o => ({model:o.model,source:o.source,lighting:o.lighting})))}`);
        await switchCourtLights(game, "TurnOff");
        const off = await playerMeshes();
        assert.equal(off.length, on.length);
        for (let i = 0; i < on.length; i++) {
          assert.ok(off[i].lighting!.received < on[i].lighting!.received * 0.1,
            "room switch must remove authored direct light from each player mesh");
          assert.deepEqual(off[i].lighting!.ambient, on[i].lighting!.ambient);
        }
        await switchCourtLights(game, "TurnOn");
        await game.devParams.set("level_light_intensity", 0.5);
        await game.step({ frames: 1 });
        const half = await playerMeshes();
        for (let i = 0; i < on.length; i++) {
          assert.ok(Math.abs(half[i].lighting!.received - on[i].lighting!.received * 0.5) < 0.001);
        }
        await game.devParams.reset("level_light_intensity");
        await game.step({ frames: 1 });
        const restored = await playerMeshes();
        for (let i = 0; i < on.length; i++) {
          assert.ok(Math.abs(restored[i].lighting!.received - on[i].lighting!.received) < 0.001);
        }
        if (vr) {
          // A tracked controller can leave the BSP (e.g. through a wall).
          // It should retain a player-cell light set rather than drop all slots.
          await game.input.set("left_hand.position", [10000, 10000, 10000]);
          await game.step({ frames: 1 });
          const { objects } = await game.scene.objects();
          const outside = objects.find(o => o.model === "vr_glove_model.glb"
            && o.name?.startsWith("Left"));
          assert.ok(outside?.lighting && outside.lighting.light_count > 0,
            "out-of-world-rep glove must fall back to the player's light set");
        }
      });
  }
}
