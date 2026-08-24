import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";

// End-to-end test for creature-inventory looting (`creaturecontainer`, #519).
//
// Every creature carries a `creaturecontainer` script, which was a NoopScript -
// so a slain creature's inventory could never be looted (unless the creature
// also happened to inherit the always-open `containerscript`). This wires
// `creaturecontainer` to the loot MFD behind a faithful gate:
//
//   a creature's inventory opens on frob only when it is a corpse
//   (dead/incapacitated) OR an invulnerable, posed non-combatant. A live,
//   killable hostile stays sealed until slain.
//
// PRIMARY negative-first subject: OG-Pipe (mission object 596), a live Hybrid
// that has `creaturecontainer` but NOT `containerscript`, Contains an "OG
// Organ". While alive it must NOT be loot-frobbable; once slain its inventory
// opens. With `creaturecontainer` mapped to NoopScript the DEAD frob opens no
// panel - that assertion is the red-before/green-after for this change.
//
// PLAYTHROUGH scenario: Dr. Watts (734), an Invulnerable/Posing Male-MedSci
// NPC, Contains the deck-2 log-14 disc whose transcript carries the conduit
// access code 12451 (level02.str LogText14). Frobbing him opens his loot MFD so
// the disc can be collected and read on demand. (Watts also inherits the
// always-open `containerscript` from the root Object template, so his loot works
// independently of this fix; the gated `creaturecontainer` recognises him as
// invulnerable and opens too, coexisting without breaking his loot.)
//
// Discovery is by stable mission object id (the `template_id` /v1/entities
// reports for level-authored entities); runtime entity ids are NOT stable.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const OG_PIPE = 596; // live Hybrid: creaturecontainer, NO containerscript, Contains OG Organ
const WATTS = 734; // Invulnerable/Posing Male-MedSci NPC, Contains the code disc
const CODE_DISC = 251; // Audio Log, deck 2 log 14 -> conduit code 12451

test(
  "creature loot: a slain creature is lootable, a live hostile is not, and Watts yields the 12451 disc",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
    });
    await game.step({ frames: 5 });

    const byTemplateOne = async (templateId: number, what: string) => {
      const matches = await game.entities.byTemplate(templateId);
      assert.equal(matches.length, 1, `expected exactly one ${what} (mission id ${templateId})`);
      return matches[0];
    };
    const containsLinks = async (entityId: number) =>
      (await game.entities.detail(entityId)).outgoing_links.filter((l) =>
        l.link_type.startsWith("Contains"),
      );
    const activePanel = async () => (await game.ui.state()).active_panel;

    // ============================================================
    // FAITHFULNESS GATE + red-before/green-after: OG-Pipe (596)
    // ============================================================
    const pipe = await byTemplateOne(OG_PIPE, "OG-Pipe (live Hybrid)");
    const organLinks = await containsLinks(pipe.id);
    assert.equal(organLinks.length, 1, "OG-Pipe should contain exactly its OG Organ");
    const organId = organLinks[0].target_id;

    await teleportVerified(game, {
      x: pipe.position[0] + 1.3,
      y: pipe.position[1] + 0.5,
      z: pipe.position[2] + 1.3,
    });

    // --- (a) ALIVE: a live, killable hostile must NOT be loot-frobbable ---
    await game.entities.sendMessage(pipe.id, { type: "Frob" });
    await game.step({ frames: 5 });
    assert.equal(
      await activePanel(),
      null,
      "frobbing a LIVE hostile creature must not open a loot panel",
    );

    // --- Slay it (lethal damage drops it to a corpse) ---
    for (let i = 0; i < 8; i++) {
      await game.entities.sendMessage(pipe.id, { type: "Damage", amount: 50 });
      await game.step({ frames: 3 });
    }
    await game.step({ frames: 20 });

    // --- (b) DEAD: the corpse's inventory now opens (RED on Noop) ---
    await game.entities.sendMessage(pipe.id, { type: "Frob" });
    await game.step({ frames: 5 });
    const corpsePanel = await activePanel();
    assert.ok(
      corpsePanel,
      "frobbing the SLAIN creature should open its loot MFD (creaturecontainer wired)",
    );
    assert.equal(
      corpsePanel.entity_id,
      pipe.id,
      "the loot panel should be bound to the slain creature",
    );
    assert.ok(
      corpsePanel.elements.some((e) => e.kind === "button" && e.entity_id === organId),
      `the corpse loot panel should list the OG Organ ` +
        `(got ${JSON.stringify(corpsePanel.elements)})`,
    );
    await game.screenshot("creature-corpse-loot.png");
    // Close the panel before moving on (bare-view click dismisses it).
    await game.input.set("pointer.pressed", 1);
    await game.step({ frames: 2 });
    await game.input.set("pointer.pressed", 0);
    await game.step({ frames: 2 });

    // ============================================================
    // PLAYTHROUGH: Dr. Watts -> the 12451 disc
    // ============================================================
    const watts = await byTemplateOne(WATTS, "Dr. Watts");
    const contained = (await containsLinks(watts.id)).map((l) => l.target_id);
    let codeDiscId: number | undefined;
    for (const id of contained) {
      if ((await game.entities.detail(id)).template_id === CODE_DISC) codeDiscId = id;
    }
    assert.ok(codeDiscId, `Watts should contain the code disc (mission id ${CODE_DISC})`);

    await teleportVerified(game, {
      x: watts.position[0] + 1.0,
      y: watts.position[1] + 0.5,
      z: watts.position[2] + 1.0,
    });
    await game.step({ frames: 5 });
    await game.entities.sendMessage(watts.id, { type: "Frob" });
    await game.step({ frames: 5 });

    const wattsPanel = await activePanel();
    assert.ok(
      wattsPanel && wattsPanel.entity_id === watts.id,
      `frobbing invulnerable/posed Watts should open his loot MFD (got ${JSON.stringify(wattsPanel)})`,
    );
    const discElement = wattsPanel.elements.find(
      (e) => e.kind === "button" && e.entity_id === codeDiscId,
    );
    assert.ok(
      discElement,
      `Watts' loot panel should list the code disc (got ${JSON.stringify(wattsPanel.elements)})`,
    );
    await game.screenshot("watts-loot-open.png");

    // Take the disc from the panel: an audio log is use-only, so the click
    // frobs it, recording deck-2 log 14 - the 12451 transcript - into the
    // persistent collection.
    const [x, y, w, h] = discElement.screen_rect;
    await game.input.set("pointer.position", [x + w / 2, y + h / 2]);
    await game.step({ frames: 2 });
    await game.input.set("pointer.pressed", 1);
    await game.step({ frames: 2 });
    await game.input.set("pointer.pressed", 0);
    await game.step({ frames: 5 });
    assert.ok(
      (await game.info()).player.collected_logs.some((c) => c.deck === 2 && c.log === 14),
      "taking Watts' disc should record deck-2 log 14 (the 12451 code log)",
    );

    // The disc's transcript surfaces 12451 in-fiction. Read it through the
    // original's on-demand `play_unread_log` path at the disc's own authored
    // position (the consumed entity remains as the reader backing state;
    // standing at Watts is >4 units away, so walk-away auto-close would fire).
    const dp = (await game.entities.detail(codeDiscId)).position;
    await teleportVerified(game, { x: dp[0], y: dp[1] + 0.5, z: dp[2] });
    await game.input.trigger("ReadLastUnreadLog");
    await game.step({ frames: 5 });
    const reader = await activePanel();
    assert.equal(
      reader?.template_id,
      CODE_DISC,
      "reading the looted disc should open the reader MFD bound to it",
    );
    const transcript = reader.elements
      .filter((e) => e.kind === "text" && e.text)
      .map((e) => e.text)
      .join(" ");
    assert.ok(
      transcript.includes("12451"),
      `the disc transcript should reveal the conduit code 12451 (got: ${transcript.slice(0, 200)})`,
    );
    await game.screenshot("watts-disc-read.png");
  },
);
