import assert from "node:assert/strict";
import { test } from "node:test";
import { existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { GameServer, findRepoRoot } from "../src/index.js";
import { cycleToWeapon } from "./helpers/weapon.js";

// End-to-end regression tests for held-weapon models: on a 25AE install the
// remastered first-person gun models (obj/*_h.bin in mods/sshock2ee.kpf) are
// closed meshes, so a VR-grabbed gun swaps to its _h viewmodel; on a classic
// install the _h meshes have their never-visible faces stripped for the fixed
// flat camera (#352) and VR keeps the world model. These tests run against the
// configured DARK_ASSET_PATH, which is a 25AE install.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

function modelOf(detail: { properties: { name: string; value: string }[] }): string {
  const p = detail.properties.find((x) => x.name === "Model");
  assert.ok(p, "entity should expose a Model property");
  return p.value;
}

test(
  "VR: a grabbed weapon wields the 25AE first-person model",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      debugFlags: ["--vr"],
    });

    // DebugCycleWeapon spawns the pistol; VR wield is a no-op so it drops to the
    // floor in front of the player.
    await game.step({ frames: 10 });
    const pistol = await cycleToWeapon(game, (e) => e.name === "Pistol", {
      settleFrames: 90,
    });
    assert.equal(modelOf(await game.entities.detail(pistol.id)), "atek_w");

    // Grab it: park the right hand on the pistol's forward raycast axis
    // (debug_weapons spawns the pawn at the origin with identity rotation, so
    // pawn-local == world minus the pawn position) and squeeze.
    const pawnY = (await game.info()).player.position[1];
    const [px, py, pz] = pistol.position;
    await game.input.set("right_hand.position", [px + 0.4, py - pawnY, pz]);
    await game.input.set("right_hand.squeeze", 1.0);
    await game.step({ frames: 10 });

    const held = (await game.info()).player.right_hand_entity_id;
    assert.equal(held, pistol.id, "pistol should be grabbed by the right hand");

    // The held pistol swaps to the remastered first-person model (25AE
    // install); dropping it restores the world model.
    assert.equal(modelOf(await game.entities.detail(pistol.id)), "atek_h");

    // The wield renders the mesh complete, exactly as authored: atek_h draws
    // 7 scene objects (its material slots, split across sub-objects). The old
    // spare-hand island strip deleted every `ND-arm_atek.psd` polygon - the
    // baked *firing* hand - which showed up here as a missing draw (6) and in
    // the headset as a handless grip. Keeping all hand islands is deliberate:
    // the real hand outranks hiding a floating spare (PR #1023).
    const draws = (await game.scene.objects({ entityId: pistol.id })).objects;
    assert.equal(
      draws.length,
      7,
      "wielded atek_h should draw its full authored mesh, firing hand included",
    );

    await game.input.set("right_hand.squeeze", 0.0);
    await game.step({ frames: 10 });
    assert.equal(modelOf(await game.entities.detail(pistol.id)), "atek_w");
  },
);

test(
  "VR: a grabbed melee weapon wields the 25AE first-person model",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      debugFlags: ["--vr"],
    });

    // DebugCycleWeapon spawns each roster weapon in turn (VR wield is a no-op,
    // so each drops to the floor); cycle until the Wrench appears.
    await game.step({ frames: 10 });
    let wrench;
    for (let i = 0; i < 20 && !wrench; i++) {
      await game.input.trigger("DebugCycleWeapon");
      await game.step({ frames: 5 });
      wrench = (await game.entities.list({ limit: 200 })).entities.find(
        (e) => e.name === "Wrench",
      );
    }
    assert.ok(wrench, "wrench should have spawned");
    await game.step({ frames: 60 });
    assert.equal(modelOf(await game.entities.detail(wrench.id)), "wrench_w");

    // Grab it: put the hand at the settled wrench's live position (pawn-local)
    // and squeeze.
    const pawn = (await game.info()).player.position;
    const settled = (await game.entities.list({ limit: 200 })).entities.find(
      (e) => e.id === wrench!.id,
    )!;
    const [px, py, pz] = settled.position;
    await game.input.set("right_hand.position", [
      px - pawn[0],
      py - pawn[1],
      pz - pawn[2],
    ]);
    await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
    await game.input.set("right_hand.squeeze", 0.0);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze", 1.0);
    await game.step({ frames: 10 });

    const held = (await game.info()).player.right_hand_entity_id;
    assert.equal(held, wrench.id, "wrench should be grabbed by the right hand");

    // The held wrench swaps to the remastered first-person melee model (LGMM
    // skinned mesh with the arm baked in, rendered at rest pose); dropping it
    // restores the world model.
    assert.equal(modelOf(await game.entities.detail(wrench.id)), "wrench_h");

    await game.input.set("right_hand.squeeze", 0.0);
    await game.step({ frames: 10 });
    assert.equal(modelOf(await game.entities.detail(wrench.id)), "wrench_w");
  },
);

test(
  "VR: dropping the PsiSword takes its materialized first-person model away",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      debugFlags: ["--vr"],
    });

    // The PsiSword authors only PropLimbModel - no world model at all - so the
    // VR wield has to *materialize* its first model. That swap must be
    // symmetric: without the drop half, the released sword keeps rendering the
    // first-person arm mesh where it was let go, forever and through saves.
    await game.step({ frames: 10 });
    let sword;
    for (let i = 0; i < 20 && !sword; i++) {
      await game.input.trigger("DebugCycleWeapon");
      await game.step({ frames: 5 });
      sword = (await game.entities.list({ limit: 200 })).entities.find(
        (e) => e.name === "PsiSword",
      );
    }
    assert.ok(sword, "psi sword should have spawned");
    await game.step({ frames: 60 });

    const pawn = (await game.info()).player.position;
    const settled = (await game.entities.list({ limit: 200 })).entities.find(
      (e) => e.id === sword!.id,
    )!;
    const [px, py, pz] = settled.position;
    await game.input.set("right_hand.position", [
      px - pawn[0],
      py - pawn[1],
      pz - pawn[2],
    ]);
    await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
    await game.input.set("right_hand.squeeze", 0.0);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze", 1.0);
    await game.step({ frames: 10 });
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      sword.id,
      "psi sword should be grabbed by the right hand",
    );
    assert.equal(modelOf(await game.entities.detail(sword.id)), "psword_h");
    assert.ok(
      (await game.scene.objects({ entityId: sword.id })).objects.length > 0,
      "the wielded psi sword should draw",
    );

    await game.input.set("right_hand.squeeze", 0.0);
    await game.step({ frames: 20 });
    assert.equal(
      (await game.entities.detail(sword.id)).properties.find(
        (p) => p.name === "Model",
      ),
      undefined,
      "a dropped psi sword should have no model again",
    );
    assert.equal(
      (await game.scene.objects({ entityId: sword.id })).objects.length,
      0,
      "a dropped psi sword must not leave the first-person arm in the world",
    );
  },
);

test(
  "VR: the LEFT hand can wield a melee weapon, with its damage volume attached",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8108),
      debugFlags: ["--vr"],
    });

    // Nothing exercised a left-hand melee wield at all before, which is how it
    // went unnoticed that it drew a right arm. This is a smoke test for that
    // gap: the left hand completes the whole wield (model swap to the `_h` rig,
    // contact body attached and tracking it) exactly as the right does.
    //
    // It deliberately does NOT try to assert the mirror. The mirror lives in a
    // model-space transform that no HTTP endpoint reports, and the melee rigs
    // are drawn double-sided (`/v1/scene` shows `backface_culling: null`), so
    // there is nothing observable here that a mirror regression would change.
    // The mirror's geometry is asserted in `vr_config`'s unit tests, which can
    // see the transform.
    await game.step({ frames: 10 });
    let wrench;
    for (let i = 0; i < 20 && !wrench; i++) {
      await game.input.trigger("DebugCycleWeapon");
      await game.step({ frames: 5 });
      wrench = (await game.entities.list({ limit: 200 })).entities.find(
        (e) => e.name === "Wrench",
      );
    }
    assert.ok(wrench, "wrench should have spawned");
    await game.step({ frames: 120 });

    // One pose, used for both hands, so the two results are comparable.
    const held: Record<string, number[]> = {};
    const holdAt: [number, number, number] = [-1.7, 0.95, -0.2];
    const yaw45: [number, number, number, number] = [
      0, 0.3826834, 0, 0.9238795,
    ];

    for (const hand of ["right", "left"] as const) {
      const other = hand === "right" ? "left" : "right";
      await game.input.set(`${other}_hand.position`, [3, -3, 3]);

      // Hand channels are pawn-relative, and the pawn drifts slightly while a
      // held weapon's body shoves the capsule around - read it fresh per hand,
      // or the second hand is staged against a stale origin and misses.
      const pawn = (await game.info()).player.position;
      const resting = (await game.entities.list({ limit: 200 })).entities.find(
        (e) => e.id === wrench!.id,
      )!.position;
      await game.input.set(`${hand}_hand.position`, [
        resting[0] - pawn[0],
        resting[1] - pawn[1],
        resting[2] - pawn[2],
      ]);
      await game.input.set(`${hand}_hand.rotation`, yaw45);
      await game.input.set(`${hand}_hand.squeeze`, 0.0);
      await game.step({ frames: 10 });
      await game.input.set(`${hand}_hand.squeeze`, 1.0);
      await game.step({ frames: 30 });

      assert.equal(
        modelOf(await game.entities.detail(wrench.id)),
        "wrench_h",
        `${hand} hand should wield the first-person melee rig`,
      );

      await game.input.set(`${hand}_hand.position`, holdAt);
      await game.step({ frames: 20 });

      held[hand] = (await game.entities.list({ limit: 200 })).entities.find(
        (e) => e.id === wrench!.id,
      )!.position;

      // The entity origin IS the contact collider's body origin.
      const bodies = await game.physics.bodies({ entityId: wrench.id });
      assert.equal(bodies.bodies.length, 1, `${hand}: one held melee body`);
      for (let axis = 0; axis < 3; axis++) {
        assert.ok(
          Math.abs(bodies.bodies[0].position[axis] - held[hand][axis]) < 1e-4,
          `${hand}: the contact body is off the rendered weapon's entity origin`,
        );
      }

      await game.input.set(`${hand}_hand.squeeze`, 0.0);
      await game.step({ frames: 200 });
    }

    // Both hands put the damage volume in the same place. That is by
    // construction (the contact point is on the hand's centreline, which is the
    // axis the mirror reflects across - see `the_contact_point_is_on_the_hands_
    // centreline`), so this is a live check of that reasoning, not of the
    // mirror.
    for (let axis = 0; axis < 3; axis++) {
      assert.ok(
        Math.abs(held.left[axis] - held.right[axis]) < 1e-4,
        `the melee contact volume moved between hands: ${JSON.stringify(held)}`,
      );
    }
  },
);

function findSavePath(saveName: string): string | undefined {
  const repoRoot = findRepoRoot(process.cwd()) ?? process.cwd();
  const roots = [
    process.env.DARK_ASSET_PATH,
    join(repoRoot, "Data"),
    join(repoRoot, "..", "Data"),
  ].filter((root): root is string => Boolean(root));
  return roots
    .map((root) => join(root, "saves", `${saveName}.sav`))
    .find(existsSync);
}

test(
  "VR: a load restores a held weapon wielding the first-person model",
  { skip: !e2eEnabled, timeout: 600_000 },
  async (t) => {
    const saveName = `vr_held_restore_${Date.now()}`;
    let savePath: string | undefined;
    t.after(() => {
      if (savePath) rmSync(savePath, { force: true });
    });

    // Build the save in a flat session, where spawnItem enters the held-items
    // graph; a VR spawn drops the item into the world instead.
    {
      await using game = await GameServer.launch({
        mission: "earth.mis",
      });
      await game.step({ frames: 10 });

      const pistol = await game.player.spawnItem("Pistol");
      await game.save(saveName);

      savePath = findSavePath(saveName);
      assert.ok(savePath, `save ${saveName} should exist on disk`);

      // Mark the pistol as held in the right hand (the shape a VR session
      // writes), without simulating a motion-controller grab.
      const save = JSON.parse(readFileSync(savePath, "utf8"));
      const held = save.global_data.held_items;
      const inventoryLinks =
        held.held_entities.links[String(held.inventory_entity)];
      inventoryLinks.to_links = inventoryLinks.to_links.filter(
        (link: { to_entity_id: number | null }) =>
          link.to_entity_id !== pistol.entity_id,
      );
      held.entity_in_right_hand = pistol.entity_id;
      held.held_entities.properties["P$HasRefs"][String(pistol.entity_id)] =
        true;
      writeFileSync(savePath, JSON.stringify(save));
    }

    // The restore path re-dispatches Hold, so the restored held entity runs
    // the VR wield swap (25AE first-person model, spare-hand strip included).
    // Without that dispatch the restored pistol keeps its saved world model.
    await using game = await GameServer.launch({
      mission: "earth.mis",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 5 });
    // Keep the grip squeezed across the load: VR drops a held item the moment
    // the squeeze reads released, and the harness's channels default to 0.
    await game.input.set("right_hand.squeeze", 1.0);
    await game.step({ frames: 2 });
    await game.load(saveName);
    await game.step({ frames: 10 });

    const heldAfterLoad = (await game.info()).player.right_hand_entity_id;
    assert.ok(heldAfterLoad, "pistol should be held after load");
    assert.equal(modelOf(await game.entities.detail(heldAfterLoad)), "atek_h");
  },
);

test(
  "flat: a wielded weapon swaps to its first-person viewmodel",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
    });

    // In flat, DebugCycleWeapon spawns AND wields (sends Hold), which swaps the
    // model to the atek_h viewmodel for the first-person weapon path.
    await game.step({ frames: 5 });
    const pistol = await cycleToWeapon(game, (e) => e.name === "Pistol");
    assert.equal(modelOf(await game.entities.detail(pistol.id)), "atek_h");
  },
);
