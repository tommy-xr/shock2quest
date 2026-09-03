import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test for TrapMessage (GitHub #1029).
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
//
// Negative-first: `trapmessage` was a no-op script and `P$UseMsg` was not even
// parsed, so pressing eng2's broken lift buttons told the player nothing.
//
// eng2 wiring (stable mission object ids, reported as `template_id`):
//   Elevator buttons 445/446/447 -> SwitchLink -> Message Trap 689
//   Message Trap 689: P$UseMsg "OutOfOrder" -> USEMSG.STR
// Runtime entity ids are not stable across runs, so the trap is discovered by
// mission object id each launch.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const MESSAGE_TRAP = 689;
const EXPECTED = "This lift has been taken offline for repairs.";

test(
  "eng2: the broken lift's message trap puts its UseMsg text on the HUD",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "eng2.mis" });
    await game.step({ frames: 10 });

    // Nothing has fired yet, so the message channel is empty.
    assert.deepEqual((await game.ui.state()).messages, []);

    const found = await game.entities.byTemplate(MESSAGE_TRAP);
    assert.equal(
      found.length,
      1,
      `expected exactly one eng2 Message Trap ${MESSAGE_TRAP}, got ${found.length}`,
    );

    // The buttons switch-link into the trap; TurnOn is what they send.
    await game.entities.sendMessage(found[0].id, { type: "TurnOn" });
    await game.step({ frames: 10 });

    assert.deepEqual(
      (await game.ui.state()).messages,
      [EXPECTED],
      "the trap's P$UseMsg key should resolve against USEMSG.STR",
    );

    // The line expires five seconds after it was shown.
    await game.step({ frames: 4 * 60 });
    assert.deepEqual(
      (await game.ui.state()).messages,
      [EXPECTED],
      "the message should still be up four seconds in",
    );
    await game.step({ frames: 2 * 60 });
    assert.deepEqual(
      (await game.ui.state()).messages,
      [],
      "the message should clear after its five-second window",
    );
  },
);
