import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test for ObjConsumeButton's locked message (`P$LockMsg`).
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
//
// Negative-first: `P$LockMsg` was not parsed, so frobbing medsci1's empty aux
// power slot without a power cell told the player nothing.
//
// medsci1 wiring (stable mission object id, reported as `template_id`):
//   Aux Power W/Out Battery 1128: ObjConsumeButton, P$ConsumeType "Power Cell",
//   P$LockMsg AuxPower -> LOCKMSG.STR
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const AUX_POWER = 1128;
const EXPECTED = "Insert charged power cell.";

test(
  "medsci1: frobbing the aux power slot without a power cell shows its LockMsg",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "medsci1.mis" });
    await game.step({ frames: 10 });
    assert.deepEqual((await game.ui.state()).messages, []);

    const found = await game.entities.byTemplate(AUX_POWER);
    assert.equal(found.length, 1, `expected one aux power slot ${AUX_POWER}, got ${found.length}`);

    await game.entities.sendMessage(found[0].id, { type: "Frob" });
    await game.step({ frames: 10 });

    assert.deepEqual(
      (await game.ui.state()).messages,
      [EXPECTED],
      "the slot's P$LockMsg key should resolve against LOCKMSG.STR",
    );
  },
);
