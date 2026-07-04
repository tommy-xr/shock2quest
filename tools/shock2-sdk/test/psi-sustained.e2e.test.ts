import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult } from "../src/index.js";
import { fireOnce } from "./helpers/weapon.js";

// End-to-end tests for sustained (timed) psi powers. Photonic Redirection
// ("Inviso" in the gamesys, tier 4) is the reference power: casting it spends
// 4 psi points and makes the player invisible to AI/cameras for
// duration_base + duration_per_psi x PSI = 5 + 5*5 = 30 seconds; the active
// power list is published via /v1/info as player.active_psi_powers.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const INVISO_COST = 4;
/** Inviso duration at the effective PSI stat of 5: 5 + 5*5 seconds. */
const INVISO_DURATION_FRAMES = 30 * 60;

function aiProp(detail: EntityDetailResult, name: string): string | undefined {
  return detail.properties.find((p) => p.name === name)?.value;
}

/** Cycle the psi power selection until Inviso is selected (bounded). */
async function selectInviso(game: GameServer): Promise<void> {
  for (let i = 0; i < 40; i++) {
    const selected = (await game.info()).player.selected_psi_power;
    if (selected === "Inviso") return;
    await game.input.trigger("CyclePsiPower");
    await game.step({ frames: 1 });
  }
  assert.fail("could not cycle the psi power selection to Inviso");
}

/** Step `frames` in chunks so a single /v1/step call stays small. */
async function stepFrames(game: GameServer, frames: number): Promise<void> {
  const chunk = 300;
  for (let done = 0; done < frames; done += chunk) {
    await game.step({ frames: Math.min(chunk, frames - done) });
  }
}

test(
  "sustained psi power (Inviso): cast spends psi, refresh, and timed expiry",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_psi",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8120),
    });

    // The scene auto-equips the Psi Amp on the first update.
    await game.step({ frames: 10 });
    let player = (await game.info()).player;
    assert.ok(player.wielded_entity_id !== null, "psi amp should be auto-wielded");
    assert.deepEqual(player.active_psi_powers, [], "no sustained powers active at start");
    const startPsi = player.psi_points;
    assert.ok(startPsi !== null && startPsi >= 2 * INVISO_COST);

    // Cast Photonic Redirection (tier 4 sustained power).
    await selectInviso(game);
    await fireOnce(game);
    player = (await game.info()).player;
    assert.equal(player.psi_points, startPsi! - INVISO_COST, "Inviso cast costs 4 psi points");
    assert.deepEqual(player.active_psi_powers, ["Inviso"], "Inviso is active after the cast");

    // Re-casting spends again and refreshes the duration (no duplicate entry).
    await fireOnce(game);
    player = (await game.info()).player;
    assert.equal(player.psi_points, startPsi! - 2 * INVISO_COST, "re-cast spends again");
    assert.deepEqual(player.active_psi_powers, ["Inviso"], "re-cast refreshes, not duplicates");

    // Still active just before the 30 s duration elapses...
    await stepFrames(game, INVISO_DURATION_FRAMES - 60);
    player = (await game.info()).player;
    assert.deepEqual(player.active_psi_powers, ["Inviso"], "still active before expiry");

    // ...and expired just after.
    await stepFrames(game, 120);
    player = (await game.info()).player;
    assert.deepEqual(player.active_psi_powers, [], "Inviso expires after 30 seconds");
  },
);

test(
  "Photonic Redirection hides the player from the security camera",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    // Control: the camera in debug_camera spots the visible player and
    // escalates its alertness within ~15 s. This guards the invisibility
    // assertion below against a camera that never detects anything.
    let detectionFrames: number;
    {
      await using game = await GameServer.launch({
        mission: "debug_camera",
        port: Number(process.env.SHOCK2_E2E_PORT ?? 8121),
      });
      await game.step({ frames: 10 });
      const camera = (await game.entities.list({ filter: "Security Camera", limit: 5 }))
        .entities[0];
      assert.ok(camera, "debug_camera should contain a Security Camera");
      assert.equal(aiProp(await game.entities.detail(camera.id), "AIAlertness"), "Lowest");

      // Step until the camera escalates (bounded at 20 s).
      detectionFrames = 0;
      let alertness = "Lowest";
      while (detectionFrames < 20 * 60) {
        await game.step({ frames: 60 });
        detectionFrames += 60;
        alertness = aiProp(await game.entities.detail(camera.id), "AIAlertness") ?? "Lowest";
        if (alertness !== "Lowest") break;
      }
      assert.notEqual(alertness, "Lowest", "camera should spot the visible player within 20 s");
    }

    // With Inviso active the camera must NOT detect the player, watched for
    // the same number of frames that sufficed to spot the visible player
    // (plus slack), all well inside the 30 s duration.
    {
      await using game = await GameServer.launch({
        mission: "debug_camera",
        port: Number(process.env.SHOCK2_E2E_PORT ?? 8122),
      });
      await game.step({ frames: 10 });
      const camera = (await game.entities.list({ filter: "Security Camera", limit: 5 }))
        .entities[0];
      assert.ok(camera, "debug_camera should contain a Security Camera");

      // Cast Inviso immediately (~0.5 s in, long before the camera reacts).
      await selectInviso(game);
      await fireOnce(game);
      const player = (await game.info()).player;
      assert.deepEqual(player.active_psi_powers, ["Inviso"], "Inviso active in debug_camera");
      assert.equal(
        aiProp(await game.entities.detail(camera.id), "AIAlertness"),
        "Lowest",
        "camera has not reacted yet at cast time (else this test proves nothing)",
      );

      const watchFrames = Math.min(detectionFrames + 5 * 60, INVISO_DURATION_FRAMES - 10 * 60);
      await stepFrames(game, watchFrames);
      assert.deepEqual(
        (await game.info()).player.active_psi_powers,
        ["Inviso"],
        "Inviso still active for the whole watch window",
      );
      assert.equal(
        aiProp(await game.entities.detail(camera.id), "AIAlertness"),
        "Lowest",
        "camera must not spot the psi-invisible player",
      );
    }
  },
);
