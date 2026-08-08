import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Stable earth.mis mission-object id of a Sound Trap (S$TrapSound). Entity
// instantiation stores the positive mission-object id as PropTemplateId, so it
// is both what `entities.byTemplate` matches and what the diagnostics report as
// `template_id`. (dark_query prints the gamesys template, -1247, for the same
// object.) Runtime entity ids are rediscovered every launch, never hardcoded.
const SOUND_TRAP_OBJ = 449;

test(
  "earth: a sound trap's TurnOn shows up in both the message trace and the audio log",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8193),
    });
    await game.step({ frames: 30 });

    const [trap] = await game.entities.byTemplate(SOUND_TRAP_OBJ);
    assert.ok(trap, `expected an earth.mis Sound Trap for obj ${SOUND_TRAP_OBJ}`);

    const audioBefore = await game.audio.recent();
    const lastAudioSequence = audioBefore.sounds.at(-1)?.sequence ?? 0;

    await game.entities.sendMessage(trap.id, { type: "TurnOn" });
    await game.step({ frames: 5 });

    // The message trace must show the delivered TurnOn, stamped with a frame
    // and carrying the payload's sender.
    const traced = (await game.messages.recent()).messages.filter(
      (message) => message.payload === "TurnOn" && message.to.entity_id === trap.id,
    );
    assert.ok(
      traced.length > 0,
      `expected a traced TurnOn for the sound trap; got ${JSON.stringify(
        (await game.messages.recent()).messages.slice(-5),
      )}`,
    );
    const turnOn = traced.at(-1)!;
    assert.equal(turnOn.to.template_id, SOUND_TRAP_OBJ);
    assert.ok(turnOn.frame > 0, "traced messages must carry a 60 Hz frame");
    assert.ok(turnOn.from !== null, "TurnOn carries its sender");

    // ...and the resulting play must be in the audio log, with the timing and
    // provenance the diagnostics added.
    const played = (await game.audio.recent()).sounds.filter(
      (sound) =>
        sound.sequence > lastAudioSequence &&
        sound.source_entity?.template_id === SOUND_TRAP_OBJ,
    );
    assert.equal(
      played.length,
      1,
      `expected exactly one new sound-trap play; got ${JSON.stringify(played)}`,
    );
    const sound = played[0];
    assert.ok(sound.handle !== null, "plays carry their audio handle");
    assert.ok(
      sound.duration_secs !== null && sound.duration_secs > 1,
      `expected a decoded clip duration; got ${sound.duration_secs}`,
    );
    assert.ok(sound.frame >= turnOn.frame, "the play cannot precede its trigger");
    assert.equal(sound.still_playing, true, "a clip this long is still playing");
    assert.equal(sound.stopped_at_sim_time, null);

    // Stepping past the clip's duration retires it (derived from sim time, not
    // from live audio-device state).
    await game.step({ frames: Math.ceil(sound.duration_secs! * 60) + 60 });
    const afterEnd = (await game.audio.recent()).sounds.find(
      (entry) => entry.sequence === sound.sequence,
    );
    // The 64-entry ring may have evicted it while the mission ran; eviction is
    // not a failure, reporting it as still playing is.
    assert.notEqual(
      afterEnd?.still_playing,
      true,
      "the clip must retire once its duration elapsed",
    );

    // A TurnOff stops the trap: a fresh play is marked stopped even though its
    // duration has not elapsed.
    const beforeRestart = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
    await game.entities.sendMessage(trap.id, { type: "TurnOn" });
    await game.step({ frames: 5 });
    const restarted = (await game.audio.recent()).sounds.find(
      (entry) =>
        entry.sequence > beforeRestart &&
        entry.source_entity?.template_id === SOUND_TRAP_OBJ,
    )!;
    assert.ok(restarted, "the trap must play again on a second TurnOn");
    assert.equal(restarted.still_playing, true);

    await game.entities.sendMessage(trap.id, { type: "TurnOff" });
    await game.step({ frames: 5 });
    const stopped = (await game.audio.recent()).sounds.find(
      (entry) => entry.sequence === restarted.sequence,
    );
    assert.ok(
      stopped?.stopped_at_sim_time !== null && stopped?.stopped_at_sim_time !== undefined,
      `expected the stop to be recorded; got ${JSON.stringify(stopped)}`,
    );
    assert.equal(stopped?.still_playing, false);
  },
);
