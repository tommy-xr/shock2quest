import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult, EntitySummary } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

const RED_ASSASSIN = 254;
const NINJA_RUN_01 = 385;
const NINJA_RUN_02 = 386;
const NINJA_RUN_TRAP = 388;
const DESTROY_NINJA_TRAP = 389;

function only<T>(matches: T[], label: string): T {
  assert.equal(matches.length, 1, `expected one ${label}`);
  return matches[0]!;
}

function prop(detail: EntityDetailResult, name: string): string | undefined {
  return detail.properties.find((property) => property.name === name)?.value;
}

function horizontalDistance(a: number[], b: number[]): number {
  return Math.hypot(a[0]! - b[0]!, a[2]! - b[2]!);
}

test(
  "ops2: damaging the Red Assassin starts its authored Ninja Run sequence",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "ops2.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8486),
    });
    await game.step({ frames: 5 });

    // Concrete mission-object ids are stable handles; runtime entity ids are
    // discovered afresh on every launch.
    const assassin = only(
      await game.entities.byTemplate(RED_ASSASSIN),
      "Red Assassin mission object 254",
    );
    const waypoint01 = only(
      await game.entities.byTemplate(NINJA_RUN_01),
      "Ninja Run 01 mission object 385",
    );
    only(
      await game.entities.byTemplate(NINJA_RUN_02),
      "Ninja Run 02 mission object 386",
    );
    only(
      await game.entities.byTemplate(NINJA_RUN_TRAP),
      "Ninja Run Trap mission object 388",
    );
    only(
      await game.entities.byTemplate(DESTROY_NINJA_TRAP),
      "Destroy Ninja Trap mission object 389",
    );

    const before = await game.entities.detail(assassin.id);
    const hitPointsBefore = Number(prop(before, "HitPoints"));
    const initialDistance01 = horizontalDistance(before.position, waypoint01.position);
    assert.equal(prop(before, "AIBehavior"), "Idle");
    assert.ok(Number.isFinite(hitPointsBefore));

    // Stay at the untouched mission spawn, before proximity tripwire 378.
    // Damage is the production payload a successful weapon hit delivers; this
    // scenario deliberately isolates the script/trap chain from aiming and
    // projectile collision.
    await game.entities.sendMessage(assassin.id, { type: "Damage", amount: 1 });
    await game.step({ frames: 6 });

    const afterDamage = await game.entities.detail(assassin.id);
    assert.equal(Number(prop(afterDamage, "HitPoints")), hitPointsBefore - 1);
    assert.equal(
      prop(afterDamage, "AIBehavior"),
      "ScriptedSequence",
      "TriggerDamage must relay TurnOn to trap 388, which signals Ninja Run",
    );

    let closestDistance01 = initialDistance01;
    let signalTrapDestroyed = false;
    const samples: EntitySummary[] = [];

    // Follow the real two-waypoint sequence through its authored final Frob.
    // Sample coarsely because A* queries complete on a worker thread.
    for (let second = 0; second < 75; second += 1) {
      await game.step({ frames: 60 });
      const current = only(
        await game.entities.byTemplate(RED_ASSASSIN),
        "living Red Assassin mission object 254",
      );
      samples.push(current);
      closestDistance01 = Math.min(
        closestDistance01,
        horizontalDistance(current.position, waypoint01.position),
      );
      signalTrapDestroyed =
        (await game.entities.byTemplate(NINJA_RUN_TRAP)).length === 0;
      if (signalTrapDestroyed) break;
    }

    assert.ok(
      closestDistance01 < initialDistance01 - 1.5,
      `assassin must make progress toward Ninja Run 01; initial=${initialDistance01}, closest=${closestDistance01}, samples=${JSON.stringify(
        samples.map((sample) => sample.position),
      )}`,
    );
    assert.equal(
      signalTrapDestroyed,
      true,
      "the sequence's authored Destroy Ninja Trap Frob must consume trap 388",
    );
  },
);
