import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Negative-first coverage for #433. The authored first MedSci interaction is
// corpse 1177 -> Contains -> wrench 990. Follow the real AIPATH bend with
// bounded, collision-valid moves, then use the same flat input channels as the
// desktop runtime. No give-item or remote-Frob shortcut is involved.
test(
  "flat container: opens the starting MedSci corpse and takes its wrench",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8117),
    });
    await game.step({ frames: 5 });

    const corpses = await game.entities.byTemplate(1177);
    assert.equal(corpses.length, 1, "expected authored corpse 1177");
    const corpse = corpses[0];
    const corpseDetail = await game.entities.detail(corpse.id);
    const wrenchLink = corpseDetail.outgoing_links.find(
      (link) => link.link_type.startsWith("Contains") && link.target_name.includes("Wrench"),
    );
    assert.ok(
      wrenchLink,
      `corpse should initially contain the authored wrench: ${JSON.stringify(corpseDetail.outgoing_links)}`,
    );

    // Preserve the player's live origin Y while following the AIPATH X/Z
    // waypoints around the cryo partition.
    const y = (await game.player.position()).y;
    for (const [x, z] of [
      [-36.5, 21.7],
      [-37.1, 25.9],
      [-39.5, 26.5],
      [-39.5, 26.7],
      [-38.3, 27.1],
      [-37.7, 28.9],
      [-37.7, 30.5],
      [-37.4, 31.2],
    ]) {
      const moved = await game.player.moveTo({ x, y, z });
      assert.equal(moved.blocked, false, `authored route blocked at ${x},${z}`);
      await game.step({ frames: 2 });
    }

    // Aim at the body's center, then use/frob on the normal squeeze edge.
    const player = await game.player.position();
    const playerSnapshot = (await game.info()).player;
    const corpseBodies = await game.physics.bodies({ entityId: corpse.id });
    const dx = corpse.position[0] - player.x;
    const dz = corpse.position[2] - player.z;
    const cameraY = player.y + 4.5 / 2.5;
    const dy = corpse.position[1] - cameraY;
    // head.look's authored desktop convention describes the camera position
    // around the origin (yaw=0 renders toward -X), so aim uses the inverse of
    // the target direction.
    const yaw = Math.atan2(-dz, -dx) * (180 / Math.PI);
    const pitch = Math.atan2(-dy, Math.hypot(dx, dz)) * (180 / Math.PI);
    // The authored creature pose shifts the visible torso away from the
    // object's position marker, so converge on it through the same read-only
    // highlighted-entity signal a headless player sees.
    let aimed = false;
    const seenHighlights = new Set<number | null>();
    for (const pitchOffset of [-60, -45, -30, -15, 0, 15]) {
      for (const yawOffset of [0, -10, 10, -20, 20]) {
        await game.input.set("head.look", [yaw + yawOffset, pitch + pitchOffset]);
        await game.step({ frames: 1 });
        const highlighted = (await game.info()).player.highlighted_entity_id;
        seenHighlights.add(highlighted);
        if (highlighted === corpse.id) {
          aimed = true;
          break;
        }
      }
      if (aimed) break;
    }
    assert.ok(
      aimed,
      `normal head look should highlight corpse ${corpse.id}; saw ${JSON.stringify([...seenHighlights])}, target=${JSON.stringify(corpse.position)}, player=${JSON.stringify(player)}, rotation=${JSON.stringify(playerSnapshot.rotation)}, bodies=${JSON.stringify(corpseBodies.bodies)}`,
    );
    await game.input.set("right_hand.squeeze", 1.0);
    await game.step({ frames: 1 });
    await game.input.set("right_hand.squeeze", 0.0);
    await game.step({ frames: 1 });

    const opened = await game.info();
    assert.equal(opened.player.active_container_entity_id, corpse.id);
    assert.deepEqual(opened.player.active_container_item_ids, [wrenchLink.target_id]);

    // Click the wrench's first 4x4 container slot in the screen-space panel.
    await game.input.set("pointer.position", [0.4, 0.55]);
    await game.input.set("pointer.pressed", 1.0);
    await game.step({ frames: 1 });
    await game.input.set("pointer.pressed", 0.0);
    await game.step({ frames: 1 });

    const inventory = await game.player.inventory();
    assert.ok(
      inventory.items.some((item) => item.entity_id === wrenchLink.target_id),
      `expected real input to take wrench; got ${JSON.stringify(inventory.items)}`,
    );

    const afterCorpse = await game.entities.detail(corpse.id);
    assert.ok(
      !afterCorpse.outgoing_links.some(
        (link) =>
          link.link_type.startsWith("Contains") && link.target_id === wrenchLink.target_id,
      ),
      "taking the wrench should remove the live Contains link",
    );

    // Normal use toggles the open container closed.
    await game.input.set("right_hand.squeeze", 1.0);
    await game.step({ frames: 1 });
    await game.input.set("right_hand.squeeze", 0.0);
    await game.step({ frames: 1 });
    assert.equal((await game.info()).player.active_container_entity_id, null);
  },
);
