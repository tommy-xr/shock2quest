import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { SceneObjectSummary } from "../src/types.js";

// The dev-params registry (`shock2vr::dev_params`) is mirrored over HTTP:
// GET /v1/dev-params lists every live-tunable knob, POST sets one by key.
// Consumers read the registry every frame, so a POST applies on the very next
// stepped frame - which this test proves by measuring where the renderer
// actually placed the VR main-menu panel before and after changing
// `panel_distance`.
//
// Negative-first: against the pre-registry build (panel distance a `const`),
// the "panel depth follows the POSTed value" assertion fails - the panel
// stays at 2.0 no matter what is POSTed (and the endpoint itself 404s).
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/**
 * The gaze-axis depth of the frontend panel, from the scene objects the
 * renderer was handed. The debug runtime's default VR head looks along -X
 * from the origin, and the panel hangs perpendicular to the gaze, so every
 * canvas element of it sits at x = -distance (give or take the millimetric
 * layer stacking). The only other draws on the main menu are the tagged
 * pointer visuals (`source: "frontend_pointer"`), which are filtered out.
 */
function panelDepth(objects: SceneObjectSummary[]): number {
  const depths = objects
    .filter((o) => o.source === null)
    .map((o) => -o.position[0])
    .sort((a, b) => a - b);
  assert.ok(depths.length > 0, "expected untagged panel objects in the scene");
  return depths[Math.floor(depths.length / 2)];
}

test(
  "POSTing a dev param moves the live VR frontend panel",
  { skip: !e2eEnabled && "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "main_menu",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8106),
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 10 });

    // The registry lists the seeded params with their replaced-const defaults.
    const { params } = await game.devParams.list();
    const panelDistance = params.find((p) => p.key === "panel_distance");
    assert.ok(panelDistance, "panel_distance must be registered");
    assert.equal(panelDistance.kind, "float");
    assert.ok(Math.abs(panelDistance.value - 2.0) < 1e-4);
    assert.ok(Math.abs(panelDistance.default - 2.0) < 1e-4);

    // At the default, the panel hangs 2m ahead of the head.
    const before = panelDepth((await game.scene.objects()).objects);
    assert.ok(
      Math.abs(before - 2.0) < 0.05,
      `panel should render at the default 2.0m, got ${before}`,
    );
    const shotBefore = await game.screenshot("dev-params-panel-2m.png");

    // Live-apply: POST a new distance and the very next frames render there.
    const applied = await game.devParams.set("panel_distance", 4.0);
    assert.ok(Math.abs(applied.value - 4.0) < 1e-4);
    await game.step({ frames: 2 });
    const after = panelDepth((await game.scene.objects()).objects);
    assert.ok(
      Math.abs(after - 4.0) < 0.05,
      `panel should have moved to the POSTed 4.0m, got ${after}`,
    );
    const shotAfter = await game.screenshot("dev-params-panel-4m.png");
    console.log(
      `panel depth ${before.toFixed(3)} -> ${after.toFixed(3)}; ` +
        `screenshots: ${shotBefore.full_path} ${shotAfter.full_path}`,
    );

    // Out-of-range sets are clamped to the declared max...
    const clamped = await game.devParams.set("panel_distance", 99.0);
    assert.ok(Math.abs(clamped.value - 6.0) < 1e-4);

    // Reset restores the exact declared default (bit-identical, not merely
    // close: set(default) can miss it, since the snap grid does not
    // round-trip every default), and the panel renders back at it.
    const reset = await game.devParams.reset("panel_distance");
    assert.equal(reset.value, panelDistance.default);
    await game.step({ frames: 2 });
    const restored = panelDepth((await game.scene.objects()).objects);
    assert.ok(
      Math.abs(restored - 2.0) < 0.05,
      `panel should be back at the 2.0m default after reset, got ${restored}`,
    );

    // Unknown keys are refused, not silently accepted.
    await assert.rejects(
      game.devParams.set("no_such_param", 1.0),
      /404/,
      "an unknown key must reject",
    );
  },
);
