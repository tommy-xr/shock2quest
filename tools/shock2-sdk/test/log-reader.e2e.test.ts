import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";

// End-to-end test for the flat-mode audio-log/email reader MFD + persistent
// log collection (projects/flat-ui-panels.md §1, flat UI 6b).
//
// The honest-play payoff: the medsci1 Amanpour "New code" audio log (mission
// object id 1608, deck-2 log 20) must surface the Cryo Recovery A keypad code
// "45100" IN-FICTION - by reading the log's transcript off the reader panel,
// not by peeking at PropKeypadCode.
//
// Negative-first: on main, frobbing log 1608 plays LOG0220.wav and destroys
// the disc - /v1/ui shows no panel, no transcript exists anywhere, and
// /v1/info has no collected-log state, so every assertion below fails.
//
// Entity discovery is by stable template_id (1608 = the mission-file object
// id); runtime entity ids are NOT stable across launches.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "log reader MFD: frob opens transcript with 45100, collects the log, persists",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8166),
    });
    await game.step({ frames: 5 });

    // --- Nothing collected, no panel before the frob ---
    const before = await game.info();
    assert.deepEqual(
      before.player.collected_logs,
      [],
      "no logs should be collected at mission start",
    );
    assert.ok(
      !(await game.ui.state()).active_panel,
      "no MFD panel should be active before frobbing the log",
    );

    // --- Discover the Amanpour log by stable template id 1608 ---
    const logs = (await game.entities.list({ filter: "Audio Log", limit: 80 }))
      .entities;
    const amanpour = logs.find((e) => e.template_id === 1608);
    assert.ok(amanpour, "medsci1 should contain the Amanpour log (obj 1608)");

    // The disc rides in a corpse; stand next to it so the panel's walk-away
    // auto-close (4 units) doesn't fire.
    const detail = await game.entities.detail(amanpour.id);
    const [lx, ly, lz] = detail.position;
    await teleportVerified(game, { x: lx, y: ly + 0.5, z: lz });

    // --- Frob: the reader panel opens, bound to the disc ---
    await game.entities.sendMessage(amanpour.id, { type: "Frob" });
    await game.step({ frames: 5 });

    const opened = await game.ui.state();
    assert.ok(
      opened.active_panel,
      "frobbing the audio log should open the reader MFD panel",
    );
    assert.equal(
      opened.active_panel.template_id,
      1608,
      "the reader panel should be bound to the Amanpour log entity",
    );

    // The transcript must surface the code 45100 in-fiction.
    const textOf = (p: { elements: { kind: string; text: string | null }[] }) =>
      p.elements
        .filter((e) => e.kind === "text" && e.text)
        .map((e) => e.text)
        .join(" ");
    const transcript = textOf(opened.active_panel);
    assert.ok(
      transcript.includes("45100"),
      `the reader transcript should contain the code 45100 (got: ${transcript.slice(0, 200)})`,
    );
    // The .str escapes must be unescaped - no literal backslash-n on screen.
    assert.ok(
      !transcript.includes("\\n"),
      "the transcript must not render literal \\n escape sequences",
    );
    // Header identifies the sender.
    assert.ok(
      transcript.toUpperCase().includes("AMANPOUR"),
      "the reader header should name the sender",
    );

    // Reader art: LOG backdrop + sender portrait + deck icon (book.crf mount).
    const textures = opened.active_panel.elements
      .filter((e) => e.kind === "image")
      .map((e) => e.texture?.toLowerCase());
    assert.ok(textures.includes("log.pcx"), "reader should draw the LOG backdrop");
    assert.ok(
      textures.includes("amanpour.pcx"),
      "reader should draw the sender portrait from book.crf",
    );
    assert.ok(
      textures.includes("medicon.pcx"),
      "reader should draw the deck icon from book.crf",
    );
    await game.screenshot("log-reader-open.png");

    // --- The audio actually resolved and played (LOG0220) ---
    const sounds = (await game.audio.recent()).sounds.map((s) =>
      s.sample.toLowerCase(),
    );
    assert.ok(
      sounds.includes("log0220"),
      `frobbing the log should play LOG0220 (recent: ${sounds.join(", ")})`,
    );

    // --- The log is recorded in the persistent collection ---
    const collected = (await game.info()).player.collected_logs;
    assert.deepEqual(
      collected,
      [{ deck: 2, log: 20 }],
      "the Amanpour log should be recorded in the collection",
    );

    // Re-frobbing does not duplicate the collection entry.
    await game.entities.sendMessage(amanpour.id, { type: "Frob" });
    await game.step({ frames: 5 });
    assert.equal(
      (await game.info()).player.collected_logs.length,
      1,
      "re-frobbing the same log must not duplicate the collection entry",
    );

    // --- Email trap: TurnOn plays EM0201 once (deduped on replay) ---
    const traps = (await game.entities.list({ filter: "EmailTrap", limit: 20 }))
      .entities;
    const emailTrap = traps.find((e) => e.template_id === 1131);
    assert.ok(emailTrap, "medsci1 should contain EmailTrap 1131 (EM0201)");
    await game.entities.sendMessage(emailTrap.id, { type: "TurnOn" });
    await game.step({ frames: 5 });
    const afterEmail = (await game.audio.recent()).sounds.filter(
      (s) => s.sample.toLowerCase() === "em0201",
    );
    assert.equal(afterEmail.length, 1, "EM0201 should play on TurnOn");

    // --- The collection survives save/load ---
    await game.save("log-reader-e2e");
    await game.step({ frames: 2 });
    await game.load("log-reader-e2e");
    await game.step({ frames: 5 });
    assert.deepEqual(
      (await game.info()).player.collected_logs,
      [{ deck: 2, log: 20 }],
      "the log collection should survive save/load",
    );
  },
);
