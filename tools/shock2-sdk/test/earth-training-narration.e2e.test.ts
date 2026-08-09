import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end regression for the earth.mis training-area audio cacophony.
//
// Each training tripwire SwitchLinks to a Player Teleport Trap, an Inverter that
// fans out to ~11 Sound Traps (authored so ENTER's TurnOn inverts to TurnOff and
// SILENCES every narration), and a Trigger Delay that then plays the one local
// narration. The teleport yanks the player straight back out of the tripwire box,
// so Rapier reports an EXIT; the tripwire's inherited EXIT flag then sent TurnOff
// to all links, which the Inverter flipped into a TurnOn broadcast - every
// narration starting at once - and also re-fired the teleport trap.
//
// The fix suppresses the EXIT edge when the departure was caused by a scripted
// teleport (mirror of the existing arrival suppression) and makes
// TrapTeleportPlayer respond only to TurnOn.
//
// Negative-first: pre-fix this run logs 12 narrations (11 at once plus the
// intended one); post-fix exactly one plays.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Stable earth.mis mission-object id of a training tripwire that links a teleport
// trap + the silence Inverter + the narration Trigger Delay. Runtime entity ids
// are rediscovered every launch, never hardcoded.
const TRAINING_TRIPWIRE_OBJ = 378;

test(
  "earth training: entering a teleport tripwire plays exactly one narration",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8194),
    });
    await game.step({ frames: 30 });

    const [tripwire] = await game.entities.byTemplate(TRAINING_TRIPWIRE_OBJ);
    assert.ok(
      tripwire,
      `expected the earth.mis training tripwire for obj ${TRAINING_TRIPWIRE_OBJ}`,
    );

    const audioBefore = await game.audio.recent();
    const lastAudioSequence = audioBefore.sounds.at(-1)?.sequence ?? 0;
    const messagesBefore = await game.messages.recent();
    const lastMessageSequence = messagesBefore.messages.at(-1)?.sequence ?? 0;

    // Walk-in equivalent: drop the player inside the tripwire's box. A debug
    // teleport is locomotion, so ENTER fires exactly as it does on foot.
    await game.player.teleport({
      x: tripwire.position[0],
      y: tripwire.position[1],
      z: tripwire.position[2],
    });
    // Well past the narration Trigger Delay (~1s) so the intended clip has started.
    await game.step({ frames: 180 });

    const narrations = (await game.audio.recent()).sounds.filter(
      (sound) => sound.sequence > lastAudioSequence && sound.sample.startsWith("trg"),
    );
    assert.equal(
      narrations.length,
      1,
      `expected exactly one training narration, got ${narrations.length}: ` +
        JSON.stringify(narrations.map((s) => ({ sample: s.sample, frame: s.frame }))),
    );
    assert.equal(
      narrations[0]!.still_playing,
      true,
      `the intended narration must not be cut short: ${JSON.stringify(narrations[0])}`,
    );

    const traced = (await game.messages.recent()).messages.filter(
      (message) => message.sequence > lastMessageSequence,
    );

    // The Inverter's fanout is the burst: only the Trigger Delay's single
    // narration may TurnOn a Sound Trap. (ENTER's own TurnOn arrives at the
    // Sound Traps inverted, as TurnOff - that is the authored "silence all".)
    const soundTrapTurnOns = traced.filter(
      (message) => message.payload === "TurnOn" && message.to.name === "Sound Trap",
    );
    assert.equal(
      soundTrapTurnOns.length,
      1,
      `expected a single Sound Trap TurnOn, got ${soundTrapTurnOns.length}: ` +
        JSON.stringify(
          soundTrapTurnOns.slice(0, 5).map((m) => ({ frame: m.frame, to: m.to.template_id })),
        ),
    );

    // The stray EXIT edge went to the tripwire's own links, re-firing the
    // teleport trap. Nothing should reach it but the ENTER TurnOn.
    const teleportTrapMessages = traced.filter(
      (message) => message.to.name === "Player Teleport Trap",
    );
    assert.deepEqual(
      teleportTrapMessages.map((m) => m.payload),
      ["TurnOn"],
      `the teleport trap must only see the ENTER TurnOn: ` +
        JSON.stringify(teleportTrapMessages),
    );
  },
);
