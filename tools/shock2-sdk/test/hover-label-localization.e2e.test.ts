import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";
const SANGER_LOG_OBJECT = 451;

test(
  "eng1.mis: authored audio-log hover label renders through the production reticle",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "eng1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8194),
    });
    await game.step({ frames: 5 });

    const logs = await game.entities.byTemplate(SANGER_LOG_OBJECT);
    assert.equal(logs.length, 1, "expected eng1 mission object 451 (Sanger audio log)");
    const log = logs[0];
    const [x, y, z] = log.position;

    // Stage nearby, then use the production world-aim path to select the
    // physical disc. Do not squeeze: the hover label should render without
    // collecting or mutating the log.
    await game.player.teleport({ x: x - 1.5, y: y - 1.6, z });
    await game.step({ frames: 3 });
    const aim = await game.player.aimAt(log, {
      hitbox: "center",
      visibility: "required",
      // Use the debug runtime's flat-camera eye offset for this low ceiling;
      // it centers the disc rather than merely confirming its surface ray.
      eyeHeight: 1.4,
    });
    assert.equal(aim.interaction_target_id, log.id, JSON.stringify(aim));
    assert.equal(aim.target_confirmed, true, JSON.stringify(aim));
    await game.step({ frames: 1 });

    // This is a real-mission render smoke and visual artifact capture. The
    // negative-first semantic assertions for OBJNAME.STR resolution, LogTitle
    // substitution, and omission of `| ?` live beside the renderer in
    // item_outline.rs.
    const shot = await game.screenshot("eng1-sanger-localized-hover-label.png");
    assert.deepEqual(shot.resolution, [800, 600]);
    assert.ok(shot.size_bytes > 10_000);

    assert.equal(
      (await game.info()).player.collected_logs.some(
        (collected) => collected.deck === 1 && collected.log === 13,
      ),
      false,
      "rendering the localized label must not collect the authored log",
    );
  },
);
