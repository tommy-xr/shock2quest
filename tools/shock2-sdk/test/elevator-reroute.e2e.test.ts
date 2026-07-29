import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary, Vec3 } from "../src/types.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";
const PORT = Number(process.env.SHOCK2_E2E_PORT ?? 8147);

function onlyEntity(entities: EntitySummary[], authoredId: number): EntitySummary {
  assert.equal(
    entities.length,
    1,
    `expected one runtime entity for command1 object ${authoredId}`,
  );
  return entities[0];
}

function assertAt(actual: Vec3, expected: Vec3, label: string): void {
  actual.forEach((value, index) => {
    assert.ok(
      Math.abs(value - expected[index]) < 0.05,
      `${label}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`,
    );
  });
}

test(
  "command1 call buttons reroute the tram to their ScriptParams stations",
  { skip: !e2eEnabled, timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "command1.mis",
      port: PORT,
    });

    // Runtime ids vary every launch; resolve all four mission objects from
    // their stable authored ids. Buttons 686/722/723 carry ScriptParams links
    // to path nodes 100/101/103 respectively and SwitchLink to tram 137.
    const tram = onlyEntity(await game.entities.byTemplate(137), 137);
    const button100 = onlyEntity(await game.entities.byTemplate(686), 686);
    const button101 = onlyEntity(await game.entities.byTemplate(722), 722);
    const button103 = onlyEntity(await game.entities.byTemplate(723), 723);

    assertAt(tram.position, [-377.53342, -15.8, 2.7231674], "initial station");

    // This must target 103 directly. Treating the frob as an ordinary TurnOn
    // would stop at the next sequential node, 101 (-184.43343).
    await game.entities.sendMessage(button103.id, { type: "Frob" });
    await game.step({ frames: 1_800 });
    assertAt(
      (await game.entities.detail(tram.id)).position,
      [-75.83342, -15.8, 2.7231674],
      "station 103",
    );

    await game.entities.sendMessage(button100.id, { type: "Frob" });
    await game.step({ frames: 1_800 });
    assertAt(
      (await game.entities.detail(tram.id)).position,
      [-377.53342, -15.8, 2.7231674],
      "station 100",
    );

    await game.entities.sendMessage(button101.id, { type: "Frob" });
    await game.step({ frames: 1_200 });
    assertAt(
      (await game.entities.detail(tram.id)).position,
      [-184.43343, -15.8, 2.7231674],
      "station 101",
    );
  },
);
