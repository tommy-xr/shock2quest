import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { describeSounds, tagValue } from "./helpers/audio.js";

// medsci1's crawl-scene hybrid plays its ranged-combat motion as a scripted
// performance (AIWatchObj `Play`). Its shot reports - gunshot and flash - but
// launches no round: only a ranged attack fires one.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const CRAWL_HYBRID = 613;
const CRAWL_SCENE_MARKER = { x: -23.57, y: 0.84, z: -15.44 };

test(
  "medsci1 crawl-scene hybrid's scripted shot reports without a round",
  { skip: !e2eEnabled, timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "medsci1.mis" });
    await game.devParams.set("cheat", 1);
    await game.step({ frames: 2 });

    const hybrid = (
      await game.entities.list({ filter: "OG-Shotgun" })
    ).entities.find((e) => e.template_id === CRAWL_HYBRID);
    assert.ok(
      hybrid,
      `medsci1 should contain the crawl-scene hybrid (${CRAWL_HYBRID})`,
    );
    const slugs = async () =>
      (await game.entities.list({ filter: "Grunt Shotgun Slug" })).total_count;
    const slugsBefore = await slugs();

    // Entering the marker starts the scene; the shot comes ~8 s in. A flash
    // lives a few frames, so step one at a time.
    await game.player.teleport(CRAWL_SCENE_MARKER);
    let flashed = false;
    for (let frame = 0; frame < 800 && !flashed; frame++) {
      await game.step({ frames: 1 });
      flashed =
        (await game.entities.list({ filter: "Assault Flash" })).total_count > 0;
    }

    assert.ok(flashed, "the scripted shot must show the hybrid's muzzle flash");
    const launches = (await game.audio.recent()).sounds.filter(
      (s) => tagValue(s, "event") === "launch",
    );
    assert.ok(
      launches.some((s) => s.sample.toLowerCase().startsWith("ogsgun")),
      `the scripted shot must play its gunshot (${describeSounds(launches)})`,
    );
    assert.equal(
      await slugs(),
      slugsBefore,
      "a scripted shot launches no round",
    );
  },
);
