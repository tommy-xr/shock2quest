import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult, EntitySummary } from "../src/types.js";
import { teleportVerified } from "./helpers/teleport.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

// Regression for #1064. A VR squeeze release always performs the ordinary
// world DropItem first, then offers the item to whatever the held-hand ray is
// over. Generic GUI hosts accept that offer as a container deposit. That is
// correct for ordinary containers and corpses, but a living killable creature
// deliberately keeps the same container sealed; accepting the offer used to
// hide the weapon behind an inaccessible Contains link.
//
// This runs against the exact authored Earth Weapons subjects from the
// campaign repro. Stable mission identities are discovered each launch;
// runtime entity ids are never hardcoded. Raw teleport is focused setup only:
// acquisition, the failing release, ray selection, and re-grab all use the
// production VR hand pose and squeeze edge.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const EARTH_PISTOL = 246;
const EARTH_TRAINING_DROID = 547;

function only(matches: EntitySummary[], label: string): EntitySummary {
  assert.equal(matches.length, 1, `expected one ${label}, got ${matches.length}`);
  return matches[0];
}

function property(detail: EntityDetailResult, name: string): string | undefined {
  return detail.properties.find((candidate) => candidate.name === name)?.value;
}

async function droidContainsPistol(
  game: GameServer,
  droidId: number,
  pistolId: number,
): Promise<boolean> {
  return (await game.entities.detail(droidId)).outgoing_links.some(
    (link) =>
      link.target_id === pistolId && link.link_type.startsWith("Contains"),
  );
}

async function assertPhysicalWorldPistol(
  game: GameServer,
  pistolId: number,
): Promise<void> {
  assert.equal(
    property(await game.entities.detail(pistolId), "HasRefs")?.toLowerCase(),
    "true",
    "released pistol must remain world-referenced",
  );
  const bodies = (await game.physics.bodies({ entityId: pistolId })).bodies;
  assert.equal(bodies.length, 1, "released pistol must have one physics body");
  assert.ok(
    bodies[0].is_enabled && bodies[0].collision_groups.includes("entity"),
    `released pistol must have an enabled entity body: ${JSON.stringify(bodies[0])}`,
  );
}

test(
  "Earth VR release over a live Training Droid remains a re-grabbable world pistol",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8196),
      debugFlags: ["--vr"],
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
    });
    await game.step({ frames: 30 });

    const pistol = only(
      await game.entities.byTemplate(EARTH_PISTOL),
      "Earth Weapons Pistol 246",
    );
    const droid = only(
      await game.entities.byTemplate(EARTH_TRAINING_DROID),
      "Earth Training Droid 547",
    );
    assert.ok(
      Number(property(await game.entities.detail(droid.id), "HitPoints")) > 0,
      "the offer target must be a living killable Training Droid",
    );
    assert.equal(
      await droidContainsPistol(game, droid.id, pistol.id),
      false,
      "the droid must not start with the pistol",
    );

    // Focused setup: stand beside the authored weapon, then acquire it with
    // the production VR ray+squeeze path.
    await teleportVerified(game, {
      x: pistol.position[0] + 1.0,
      y: pistol.position[1] + 0.5,
      z: pistol.position[2] + 1.0,
    });
    const pistolAim = await game.player.aimAt(pistol.id, {
      hitbox: "center",
      visibility: "required",
    });
    assert.equal(pistolAim.target_confirmed, true, JSON.stringify(pistolAim));
    await aimVrHandAt(game, pistolAim.world_point, 0.35, 1);
    await game.step({ frames: 5 });
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      pistol.id,
      "production squeeze must hold the authored pistol",
    );

    // Carry the held pistol to the live target and keep the hand ray on its
    // classified body when opening the grip: this is the exact #1064 edge.
    await teleportVerified(game, {
      x: droid.position[0] + 1.2,
      y: droid.position[1] + 0.5,
      z: droid.position[2] + 1.2,
    });
    const droidAim = await game.player.aimAt(droid.id, {
      hitbox: "torso",
      visibility: "required",
    });
    assert.equal(droidAim.entity_id, droid.id, JSON.stringify(droidAim));
    assert.equal(droidAim.visibility.state, "visible", JSON.stringify(droidAim));
    await aimVrHandAt(game, droidAim.world_point, 0.45, 1);
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 8 });

    assert.equal((await game.info()).player.right_hand_entity_id, null);
    assert.equal(
      await droidContainsPistol(game, droid.id, pistol.id),
      false,
      "a living creature must reject the offer instead of containing the pistol",
    );
    await assertPhysicalWorldPistol(game, pistol.id);

    // Player-observable recovery: select the dropped body through the real
    // interaction ray, then squeeze it back into the same hand.
    const dropped = only(
      await game.entities.byTemplate(EARTH_PISTOL),
      "released Earth Pistol 246",
    );
    const droppedAim = await game.player.aimAt(dropped.id, {
      hitbox: "center",
      visibility: "required",
    });
    assert.equal(droppedAim.target_confirmed, true, JSON.stringify(droppedAim));
    await aimVrHandAt(game, droppedAim.world_point, 0.35, 1);
    await game.step({ frames: 5 });
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      pistol.id,
      "the world pistol must be re-grabbable through production VR input",
    );

    // Clear-ray release remains the ordinary world-drop control.
    await game.input.set("right_hand.position", [0.45, -0.15, -0.65]);
    await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
    await game.step({ frames: 5 });
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 8 });
    assert.equal((await game.info()).player.right_hand_entity_id, null);
    assert.equal(await droidContainsPistol(game, droid.id, pistol.id), false);
    await assertPhysicalWorldPistol(game, pistol.id);
  },
);
