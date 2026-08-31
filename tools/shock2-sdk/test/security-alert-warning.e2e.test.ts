import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Stable medsci1 mission object ids: camera 68 SwitchLinks to security
// ecology 71 (see security-ecology.e2e.test.ts for the full authored chain).
const CAMERA = 68;
const ECOLOGY = 71;

/** Xerxes' "Potential threat detected." - the security-alert warning. */
const WARNING = "xxyal001";

function property(detail: EntityDetailResult, name: string): string | undefined {
  return detail.properties.find((candidate) => candidate.name === name)?.value;
}

test(
  "an alerted security ecology loops the warning, and clearing it goes quiet",
  { skip: !e2eEnabled, timeout: 900_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "medsci1.mis" });
    await game.step({ frames: 10 });

    const [camera] = await game.entities.byTemplate(CAMERA);
    const [ecology] = await game.entities.byTemplate(ECOLOGY);
    assert.ok(camera && ecology, "medsci1 must have its authored camera + ecology");

    // `/v1/audio/recent` is a small ring buffer that busy levels overrun, so
    // drain it after every short step rather than once at the end.
    let warnings = 0;
    // Sounds linger in the buffer across polls, so only count ones newer than
    // everything already seen.
    let lastSequence = 0;
    const stepAndListen = async (frames: number, chunk = 60) => {
      for (let done = 0; done < frames; done += chunk) {
        await game.step({ frames: Math.min(chunk, frames - done) });
        const { sounds } = await game.audio.recent();
        for (const sound of sounds) {
          if (sound.sequence <= lastSequence) continue;
          lastSequence = sound.sequence;
          if (sound.sample === WARNING) warnings += 1;
        }
      }
    };

    await stepAndListen(60);
    assert.equal(
      property(await game.entities.detail(ecology.id), "EcologyState"),
      "Normal",
      "the ecology starts calm",
    );
    assert.equal(warnings, 0, "a calm station must not warn");

    // Force the camera to identify a threat; CameraAlert raises the ecology.
    await game.entities.sendMessage(camera.id, {
      type: "SetAlertness",
      level: "High",
    });
    await stepAndListen(30, 15);
    assert.equal(
      property(await game.entities.detail(ecology.id), "EcologyState"),
      "Alert",
      "the camera alarm should raise its linked ecology",
    );
    assert.ok(warnings >= 1, "the alert should speak the warning immediately");

    // It repeats while the alert holds.
    await stepAndListen(15 * 60);
    assert.ok(
      warnings >= 2,
      `the warning should repeat while alerted, heard ${warnings}`,
    );

    // Run out the authored 120s recovery, then confirm the station goes quiet.
    for (let poll = 0; poll < 20; poll += 1) {
      if (property(await game.entities.detail(ecology.id), "EcologyState") !== "Alert") {
        break;
      }
      await game.step({ frames: 600 });
    }
    assert.equal(
      property(await game.entities.detail(ecology.id), "EcologyState"),
      "Normal",
      "authored recovery should clear the alert",
    );

    warnings = 0;
    await stepAndListen(15 * 60);
    assert.equal(warnings, 0, "a cleared alert must stop warning");
  },
);
