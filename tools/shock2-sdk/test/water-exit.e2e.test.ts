import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";

import { GameServer, findRepoRoot } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

function hasFrontierSave(name: string): boolean {
  const repoRoot = findRepoRoot(process.cwd()) ?? process.cwd();
  const roots = [
    process.env.DARK_ASSET_PATH,
    join(repoRoot, "Data"),
    join(repoRoot, "..", "Data"),
  ].filter((root): root is string => Boolean(root));
  return roots.some((root) => existsSync(join(root, "saves", `${name}.sav`)));
}

// Issue #680: frontier-007 is the verified campaign state on rec1's pool
// floor. Before water-medium locomotion and general jump mantling, the same
// held input remained pinned at y=-8.196 below the west-wall duct.
//
// The copyrighted campaign save is deliberately not checked in. Developers
// with the play-through artifacts get the full regression; other environments
// skip this one scenario while the equivalent mechanics remain covered by
// deterministic Rust physics tests.
test(
  "rec1 frontier-007 swims and mantles out of the pool",
  {
    skip: !e2eEnabled || !hasFrontierSave("frontier-007"),
    timeout: 600_000,
  },
  async () => {
    await using game = await GameServer.launch({
      mission: "rec1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8127),
    });
    assert.equal((await game.load("frontier-007")).success, true);
    await game.step({ frames: 5 });

    const before = await game.player.position();
    assert.ok(
      before.y < -7.5,
      `frontier must start on the pool floor, got ${JSON.stringify(before)}`,
    );

    // The save-restored body rotation plus yaw -90 faces the west-wall duct.
    // These are ordinary persistent input channels used by desktop/VR too:
    // forward swims, jump swims upward, then the same held jump mantles the lip.
    await game.input.set("head.look", [-90, 0]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.input.set("jump", 1);
    await game.step({ frames: 140 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.input.set("jump", 0);
    await game.step({ frames: 5 });

    const after = await game.player.position();
    assert.ok(
      after.y > -4.5,
      `held swim+jump must reach the dry duct height, moved ${JSON.stringify(before)} -> ${JSON.stringify(after)}`,
    );
    assert.ok(
      after.x < 10,
      `the mantle must cross the west pool wall toward the duct, ended ${JSON.stringify(after)}`,
    );
  },
);
