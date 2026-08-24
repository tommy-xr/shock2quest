import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult, EntitySummary, Vec3 } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";
// Bare save name copied into DARK_ASSET_PATH/saves by the caller. Retail save
// payloads are copyrighted and intentionally remain outside the repository.
const avatarSave = process.env.SHOCK2_SHODAN_AVATAR_SAVE;
const bossE2eEnabled = e2eEnabled && Boolean(avatarSave);

// Stable gamesys archetype identity (`cargo dq templates 196`). Runtime entity
// ids are assigned afresh on every launch and must never be used as fixtures.
const SHODAN_AVATAR = -196;
const ECOLOGY_PERIOD_FRAMES = 901;

function property(detail: EntityDetailResult, name: string): string | undefined {
  return detail.properties.find((candidate) => candidate.name === name)?.value;
}

function distance(a: Vec3, b: Vec3): number {
  return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
}

async function livingAvatars(
  game: GameServer,
  excludedId?: number,
): Promise<EntitySummary[]> {
  const avatars = await game.entities.byTemplate(SHODAN_AVATAR);
  const living: EntitySummary[] = [];
  for (const avatar of avatars) {
    if (avatar.id === excludedId) continue;
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
  excludedId?: number,
): Promise<EntitySummary> {
  const originalPosition = await game.player.position();
  // Retail chooses exactly one random marker per ecology pulse and rejects it
  // when the RAYCAST flag says the player can see it. Stage the observer just
  // beyond the authored east arena wall, where terrain occludes all four
  // stable SpawnPoint objects. This removes only the retry lottery: the retail
  // timer, SwitchLink, TrapSpawn selection, and entity creation remain real.
  await game.player.teleport({ x: 60, y: -91.8, z: 72 });
  let found: EntitySummary | undefined;
  try {
    for (let poll = 0; poll < 3; poll += 1) {
      await game.step({ frames: ECOLOGY_PERIOD_FRAMES });
      [found] = await livingAvatars(game, excludedId);
      if (found) break;
    }
  } finally {
    await game.player.teleport(originalPosition);
    await game.step({ frames: 2 });
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

test(
  "boss-entry save restores Avatar pursuit, attack, and respawn",
  { skip: !bossE2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "shodan.mis",
    });
    await game.load(avatarSave!);
    await game.step({ frames: 10 });

    const avatar = await waitForAvatar(game);
    const player = (await game.info()).player.position;
    const initialDetail = await game.entities.detail(avatar.id);
    const initialDistance = distance(initialDetail.position, player);
    assert.equal(property(initialDetail, "AIAlertness"), "High");
    assert.equal(property(initialDetail, "AITargetVisible"), "true");

    let nearestDistance = initialDistance;
    let attacked = false;
    for (let second = 0; second < 20; second += 1) {
      await game.step({ frames: 60 });
      const detail = await game.entities.detail(avatar.id);
      nearestDistance = Math.min(
        nearestDistance,
        distance(detail.position, (await game.info()).player.position),
      );
      attacked ||= property(detail, "AIBehavior")?.endsWith("Attack") ?? false;
      if (attacked && nearestDistance < initialDistance - 2) break;
    }
    assert.ok(
      nearestDistance < initialDistance - 2,
      `GotoLoc should pursue the player (${initialDistance} -> ${nearestDistance})`,
    );
    assert.ok(attacked, "the pursuing Avatar should enter a production attack behavior");

    // Exercise ordinary lethal creature damage. Runtime ids below came from
    // stable-template discovery in this launch; no concrete id is a fixture.
    await game.entities.sendMessage(avatar.id, {
      type: "Damage",
      amount: 1000,
    });
    await game.step({ frames: 60 });
    assert.equal(
      property(await game.entities.detail(avatar.id), "AIBehavior"),
      "Dead",
    );

    const replacement = await waitForAvatar(game, avatar.id);
    assert.equal(replacement.name, "SHODAN");
  },
);
