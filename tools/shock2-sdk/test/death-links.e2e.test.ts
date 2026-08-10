import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test against a real debug runtime. Requires game assets in
// Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "a killed droid explodes into its Corpse link and leaves no body",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8131),
    });

    await game.step({ frames: 10 });

    // earth.mis's training droids inherit template -4015 "Training Droid",
    // which links Corpse -> -2912 "HE_Harmless". Runtime entity IDs are
    // assigned afresh every launch, so discover the droid by name.
    const droids = await game.entities.list({
      filter: "Training Droid",
      limit: 50,
    });
    const droid = droids.entities.find(
      (entity) => entity.name === "Training Droid",
    );
    assert.ok(droid, "earth.mis should contain a Training Droid");

    const before = await game.entities.list({ limit: 2000 });
    const knownIds = new Set(before.entities.map((entity) => entity.id));

    await game.entities.sendMessage(droid.id, { type: "Damage", amount: 100 });
    await game.step({ frames: 30 });

    const after = await game.entities.list({ limit: 2000 });
    const spawned = after.entities.filter(
      (entity) => !knownIds.has(entity.id),
    );
    assert.ok(
      spawned.some((entity) => entity.template_id === -2912),
      `expected the linked HE_Harmless explosion, got ${JSON.stringify(spawned)}`,
    );
    assert.ok(
      !after.entities.some((entity) => entity.id === droid.id),
      "the exploded droid must be removed, not left as a corpse",
    );
  },
);

test(
  "a killed organic still leaves a corpse",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8131),
    });

    await game.step({ frames: 10 });

    // Spawn a hybrid (OG-Pipe): organics author no Corpse/Flinderize links,
    // so they must keep crumpling into a persistent body. Diff the list -
    // medsci1 has native OG-Pipes too.
    const preSpawn = await game.entities.list({ filter: "OG-Pipe", limit: 50 });
    const known = new Set(preSpawn.entities.map((entity) => entity.id));
    await game.input.trigger("SpawnDebugMonster");
    await game.step({ frames: 30 });
    const postSpawn = await game.entities.list({ filter: "OG-Pipe", limit: 50 });
    const monster = postSpawn.entities.find(
      (entity) => entity.name === "OG-Pipe" && !known.has(entity.id),
    );
    assert.ok(monster, "expected a newly spawned OG-Pipe");

    await game.entities.sendMessage(monster.id, {
      type: "Damage",
      amount: 1000,
    });
    await game.step({ frames: 120 });

    const detail = await game.entities.detail(monster.id);
    assert.equal(
      detail.properties.find((p) => p.name === "AIBehavior")?.value,
      "Dead",
      "an organic should crumple into a persistent corpse",
    );
  },
);
