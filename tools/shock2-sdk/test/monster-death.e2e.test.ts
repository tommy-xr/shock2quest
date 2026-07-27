import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult } from "../src/index.js";

// End-to-end test against a real debug runtime. Requires game assets in
// Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

function aiProp(detail: EntityDetailResult, name: string): string | undefined {
  return detail.properties.find((p) => p.name === name)?.value;
}

function distance3(a: [number, number, number], b: [number, number, number]) {
  return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
}

function maxJointDistance(
  a: Array<[number, number, number]>,
  b: Array<[number, number, number]>,
) {
  assert.equal(a.length, b.length, "saved and loaded joint counts should match");
  return Math.max(...a.map((joint, index) => distance3(joint, b[index]!)));
}

async function spawnOgPipe(game: GameServer) {
  // Identify the debug spawn by diffing the entity list: medsci1 has native
  // OG-Pipes too, and runtime entity IDs are not stable across launches.
  const preSpawn = await game.entities.list({ filter: "OG-Pipe", limit: 50 });
  const known = new Set(preSpawn.entities.map((entity) => entity.id));
  await game.input.trigger("SpawnDebugMonster");
  await game.step({ frames: 30 });
  const postSpawn = await game.entities.list({ filter: "OG-Pipe", limit: 50 });
  const monster = postSpawn.entities.find(
    (entity) => entity.name === "OG-Pipe" && !known.has(entity.id),
  );
  assert.ok(
    monster,
    `expected a newly spawned OG-Pipe, got ${JSON.stringify(postSpawn.entities.map((entity) => entity.id))}`,
  );
  return monster;
}

test(
  "a killed monster dies and stays dead",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8104),
    });

    await game.step({ frames: 10 });

    const monster = await spawnOgPipe(game);

    // Alert it first so the post-death alertness machinery has both paths to
    // fire (escalation while the player is visible, decay afterwards) - the
    // regression this guards against is any alertness level change replacing
    // DeadBehavior.
    // Only step a few frames past the alert: forcing alertness queues a
    // fresh chase clip, so the kill below lands near the clip's start and
    // the eager-death assertion actually discriminates (waiting for the
    // clip to complete would blow the half-second window).
    await game.input.trigger("DebugAlertAll");
    await game.step({ frames: 10 });
    let detail = await game.entities.detail(monster.id);
    assert.equal(aiProp(detail, "AIBehavior"), "Chase");

    // Lethal damage reacts eagerly: the death animation interrupts the
    // in-flight chase clip instead of waiting for it to complete, so the
    // behavior must read Dead within half a second of the killing blow.
    await game.entities.sendMessage(monster.id, {
      type: "Damage",
      amount: 1000,
    });
    await game.step({ frames: 30 });
    assert.equal(
      aiProp(await game.entities.detail(monster.id), "AIBehavior"),
      "Dead",
      "killing blow should interrupt the playing clip immediately",
    );

    // Let the crumple (and any interrupted clip still in the animation
    // queue) finish - death clips carry root motion, so the body still
    // translates for a couple of seconds after DeadBehavior is set.
    await game.step({ frames: 240 });
    assert.equal(
      aiProp(await game.entities.detail(monster.id), "AIBehavior"),
      "Dead",
    );

    // The corpse must stay dead: the alertness escalate (1.5s) and decay (3s)
    // windows both elapse several times over while the player stands in view.
    // Wait for the body to actually come to rest before taking the baseline.
    // What the assertion below is about is the corpse WANDERING, not how far it
    // slid while coming to rest, and the settle does not take a constant time -
    // it finishes anywhere from x=-36.5 to -39.9 across runs - so no fixed frame
    // count separates the two. Inferring rest from position samples does not
    // either: a quiet window also occurs during a momentary pause mid-settle,
    // and a baseline taken there left the remaining slide inside the
    // measurement (observed 0.74 against a 0.5 threshold). Rest is a physics
    // fact, so ask the body. If it never sleeps, fall through and let the
    // assertion report the real motion rather than masking it.
    for (let i = 0; i < 60; i++) {
      const [body] = (await game.physics.bodies({ entityId: monster.id })).bodies;
      if (body?.is_sleeping) break;
      await game.step({ frames: 10 });
    }
    const deadPos = (await game.entities.detail(monster.id)).position;
    for (let i = 0; i < 5; i++) {
      await game.step({ frames: 120 });
      detail = await game.entities.detail(monster.id);
      assert.equal(
        aiProp(detail, "AIBehavior"),
        "Dead",
        `corpse resurrected after ${(i + 1) * 2}s`,
      );
    }

    // ... and must not wander off.
    const finalPos = detail.position;
    const drift = Math.hypot(
      finalPos[0] - deadPos[0],
      finalPos[2] - deadPos[2],
    );
    assert.ok(
      drift < 0.5,
      `corpse moved ${drift.toFixed(2)} units after death`,
    );
  },
);

test(
  "a loaded corpse does not replay its death",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8104),
    });

    await game.step({ frames: 10 });
    const monster = await spawnOgPipe(game);
    await game.entities.sendMessage(monster.id, {
      type: "Damage",
      amount: 1000,
    });

    // Death clips are randomly resolved and have different durations. Advance
    // fixed-size simulation chunks until whichever clip was chosen has
    // genuinely drained, rather than assuming a particular frame count.
    let savedAnimation:
      | Awaited<ReturnType<typeof game.entities.animation>>
      | undefined;
    let savedBody:
      | Awaited<ReturnType<typeof game.physics.bodies>>["bodies"][number]
      | undefined;
    for (let attempt = 0; attempt < 60; attempt += 1) {
      await game.step({ frames: 30 });
      const animation = await game.entities.animation(monster.id);
      const bodies = await game.physics.bodies({ entityId: monster.id });
      assert.equal(bodies.bodies.length, 1, "OG-Pipe should own one capsule body");
      const body = bodies.bodies[0];
      if (
        animation?.clip === null &&
        animation.last_clip &&
        animation.queue.length === 0 &&
        body?.is_sleeping
      ) {
        savedAnimation = animation;
        savedBody = body;
        break;
      }
    }
    assert.ok(
      savedAnimation,
      "death animation should reach a settled terminal pose within 1800 frames",
    );
    assert.ok(savedBody, "terminal corpse body should be sleeping");
    assert.equal(savedBody.body_type, "dynamic");
    const savedCorpse = await game.entities.detail(monster.id);
    assert.equal(aiProp(savedCorpse, "AIBehavior"), "Dead");
    assert.equal(savedAnimation.clip, null, "death animation should be complete");
    assert.ok(savedAnimation.last_clip, "death should leave a terminal pose");
    assert.deepEqual(savedAnimation.queue, []);
    assert.ok(savedAnimation.joints.length > 0, "saved corpse should be posed");

    // Save/load rebuilds scripts rather than serializing their internal
    // behavior state. Rediscover the corpse after load by its stable template
    // plus saved position: runtime entity IDs are assigned afresh.
    const saveName = `monster_reload_e2e_${Date.now()}`;
    await game.save(saveName);
    await game.load(saveName);

    const loadedMonsters = await game.entities.list({
      filter: "OG-Pipe",
      limit: 50,
    });
    const loadedCorpse = loadedMonsters.entities
      .filter(
        (entity) =>
          entity.name === monster.name &&
          entity.template_id === monster.template_id,
      )
      .map((entity) => ({
        entity,
        distance: distance3(entity.position, savedCorpse.position),
      }))
      .sort((a, b) => a.distance - b.distance)[0];
    assert.ok(
      loadedCorpse && loadedCorpse.distance < 0.02,
      `expected the saved corpse near ${JSON.stringify(savedCorpse.position)}, got ${JSON.stringify(loadedMonsters.entities)}`,
    );

    // A loaded corpse must hold the exact saved terminal death pose and body
    // position immediately. Sample irregular frames through the old idle
    // completion window: before the fix initialize() queued idle, then
    // AnimationCompleted replayed the death clip and death speech.
    const loadedStates: Array<{
      frame: number;
      behavior: string | undefined;
      clip: string | null;
      clipFrame: number;
      lastClip: string | null;
      queuedClips: Array<string | null>;
      bodyType: "dynamic" | "static" | "kinematic";
      bodySleeping: boolean;
      bodyVelocity: number;
      bodyMass: number | null;
      collisionGroups: string[];
      positionError: number;
      poseError: number;
    }> = [];
    let previousFrame = 0;
    for (const frame of [0, 17, 53, 97, 151]) {
      if (frame > previousFrame) {
        await game.step({ frames: frame - previousFrame });
      }
      const loadedDetail = await game.entities.detail(loadedCorpse.entity.id);
      const animation = await game.entities.animation(loadedCorpse.entity.id);
      assert.ok(animation, "loaded OG-Pipe should have an animation player");
      const bodies = await game.physics.bodies({
        entityId: loadedCorpse.entity.id,
      });
      assert.equal(bodies.bodies.length, 1, "loaded corpse should retain one body");
      const body = bodies.bodies[0]!;
      loadedStates.push({
        frame,
        behavior: aiProp(loadedDetail, "AIBehavior"),
        clip: animation.clip,
        clipFrame: animation.frame,
        lastClip: animation.last_clip,
        queuedClips: animation.queue.map((entry) => entry.name),
        bodyType: body.body_type,
        bodySleeping: body.is_sleeping,
        bodyVelocity: Math.hypot(...body.velocity),
        bodyMass: body.mass,
        collisionGroups: body.collision_groups,
        positionError: distance3(loadedDetail.position, savedCorpse.position),
        poseError: maxJointDistance(animation.joints, savedAnimation.joints),
      });
      previousFrame = frame;
    }

    const initialPlayback = loadedStates[0];
    assert.ok(initialPlayback, "expected at least one loaded animation sample");
    assert.ok(
      loadedStates.every(
        (state) =>
          state.behavior === "Dead" &&
          state.clip === null &&
          state.clipFrame === initialPlayback.clipFrame &&
          state.lastClip === savedAnimation.last_clip &&
          state.queuedClips.length === 0 &&
          state.bodyType === savedBody.body_type &&
          state.bodySleeping &&
          state.bodyVelocity < 0.0001 &&
          state.bodyMass === savedBody.mass &&
          JSON.stringify(state.collisionGroups) ===
            JSON.stringify(savedBody.collision_groups) &&
          state.positionError < 0.02 &&
          state.poseError < 0.02,
      ),
      `loaded corpse changed behavior, pose, or position: ${JSON.stringify(loadedStates)}`,
    );
  },
);
