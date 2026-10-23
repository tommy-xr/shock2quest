import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { findRepoRoot, GameServer } from "../src/index.js";
import type { ResolvedGrip } from "../src/types.js";
import { isolatedDataRoot } from "./helpers/isolated-data-root.js";
import { holdDebugMug } from "./helpers/vr-hand.js";

const enabled = process.env.SHOCK2_E2E === "1";

test(
  "grip overrides in the data root win over the shipped grip",
  { skip: !enabled, timeout: 300_000 },
  async () => {
    // The user's own overrides, saves and settings stay out of the private root.
    using data = isolatedDataRoot([
      "vr-grip-overrides.json", "saves", "recordings", "settings.json", "user-settings.json",
    ]);
    const shipped = JSON.parse(
      readFileSync(join(findRepoRoot(process.cwd())!, "assets/vr-grips.json"), "utf8"),
    ) as { entries: { model: string; hand: string; grip: ResolvedGrip }[] };
    const mug = shipped.entries.find((e) => e.model === "mug" && e.hand === "right")!;
    assert.notEqual(mug.grip.item_scale, 0.5, "the override must differ from the shipped pose");
    const override = { ...mug, grip: { ...mug.grip, item_scale: 0.5 } };
    writeFileSync(
      join(data.root, "vr-grip-overrides.json"),
      JSON.stringify({ version: 1, solver_revision: 3, entries: [override] }),
    );
    await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
    const item = await holdDebugMug(game);
    const grip = (await game.info()).player.hand_grips.find((g) => g.hand === "right")!;
    assert.equal(grip.entity_id, item.id, "the mug is held");
    // Reported and drawn at the world scale, as shipped grips are: the
    // override's authored size, kept physically.
    const worldScale = (await game.devParams.list()).params.find((p) => p.key === "world_scale")!.value as number;
    const itemScale = 0.5 / worldScale;
    assert.ok(Math.abs(grip.grip!.item_scale - itemScale) < 1e-5, `${grip.grip?.item_scale} vs ${itemScale}`);
    for (const draw of (await game.scene.objects({ entityId: item.id })).objects) {
      for (const scale of draw.scale) assert.ok(Math.abs(scale - itemScale) < 1e-5, "held mug renders at the override's scale");
    }
  },
);
