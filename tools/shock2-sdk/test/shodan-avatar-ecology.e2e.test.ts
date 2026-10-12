import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult, EntitySummary } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Stable gamesys archetype identity (`cargo dq templates 196`). Runtime entity
// ids are assigned afresh on every launch and must never be used as fixtures.
const SHODAN_AVATAR = -196;
const ECOLOGY_PERIOD_FRAMES = 901;

function property(detail: EntityDetailResult, name: string): string | undefined {
  return detail.properties.find((candidate) => candidate.name === name)?.value;
}

async function livingAvatars(
  game: GameServer,
): Promise<EntitySummary[]> {
  const avatars = await game.entities.byTemplate(SHODAN_AVATAR);
  const living: EntitySummary[] = [];
  for (const avatar of avatars) {
    const detail = await game.entities.detail(avatar.id);
    const hp = Number(property(detail, "HitPoints") ?? "1");
    if (hp > 0 && property(detail, "AIBehavior") !== "Dead") {
      living.push(avatar);
    }
  }
  return living;
}

async function waitForAvatar(
  game: GameServer,
): Promise<EntitySummary> {
  const originalPosition = await game.player.position();
  // Retail chooses exactly one random marker per ecology pulse and rejects it
  // when the RAYCAST flag says the player can see it. Stage the observer just
  // beyond the authored east arena wall, where terrain occludes all four
  // stable SpawnPoint objects. This removes only the retry lottery: the retail
  // timer, SwitchLink, TrapSpawn selection, and entity creation remain real.
  // The occluded observation point is outside walkable terrain. Keep the
  // observer alive through ecology pulses, then restore normal combat rules.
  const cheat = (await game.devParams.list()).params.find(param => param.key === "cheat")!.value;
  await game.devParams.set("cheat", 1);
  await game.player.teleport({ x: 60, y: -91.8, z: 72 });
  let found: EntitySummary | undefined;
  try {
    for (let poll = 0; poll < 3; poll += 1) {
      await game.step({ frames: ECOLOGY_PERIOD_FRAMES });
      assert.ok((await game.info()).player.hit_points! > 0, "the ecology observer remains alive");
      [found] = await livingAvatars(game);
      if (found) break;
    }
  } finally {
    await game.player.teleport(originalPosition);
    await game.step({ frames: 2 });
    await game.devParams.set("cheat", cheat);
  }
  assert.ok(found, "the SHODAN ecology should spawn behind authored terrain");
  return found;
}

test(
  "SHODAN final ecology spawns its authored Avatar",
  { skip: !e2eEnabled, timeout: 240_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "shodan.mis",
    });
    await game.step({ frames: 10 });

    assert.deepEqual(
      await game.entities.byTemplate(SHODAN_AVATAR),
      [],
      "the Avatar must be produced by the final ecology, not pre-placed",
    );

    const avatar = await waitForAvatar(game);
    assert.equal(avatar.name, "SHODAN");
  },
);
