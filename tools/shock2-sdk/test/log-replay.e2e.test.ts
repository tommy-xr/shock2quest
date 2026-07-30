import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, type Game } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";
const PLAY_UNREAD_LOG = "PlayUnreadLog";

const textOf = (panel: {
  elements: { kind: string; text: string | null }[];
}) =>
  panel.elements
    .filter((element) => element.kind === "text" && element.text)
    .map((element) => element.text)
    .join(" ");

async function closePanel(game: Game) {
  const close = (await game.ui.state()).active_panel?.elements.find(
    (element) => element.label === "close",
  );
  assert.ok(close, "the reader should expose the host close button");
  const [x, y, width, height] = close.screen_rect;
  await game.input.set("pointer.position", [x + width / 2, y + height / 2]);
  await game.input.set("pointer.pressed", 0);
  await game.step({ frames: 1 });
  await game.input.set("pointer.pressed", 1);
  await game.step({ frames: 1 });
  await game.input.set("pointer.pressed", 0);
  await game.step({ frames: 1 });
}

test(
  "PlayUnreadLog is a safe no-op when no audio log has been collected",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "eng2.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8188),
    });
    await game.step({ frames: 5 });

    assert.deepEqual((await game.info()).player.collected_logs, []);
    assert.ok(
      (await game.input.actions()).includes(PLAY_UNREAD_LOG),
      "the normal player log-replay action should be registered",
    );

    const priorAudioSequence =
      (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
    await game.input.trigger(PLAY_UNREAD_LOG);
    await game.step({ frames: 5 });

    assert.equal(
      (await game.ui.state()).active_panel,
      null,
      "an empty collection must not open a blank media panel",
    );
    assert.ok(
      (await game.audio.recent()).sounds.every(
        (sound) => sound.sequence <= priorAudioSequence,
      ),
      "an empty collection must not play fabricated media",
    );
  },
);

test(
  "PlayUnreadLog reopens the latest authentic log after a mission transition",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8189),
    });
    await game.step({ frames: 5 });

    // Collect the real Amanpour "New code" disc through its production frob
    // path. Runtime ids are unstable, so identify it by stable mission object
    // template_id 1608.
    const logs = (await game.entities.list({ filter: "Audio Log", limit: 80 }))
      .entities;
    const amanpour = logs.find((entity) => entity.template_id === 1608);
    assert.ok(amanpour, "medsci1 should contain Amanpour log object 1608");
    const detail = await game.entities.detail(amanpour.id);
    const [x, y, z] = detail.position;
    await teleportVerified(game, { x, y: y + 0.5, z });
    await game.entities.sendMessage(amanpour.id, { type: "Frob" });
    await game.step({ frames: 5 });
    assert.deepEqual((await game.info()).player.collected_logs, [
      { deck: 2, log: 20 },
    ]);
    await closePanel(game);

    // Leave the source mission, then save/load on the destination. The physical
    // disc and its entity-bound RuntimePropLogData no longer exist here; only
    // the authentic persisted (deck, log) identity crosses this boundary.
    await game.transitionLevel("eng2.mis");
    await game.step({ frames: 5 });
    assert.equal((await game.info()).mission, "eng2.mis");
    await game.save("log-replay-e2e");
    await game.load("log-replay-e2e");
    await game.step({ frames: 5 });
    assert.deepEqual((await game.info()).player.collected_logs, [
      { deck: 2, log: 20 },
    ]);
    assert.equal((await game.ui.state()).active_panel, null);

    const priorAudioSequence =
      (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
    await game.input.trigger(PLAY_UNREAD_LOG);
    await game.step({ frames: 5 });

    const panel = (await game.ui.state()).active_panel;
    assert.ok(panel, "U should open the collected log reader on another deck");
    assert.equal(
      panel.template_id,
      -1,
      "replay should use the synthetic player-owned panel, not a stale disc id",
    );
    const transcript = textOf(panel);
    assert.ok(transcript.includes("45100"), transcript);
    assert.ok(transcript.toUpperCase().includes("AMANPOUR"), transcript);

    const textures = panel.elements
      .filter((element) => element.kind === "image")
      .map((element) => element.texture?.toLowerCase());
    assert.ok(textures.includes("iface/log.pcx"));
    assert.ok(textures.includes("amanpour.pcx"));
    assert.ok(textures.includes("medicon.pcx"));

    const replayed = (await game.audio.recent()).sounds.filter(
      (sound) =>
        sound.sequence > priorAudioSequence &&
        sound.sample.toLowerCase() === "log0220",
    );
    assert.equal(replayed.length, 1, "U should replay authentic LOG0220 audio");
    await game.screenshot("log-replay-cross-mission.png");
  },
);
