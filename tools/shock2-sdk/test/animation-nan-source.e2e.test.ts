import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test for the #508 producer fix: animation clips with all-zero
// quaternion joint tracks (23 of the 613 stock clips - bh114009, humwalks,
// humalert, the mbt* maintenance-bot set, ...) used to parse those tracks to
// all-NaN rotation matrices (`read_quat` inverted a zero quaternion, dividing
// by zero magnitude). Any creature playing such a clip emitted NaN joint
// transforms, and the per-joint kinematic hitbox bodies fed those NaN poses
// to rapier every frame - poisoning the broad-phase BVH and panicking parry's
// binned rebuild a few frames later (#506; upstream dimforge/rapier#961).
//
// The fix parses a zero quaternion as the identity rotation (bind pose for
// the unanimated joint - the original engine's unnormalized quat->matrix
// conversion behaves identically), so the NaN never exists.
//
// The test forces the exact organic producer deterministically: the
// DebugHitboxCyclePose action queues named pose clips on every creature, and
// its playlist ends with bh114009 (the clip caught producing NaN hitbox poses
// live on station.mis). Playing it through must produce zero non-finite
// rigid-body reports (#507's detection layer is the tripwire - it mutates
// nothing, so any NaN reaching physics is reported) and leave every physics
// body finite. Unfixed, this fails within seconds: the detector fires for
// each hitbox of every posed creature, and the parry panic can kill stepping
// outright.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "station.mis: posing creatures with a zero-quat-track clip stays finite (#508)",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "station.mis",
    });
    await game.step({ frames: 30 }); // settle

    const nonfiniteReports = () =>
      game.logs().filter((l) => l.includes("non-finite rigid-body state"));
    assert.equal(
      nonfiniteReports().length,
      0,
      "no non-finite report should fire before the poisoned clip plays",
    );

    // A creature to observe the pose cycle on (entity ids are not stable
    // across runs, so discover one by name each launch).
    const entitiesRes = await fetch(`${game.baseUrl}/v1/entities?filter=DROID`);
    const { entities } = (await entitiesRes.json()) as {
      entities: { id: number }[];
    };
    assert.ok(entities.length > 0, "station.mis should have droid creatures");
    const droidId = entities[0].id;
    const animationClips = async (): Promise<string[]> => {
      const res = await fetch(
        `${game.baseUrl}/v1/entities/${droidId}/animation`,
      );
      const state = (await res.json()) as {
        clip: string | null;
        queue: { name: string | null }[];
        last_clip: string | null;
      } | null;
      if (!state) return [];
      return [
        state.clip,
        state.last_clip,
        ...state.queue.map((q) => q.name),
      ].filter((c): c is string => typeof c === "string");
    };

    // Cycle the pose playlist to its final entry, bh114009 (the playlist is
    // deterministic: one advance per trigger, starting at index 0). The
    // regression clip must actually reach the creature's animation queue.
    let sawRegressionClip = false;
    for (let i = 0; i < 7; i++) {
      await game.input.trigger("DebugHitboxCyclePose");
      await game.step({ frames: 2 });
      const clips = await animationClips();
      if (clips.some((c) => c.toLowerCase().includes("bh114009"))) {
        sawRegressionClip = true;
      }
    }
    assert.ok(
      sawRegressionClip,
      "cycling the full pose playlist should queue bh114009 on the droid",
    );

    // Play the clip through (98 frames at 30fps + 500ms blend-in) while the
    // hitbox bodies track the posed joints every frame. Unfixed, stepping
    // dies here (parry panic) or the detector fires below.
    for (let chunk = 0; chunk < 5; chunk++) {
      const stepped = await game.step({ frames: 60 });
      assert.equal(
        stepped.frames_advanced,
        60,
        `step chunk ${chunk} should advance (a failure means the physics ` +
          `thread died - see #506 / parry bvh_binned_build panic)`,
      );
    }

    assert.equal(
      nonfiniteReports().length,
      0,
      `the zero-quat clip must not produce any non-finite rigid-body state, ` +
        `got:\n${nonfiniteReports().slice(0, 5).join("\n")}`,
    );

    // Belt and braces: every physics body (hitboxes included) is still
    // finite. (serde_json writes NaN/inf as null, so non-finite components
    // fail Number.isFinite here.)
    const { bodies } = await game.physics.bodies({ limit: 2000 });
    const bad = bodies.filter(
      (b) =>
        ![
          ...b.position,
          ...b.rotation,
          ...b.velocity,
          ...b.angular_velocity,
        ].every((c) => Number.isFinite(c)),
    );
    assert.equal(
      bad.length,
      0,
      `all bodies must stay finite, got ${bad.length} bad (first: ${JSON.stringify(bad[0])})`,
    );
  },
);

// The scenario that killed 5/5 campaign sessions before the stack (#506):
// plain walking around station.mis's start room while nearby NPCs animate.
// With the producer fixed there is nothing left to poison the BVH - the walk
// must survive with zero non-finite reports.
test(
  "station.mis: walking the start room survives with zero non-finite reports (#506)",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "station.mis",
    });
    await game.step({ frames: 30 });

    // The corridor from the start room toward the Mission Postings door is
    // covered by overlapping tripwire sensors and room boundaries around
    // (67..69, -4.6, 17.5) - the area the crashing campaign sessions walked.
    const waypoints = [
      { x: 68.5, y: -4.3, z: 24.0 }, // outside, start-room side
      { x: 68.5, y: -4.3, z: 12.0 }, // outside, Mission Postings side
    ];
    for (let pass = 0; pass < 4; pass++) {
      for (const wp of waypoints) {
        await game.player.teleport(wp);
        await game.step({ frames: 30 });
      }
      // Also walk through under locomotion (exit-by-walking is how the
      // campaign hit it, and it exercises the character controller path).
      await game.input.set("right_hand.thumbstick", [0, 1]);
      await game.step({ frames: 90 });
      await game.input.set("right_hand.thumbstick", [0, 0]);
      const stepped = await game.step({ frames: 30 });
      assert.equal(stepped.frames_advanced, 30, `pass ${pass} should survive`);
    }

    const reports = game
      .logs()
      .filter((l) => l.includes("non-finite rigid-body state"));
    assert.equal(
      reports.length,
      0,
      `walking must not produce non-finite body state, got:\n${reports
        .slice(0, 5)
        .join("\n")}`,
    );
    const info = await game.info();
    assert.equal(info.mission, "station.mis");
  },
);
