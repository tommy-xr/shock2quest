import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test for TrapEmail granting its authored objective (GitHub #556).
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
//
// Negative-first: TrapEmail read only PropLog, so it played the email audio but
// never applied the entity's PropQuestBitName/PropQuestBitValue pair. On ops1
// that meant Note_4_2 ("reprogram the three Simulation Units") - the starting
// point of the whole Operations objective chain, with no other grantor anywhere
// in ops1-ops4 - was never handed out.
//
// The trap stays once-only (it consumes itself): the tripwires feeding these
// traps fire on every crossing, and re-applying the quest bit would knock an
// already-COMPLETE objective back to INCOMPLETE.
//
// The authored chain is Tripwire 277 -> QB Filter 278 (on ShodanRoom) ->
// EmailTrap 279. Discover entities by their stable mission object id
// (`template_id`), never by runtime entity id.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "ops1: the email trap grants Note_4_2 and is consumed once fired",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "ops1.mis",
    });
    await game.step({ frames: 5 });

    assert.equal(
      await game.quests.get("note_4_2"),
      "unknown",
      "the Sim Unit objective should not be granted before the trap fires",
    );

    // The QB Filter ahead of the trap only passes after the SHODAN reveal.
    await game.quests.set("ShodanRoom", "complete");

    const filters = await game.entities.byTemplate(278);
    assert.equal(
      filters.length,
      1,
      `expected the ops1 QB Filter (object 278), got ${JSON.stringify(filters.map((f) => f.name))}`,
    );

    await game.entities.sendMessage(filters[0].id, { type: "TurnOn" });
    await game.step({ frames: 30 });

    assert.equal(
      await game.quests.get("note_4_2"),
      "incomplete",
      "the email trap should grant Note_4_2 as an active (INCOMPLETE) objective",
    );

    const emails = (await game.audio.recent()).sounds.filter((sound) =>
      sound.tags.some(([key, value]) => key === "kind" && value === "email"),
    );
    assert.equal(
      emails.length,
      1,
      `the email should play exactly once, got ${JSON.stringify(emails)}`,
    );

    // The trap is consumed, so re-crossing the tripwire later - after the
    // player has actually completed the Sim Unit objective on ops3/ops4 -
    // cannot downgrade it back to INCOMPLETE.
    assert.equal(
      (await game.entities.byTemplate(279)).length,
      0,
      "the email trap should be consumed once it fires",
    );
    await game.quests.set("note_4_2", "complete");
    await game.entities.sendMessage(filters[0].id, { type: "TurnOn" });
    await game.step({ frames: 30 });
    assert.equal(
      await game.quests.get("note_4_2"),
      "complete",
      "re-triggering the trap chain must not knock a completed objective " +
        "back to incomplete",
    );
  },
);
