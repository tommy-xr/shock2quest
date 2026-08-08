import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

const LIVE_RED_ASSASSINS = [643, 644, 649, 678] as const;
const ASSASSIN_CORRIDOR_SPIKES = [1226, 1227, 1309] as const;

function aiProp(detail: EntityDetailResult, name: string): string | undefined {
  return detail.properties.find((property) => property.name === name)?.value;
}

function distance3(
  a: [number, number, number],
  b: [number, number, number],
): number {
  return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
}

test(
  "newly killed SHODAN assassins stop at their terminal death pose",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "shodan.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8460),
    });

    await game.step({ frames: 10 });

    const assassins = await Promise.all(
      LIVE_RED_ASSASSINS.map(async (templateId) => {
        const matches = await game.entities.byTemplate(templateId);
        assert.equal(
          matches.length,
          1,
          `expected one Red Assassin mission object ${templateId}`,
        );
        return matches[0]!;
      }),
    );
    const deathPositions = new Map(
      assassins.map((assassin) => [assassin.id, assassin.position] as const),
    );

    for (const assassin of assassins) {
      await game.entities.sendMessage(assassin.id, {
        type: "Damage",
        amount: 1000,
      });
    }

    // Wait for every randomly selected one-shot crumple to drain. The
    // terminal pose is the ownership boundary: authored root motion may move
    // the creature during the crumple, but no motion may survive afterwards.
    const terminalPositions = new Map<number, [number, number, number]>();
    for (
      let attempt = 0;
      attempt < 120 && terminalPositions.size < assassins.length;
      attempt += 1
    ) {
      await game.step({ frames: 5 });
      for (const assassin of assassins) {
        if (terminalPositions.has(assassin.id)) continue;
        const animation = await game.entities.animation(assassin.id);
        if (
          animation?.clip === null &&
          animation.last_clip &&
          animation.queue.length === 0
        ) {
          const detail = await game.entities.detail(assassin.id);
          assert.equal(aiProp(detail, "AIBehavior"), "Dead");
          const deathPosition = deathPositions.get(assassin.id)!;
          const crumpleTravel = distance3(deathPosition, detail.position);
          assert.ok(
            crumpleTravel < 3,
            `dying Assassin ${assassin.template_id} traveled ${crumpleTravel.toFixed(3)} units from ${JSON.stringify(deathPosition)} to terminal pose ${JSON.stringify(detail.position)}`,
          );
          terminalPositions.set(assassin.id, detail.position);
        }
      }
    }
    assert.equal(
      terminalPositions.size,
      assassins.length,
      "all Assassin death animations should reach their terminal poses",
    );

    // The east-loop tripwire activates these authored DataShape elevators
    // beside the Assassin group. Injecting the same TurnOn payload directly
    // is focused fixture setup: it preserves the production moving-terrain
    // and corpse physics paths without replaying the whole traversal.
    for (const templateId of ASSASSIN_CORRIDOR_SPIKES) {
      const matches = await game.entities.byTemplate(templateId);
      assert.equal(
        matches.length,
        1,
        `expected moving Spike mission object ${templateId}`,
      );
      await game.entities.sendMessage(matches[0]!.id, { type: "TurnOn" });
    }
    await game.step({ frames: 600 });

    for (const assassin of assassins) {
      const terminal = terminalPositions.get(assassin.id)!;
      const detail = await game.entities.detail(assassin.id);
      const drift = distance3(terminal, detail.position);
      assert.equal(aiProp(detail, "AIBehavior"), "Dead");
      assert.ok(
        drift < 0.1,
        `dead Assassin ${assassin.template_id} drifted ${drift.toFixed(3)} units from ${JSON.stringify(terminal)} to ${JSON.stringify(detail.position)}`,
      );
    }
  },
);
