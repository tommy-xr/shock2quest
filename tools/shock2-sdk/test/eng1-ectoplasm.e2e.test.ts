import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "eng1 Ectoplasm marker does not create a doorway-blocking collider",
  { skip: !e2eEnabled, timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "eng1.mis",
      port: 8464,
    });
    await game.step({ frames: 2 });

    // Runtime IDs change every launch. Mission object 181 is the durable
    // identity of the Ectoplasm marker that relays the nearby watch action to
    // Radapp; it has no model or authored physics of its own.
    const { entities } = await game.entities.list({ filter: "Ectoplasm" });
    const ectoplasm = entities.find((entity) => entity.template_id === 181);
    assert.ok(ectoplasm, "expected eng1 mission object 181 (Ectoplasm)");

    const { bodies } = await game.physics.bodies({ entityId: ectoplasm.id });
    assert.deepEqual(
      bodies,
      [],
      "script-only Ectoplasm marker must not obstruct the doorway",
    );
  },
);
