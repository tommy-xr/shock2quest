import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test for a log disc granting its authored objective (GitHub #568).
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
//
// Negative-first: MediaGui::on_frob emitted CollectLog + PlaySound + the
// switch-link relay, but never applied the disc's PropQuestBitName /
// PropQuestBitValue pair. ops3's Bronson log (object 1349 -> Note_4_6) has zero
// links in either direction, so the quest-bit pair on the disc is the
// objective's only grantor - it could never be handed out.
//
// Re-reading must NOT re-apply it: SetQuestBit overwrites, so the authored
// INCOMPLETE would knock an objective the player has since COMPLETEd back to
// incomplete. The application is gated by the same `already_collected` one-shot
// that guards the switch-link send.
//
// The disc is discovered by its stable mission object id (`template_id`), never
// by runtime entity id.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "ops3: reading the Bronson log grants Note_4_6, and re-reading cannot downgrade it",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "ops3.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8221),
    });
    await game.step({ frames: 5 });

    assert.equal(
      await game.quests.get("note_4_6"),
      "unknown",
      "the objective should not be granted before the log is read",
    );

    const discs = await game.entities.byTemplate(1349);
    assert.equal(
      discs.length,
      1,
      `expected the ops3 Bronson log (object 1349), got ${JSON.stringify(discs.map((d) => d.name))}`,
    );

    await game.entities.sendMessage(discs[0].id, { type: "Frob" });
    await game.step({ frames: 30 });

    assert.equal(
      await game.quests.get("note_4_6"),
      "incomplete",
      "reading the log should grant Note_4_6 as an active (INCOMPLETE) objective",
    );

    // Once the player has actually completed the objective, a direct debug
    // replay against the hidden reader-backing entity must leave it complete.
    await game.quests.set("note_4_6", "complete");
    await game.entities.sendMessage(discs[0].id, { type: "Frob" });
    await game.step({ frames: 30 });
    assert.equal(
      await game.quests.get("note_4_6"),
      "complete",
      "re-reading a collected log must not knock a completed objective back to incomplete",
    );
  },
);
