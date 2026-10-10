import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, type PlayedSound } from "../src/index.js";
import { crossEarthTrainingTripwire } from "./helpers/earth-tripwire.js";

// Keep this with trap-sound-amb.e2e: solving Earth by spatializing every
// announcement made remote Xerxes messages inaudible (#870 / #1685).
// Assert the whole transition's playback intervals, not a single play count
// or source position. A late basic timer must not interrupt advanced speech.
for (const vr of [false, true]) {
  for (const dwell of [0, 60]) {
    test(`Earth briefing transition: no stale speech (${vr ? "VR" : "flat"}, ${dwell} lobby frames)`, {
      skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
    }, async () => {
      await using game = await GameServer.launch({
        mission: "earth.mis", debugFlags: vr ? ["--vr"] : [],
      });
      await game.step({ frames: 30 });
      await crossEarthTrainingTripwire(game, 378, 377);
      await game.step({ frames: 180 });
      assert.ok((await game.audio.recent()).sounds.some(s => s.sample === "trg0006" && s.still_playing));
      await crossEarthTrainingTripwire(game, 380, 379);
      if (dwell) await game.step({ frames: dwell });

      // Stage inside the closed door, then cross sensor 476 under collision.
      // Zero dwell deliberately stresses the exit's 0.33s pending start; this
      // is a bounded trigger test, not a claim to walk the corridor that fast.
      await game.player.teleport({ x: 0, y: 24.5, z: 51 });
      await game.step({ frames: 3 });
      await game.player.moveTo({ x: 4.5, y: 24.5, z: 51 });

      const sounds = new Map<number, PlayedSound>();
      // Poll to retain entries before ring eviction, past the entire 33s
      // delayed montage clip. Include explicitly stopped plays in the check.
      for (let i = 0; i < 180; i++) {
        await game.step({ frames: 15 });
        for (const sound of (await game.audio.recent()).sounds) {
          if (sound.sample.startsWith("trg")) sounds.set(sound.sequence, sound);
        }
      }
      const plays = [...sounds.values()];
      const advanced = plays.filter(s => s.sample === "trg0014");
      assert.equal(advanced.length, 1, "must cross the real advanced entrance and play its briefing once");
      assert.equal(advanced[0].stopped_at_sim_time, null, "a stale timer must not cut the advanced briefing short");
      assert.ok(advanced[0].pan_applied, "ambient training speech must remain listener-relative");
      assert.equal(plays.filter(s => s.sample === "trg0007").length, 0,
        "leaving basic training must invalidate its pending sound start");
      for (let i = 0; i < plays.length; i++) {
        const a = plays[i];
        assert.ok(a.duration_secs !== null, "overlap checks require decoded durations");
        const aEnd = Math.min(a.sim_time + a.duration_secs!, a.stopped_at_sim_time ?? Infinity);
        for (const b of plays.slice(i + 1)) {
          assert.ok(b.duration_secs !== null);
          const bEnd = Math.min(b.sim_time + b.duration_secs!, b.stopped_at_sim_time ?? Infinity);
          assert.ok(Math.min(aEnd, bEnd) <= Math.max(a.sim_time, b.sim_time),
            `briefings overlap: ${JSON.stringify([a, b])}`);
        }
      }
    });
  }
}
