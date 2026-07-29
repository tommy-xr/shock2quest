import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult, EntitySummary } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

const LIVE_ASSASSIN = 649;
const ASSASSIN_CORRIDOR_SPIKES = [1226, 1227, 1309] as const;

function only<T>(matches: T[], label: string): T {
  assert.equal(matches.length, 1, `expected one ${label}`);
  return matches[0]!;
}

function prop(detail: EntityDetailResult, name: string): string | undefined {
  return detail.properties.find((property) => property.name === name)?.value;
}

test(
  "living SHODAN assassin remains supported when corridor spikes activate",
  { skip: !e2eEnabled, timeout: 240_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "shodan.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8471),
    });
    await game.step({ frames: 10 });

    const assassin = only(
      await game.entities.byTemplate(LIVE_ASSASSIN),
      "Red Assassin mission object 649",
    );
    assert.equal(assassin.name, "Red Assassin");

    // The east-loop tripwire activates these three authored DataShape movers
    // together. TurnOn is the exact production SwitchLink payload and keeps
    // this focused on the live creature/moving-terrain interaction.
    for (const templateId of ASSASSIN_CORRIDOR_SPIKES) {
      const spike = only(
        await game.entities.byTemplate(templateId),
        `moving Spike mission object ${templateId}`,
      );
      await game.entities.sendMessage(spike.id, { type: "TurnOn" });
    }
    // Recreate the production timing frontier: the player visited the log-2
    // platform after activating the spike loop, then continued through the
    // east corridor while the authored movers kept cycling.
    await game.player.teleport({ x: 2.68, y: 7.24, z: 10.12 });
    await game.step({ frames: 180 });
    await game.player.teleport({
      x: 29.6472,
      y: -0.356,
      z: -20.3578,
    });

    const samples: EntitySummary[] = [];
    const spike02Positions: number[][] = [];
    // One hundred simulated seconds covers multiple traversals of Spike02's
    // long authored loop while sampling between its waypoints.
    for (let sample = 0; sample < 20; sample += 1) {
      await game.step({ frames: 300 });
      samples.push(
        only(
          await game.entities.byTemplate(LIVE_ASSASSIN),
          "Red Assassin mission object 649",
        ),
      );
      spike02Positions.push(
        only(
          await game.entities.byTemplate(ASSASSIN_CORRIDOR_SPIKES[0]),
          "moving Spike02 mission object 1226",
        ).position,
      );
      if (samples.at(-1)!.position[1] < -5) break;
    }

    const detail = await game.entities.detail(assassin.id);
    assert.ok(Number(prop(detail, "HitPoints")) > 0, "Assassin must remain alive");
    assert.notEqual(prop(detail, "AIBehavior"), "Dead");
    const lowestY = Math.min(...samples.map((sample) => sample.position[1]));
    assert.ok(
      lowestY > -5,
      `living Assassin fell below its authored deck: ${JSON.stringify(
        samples.map((sample) => sample.position),
      )}`,
    );
    const spike02Z = spike02Positions.map((position) => position[2]!);
    assert.ok(
      Math.max(...spike02Z) - Math.min(...spike02Z) > 20,
      `Spike02 did not traverse its long authored cycle: ${JSON.stringify(
        spike02Positions,
      )}`,
    );
  },
);
