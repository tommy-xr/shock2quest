import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { crossEarthTrainingTripwire } from "./helpers/earth-tripwire.js";
import { teleportVerified } from "./helpers/teleport.js";

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

// Stable earth.mis mission-object ids (the runtime reports these as
// `template_id`, which is why they are the argument to `byTemplate` - runtime
// entity ids are rediscovered every launch, never hardcoded). 378 is a training
// tripwire linking the teleport trap 377, the silence Inverter, and the
// narration Trigger Delay; 377's own position is the teleport destination pad.
const TRAINING_TRIPWIRE_OBJ = 378;
const TELEPORT_TRAP_OBJ = 377;

test(
  "earth training: entering a teleport tripwire plays exactly one narration",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
    });
    await game.step({ frames: 30 });

    const audioBefore = await game.audio.recent();
    const lastAudioSequence = audioBefore.sounds.at(-1)?.sequence ?? 0;
    const messagesBefore = await game.messages.recent();
    const lastMessageSequence = messagesBefore.messages.at(-1)?.sequence ?? 0;

    // Enter the sensor under collision (real SensorBeginIntersect path) and
    // verify the linked teleport actually fired.
    await crossEarthTrainingTripwire(
      game,
      TRAINING_TRIPWIRE_OBJ,
      TELEPORT_TRAP_OBJ,
    );
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

    // Direct coverage of the TurnOn-only gate: a TurnOff injected into the
    // teleport trap must NOT move the player (pre-fix it teleported on any
    // message); a TurnOn must.
    const [teleportTrap] = await game.entities.byTemplate(TELEPORT_TRAP_OBJ);
    assert.ok(teleportTrap, `expected teleport trap ${TELEPORT_TRAP_OBJ}`);
    const [padX, padY, padZ] = teleportTrap.position;
    await teleportVerified(game, { x: padX, y: padY + 0.5, z: padZ - 8 });
    const parked = await game.player.position();

    await game.entities.sendMessage(teleportTrap.id, { type: "TurnOff" });
    await game.step({ frames: 30 });
    const afterTurnOff = await game.player.position();
    assert.ok(
      Math.hypot(afterTurnOff.x - parked.x, afterTurnOff.z - parked.z) < 1,
      `TurnOff must not teleport the player: parked=${JSON.stringify(parked)} ` +
        `after=${JSON.stringify(afterTurnOff)}`,
    );

    await game.entities.sendMessage(teleportTrap.id, { type: "TurnOn" });
    await game.step({ frames: 30 });
    const afterTurnOn = await game.player.position();
    assert.ok(
      Math.hypot(afterTurnOn.x - padX, afterTurnOn.z - padZ) < 3,
      `TurnOn must teleport the player to the pad: pad=[${padX},${padY},${padZ}] ` +
        `after=${JSON.stringify(afterTurnOn)}`,
    );
  },
);
