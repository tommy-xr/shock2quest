import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// Narration subtitles (the earth/station training voice-overs).
//
// A TrapSound TurnOn plays its narration AND posts the sample's transcript
// (25AE data: KEX `.sub` cues resolved through `loc_english.txt`). The text is
// laid out once on the shared 640x480 canvas (wrapped lines, bottom-center):
// flat renders that canvas in screen space, VR renders the same canvas on a
// world panel anchored in front of the head. Scene objects from either path
// carry the "subtitle" debug source, which is what these tests assert on.
//
// Negative-first: without the overlay no scene object ever reports the
// "subtitle" source, so every assertion below fails on the base revision.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// earth.mis mission-object id of the arrival PA Sound Trap (TrapSoundAmb +
// EarthText, PropObjectSound "trg0001" - the "Welcome to the Ramsey Center"
// narration, a 4-cue timed multisub ~12.2s long). Stable across launches as
// `template_id`; runtime entity ids are rediscovered every run.
const ARRIVAL_PA_SOUND_TRAP = 449;

async function fireArrivalNarration(game: GameServer): Promise<void> {
  const [trap] = await game.entities.byTemplate(ARRIVAL_PA_SOUND_TRAP);
  assert.ok(trap, `expected Sound Trap ${ARRIVAL_PA_SOUND_TRAP} in earth.mis`);
  await game.entities.sendMessage(trap.id, { type: "TurnOn" });
  await game.step({ frames: 30 });
}

test(
  "flat: a narration shows its subtitle lines and they expire with the cues",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8196),
    });
    await game.step({ frames: 30 });

    assert.equal(
      (await game.scene.fromSource("subtitle")).length,
      0,
      "no subtitle may show before any narration plays",
    );

    await fireArrivalNarration(game);

    const shown = await game.scene.fromSource("subtitle");
    assert.ok(
      shown.length >= 1,
      "the playing narration must draw its subtitle text",
    );

    // The narration sound plays alongside the text (text replaces nothing).
    const narrations = (await game.audio.recent()).sounds.filter((sound) =>
      sound.sample.startsWith("trg"),
    );
    assert.ok(
      narrations.length >= 1,
      `the narration audio must still play: ${JSON.stringify(narrations)}`,
    );

    // The trg0001 track totals ~12.2s of cues; well past that the overlay
    // must be gone (transient toast, not a sticky HUD element).
    await game.step({ frames: 13 * 60 });
    assert.equal(
      (await game.scene.fromSource("subtitle")).length,
      0,
      "subtitle lines must expire when their cues end",
    );
  },
);

test(
  "vr: the same narration presents the subtitle on a world panel near the head",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8196),
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    await fireArrivalNarration(game);

    const shown = await game.scene.fromSource("subtitle");
    assert.ok(
      shown.length >= 1,
      "VR must present the playing narration's subtitle",
    );

    // World-space panel, not a screen-space paste-over: the text hangs at the
    // frontend panel distance (2m) in front of the player, so its world
    // position is near the player - unlike flat's screen-space objects, which
    // sit at the identity transform.
    const player = await game.player.position();
    for (const object of shown) {
      const [x, , z] = object.position;
      const distance = Math.hypot(x - player.x, z - player.z);
      assert.ok(
        distance > 0.5 && distance < 4,
        `subtitle panel must hang near the player (got ${distance.toFixed(2)} ` +
          `at ${JSON.stringify(object.position)}, player ${JSON.stringify(player)})`,
      );
    }
  },
);
