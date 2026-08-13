import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary, UiElement } from "../src/types.js";
import { earthWorldUse } from "./helpers/earth-world-use.js";
import { teleportVerified } from "./helpers/teleport.js";
import { clickUiElement } from "./helpers/ui.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

// Production Earth regression for #965. The two concrete mission objects are
// inherited from the real gamesys Comestible archetypes:
//
//   object 355 -> Juice bottle (-966, foodtype drink)
//   object 352 -> Chips (-92, foodtype chips)
//
// The shipped 25AE allobjs module heals the frobber by one, plays the
// environmental Activate event, and destroys the source even at full HP.
// Negative-first: on main both inventory gestures deliver the correct Frob,
// but Comestible is an UnimplementedScript, so HP and the exact item are
// unchanged.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const BASIC_CHIPS = 352;
const BASIC_JUICE = 355;

async function exactlyOne(
  game: GameServer,
  templateId: number,
  label: string,
): Promise<EntitySummary> {
  const matches = await game.entities.byTemplate(templateId);
  assert.equal(
    matches.length,
    1,
    `expected exactly one ${label} (${templateId}), got ${JSON.stringify(matches)}`,
  );
  return matches[0];
}

function hp(info: Awaited<ReturnType<GameServer["info"]>>): number {
  assert.notEqual(info.player.hit_points, null, "Earth player should have hit points");
  return info.player.hit_points as number;
}

async function stripItem(
  game: GameServer,
  entityId: number,
): Promise<UiElement> {
  const ui = await game.ui.state();
  assert.equal(ui.mode, "use", "flat comestible use must happen in live use mode");
  const item = ui.strip?.elements.find((candidate) => candidate.entity_id === entityId);
  assert.ok(item, `inventory strip should expose carried comestible ${entityId}`);
  return item;
}

async function useInventoryItem(game: GameServer, entityId: number): Promise<void> {
  const item = await stripItem(game, entityId);
  await clickUiElement(game, item);
  assert.equal(
    (await game.ui.state()).cursor?.entity_id,
    entityId,
    "first click should lift the exact carried comestible",
  );
  await clickUiElement(game, item);
  assert.equal(
    (await game.ui.state()).cursor,
    null,
    "second click should complete authored inventory use",
  );
}

test(
  "Earth flat inventory use consumes Juice and Chips with retail one-HP semantics",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8225),
      repoRoot: process.env.SHOCK2_E2E_REPO_ROOT,
    });
    await game.step({ frames: 5 });

    const initial = await game.info();
    assert.equal(hp(initial), 30);
    assert.equal(initial.player.max_hit_points, 30);
    const playerId = initial.player.entity_id;
    assert.notEqual(playerId, null, "Earth should have a live player");

    const juice = await exactlyOne(game, BASIC_JUICE, "Basic Juice bottle");
    const chips = await exactlyOne(game, BASIC_CHIPS, "Basic Chips");
    await earthWorldUse(game, juice);
    await earthWorldUse(game, chips);
    const carried = await game.player.inventory();
    assert.ok(carried.items.some((item) => item.entity_id === juice.id));
    assert.ok(carried.items.some((item) => item.entity_id === chips.id));

    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });

    // Retail consumes a comestible at full HP; the health service simply
    // clamps the one-point heal to MAX_HP.
    const audioBefore =
      (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
    await useInventoryItem(game, juice.id);
    assert.equal(hp(await game.info()), 30);
    assert.equal(
      (await game.entities.byTemplate(BASIC_JUICE)).length,
      0,
      "full-health use must consume exactly the authored Juice bottle",
    );
    assert.ok(
      (await game.player.inventory()).items.some((item) => item.entity_id === chips.id),
      "using Juice must leave the neighboring Chips untouched",
    );
    const activationSounds = (await game.audio.recent()).sounds.filter(
      (sound) =>
        sound.sequence > audioBefore &&
        sound.tags.some(
          ([tag, value]) => tag === "event" && value === "activate",
        ) &&
        sound.tags.some(
          ([tag, value]) => tag === "foodtype" && value === "drink",
        ),
    );
    assert.equal(
      activationSounds.length,
      1,
      `Juice must play its authored Activate schema exactly once: ${JSON.stringify(activationSounds)}`,
    );

    await game.entities.sendMessage(playerId!, { type: "Damage", amount: 10 });
    await game.step({ frames: 2 });
    assert.equal(hp(await game.info()), 20);

    await useInventoryItem(game, chips.id);
    assert.equal(hp(await game.info()), 21, "Chips should restore exactly one HP");
    assert.equal(
      (await game.entities.byTemplate(BASIC_CHIPS)).length,
      0,
      "partial-health use must consume exactly the authored Chips",
    );
    assert.equal((await game.player.inventory()).count, 0);

    // HP and object destruction are ordinary mission state, not script-private
    // latches: prove both survive the canonical save/load path.
    await game.save("comestible-flat-complete");
    await game.load("comestible-flat-complete");
    assert.equal(hp(await game.info()), 21);
    assert.equal((await game.entities.byTemplate(BASIC_JUICE)).length, 0);
    assert.equal((await game.entities.byTemplate(BASIC_CHIPS)).length, 0);
    assert.equal((await game.player.inventory()).count, 0);
  },
);

test(
  "Earth VR held Juice uses the same authored Frob and consumes exactly once",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8226),
      repoRoot: process.env.SHOCK2_E2E_REPO_ROOT,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 5 });

    const initial = await game.info();
    const playerId = initial.player.entity_id;
    assert.notEqual(playerId, null, "Earth should have a live player");
    await game.entities.sendMessage(playerId!, { type: "Damage", amount: 10 });
    await game.step({ frames: 2 });
    assert.equal(hp(await game.info()), 20);

    const juice = await exactlyOne(game, BASIC_JUICE, "Basic Juice bottle");
    // Juice begins slightly above its display surface and is still falling
    // during the first frames. Settle the genuine body before composing the
    // tiny bottle's production hand ray.
    await game.step({ frames: 120 });
    const bodies = (await game.physics.bodies({ entityId: juice.id })).bodies;
    assert.equal(bodies.length, 1, "authored Juice should begin as one world body");
    let bodyPosition = bodies[0].position;
    await teleportVerified(game, {
      x: bodyPosition[0] - 1.5,
      y: bodyPosition[1] - 0.42,
      z: bodyPosition[2],
    });
    await game.step({ frames: 60 });
    const settledBodies = (await game.physics.bodies({ entityId: juice.id })).bodies;
    assert.equal(settledBodies.length, 1);
    bodyPosition = settledBodies[0].position;
    const handRay = await aimVrHandAt(game, bodyPosition, 0.8);
    const hit = await game.raycast({
      start: handRay.start,
      end: handRay.target,
      collision_groups: ["entity", "selectable", "world", "ui", "raycast"],
      max_distance: 1,
      ignore_sensors: true,
    });
    assert.equal(
      hit.entity_id,
      juice.id,
      `production hand ray must select exact Juice: ${JSON.stringify({ juice, bodies, handRay, hit })}`,
    );

    await game.input.set("right_hand.squeeze", 1);
    // PlayerInfo snapshots held entities at the start of the next frame after
    // the interaction takes ownership, so advance one grab frame plus one
    // observation frame.
    await game.step({ frames: 2 });
    const afterGrabInfo = await game.info();
    assert.equal(
      afterGrabInfo.player.right_hand_entity_id,
      juice.id,
      `right VR hand must physically hold the exact authored Juice: ${JSON.stringify({ handRay, hit, afterGrabInfo })}`,
    );
    assert.equal(
      (await game.physics.bodies({ entityId: juice.id })).bodies.length,
      0,
      "held Juice must already be outside world-ray selection",
    );

    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 2 });

    const trace = await game.messages.recent();
    const foodMessages = trace.messages.filter(
      (message) => message.to.entity_id === juice.id,
    );
    assert.ok(
      foodMessages.some((message) => message.payload === "Frob"),
      "holder trigger must deliver the authored inventory Frob",
    );
    assert.ok(
      !foodMessages.some((message) => message.payload === "TriggerPull"),
      "food must not receive the weapon trigger protocol",
    );
    assert.equal(hp(await game.info()), 21, "one VR use should restore exactly one HP");
    assert.equal(
      (await game.entities.byTemplate(BASIC_JUICE)).length,
      0,
      "the one holder-trigger edge must consume exactly the held Juice",
    );
    assert.equal((await game.info()).player.right_hand_entity_id, null);
    assert.ok(
      !(await game.player.inventory()).items.some((item) => item.entity_id === juice.id),
      "destroyed Juice must leave every carried location",
    );
    assert.equal(
      (await game.entities.byTemplate(BASIC_CHIPS)).length,
      1,
      "using Juice must not consume the neighboring Chips",
    );

    // Holding the trigger for more frames must not apply another heal after
    // canonical teardown removed the source entity.
    await game.step({ frames: 30 });
    assert.equal(hp(await game.info()), 21);
    assert.equal((await game.entities.byTemplate(BASIC_JUICE)).length, 0);
  },
);
