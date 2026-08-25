import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult, EntitySummary } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

const THREATEN_OG = 1383;
const OG_MOAN_WAYPOINT = 760;

function only(matches: EntitySummary[], label: string): EntitySummary {
  assert.equal(matches.length, 1, `expected one ${label}, got ${matches.length}`);
  return matches[0]!;
}

function property(detail: EntityDetailResult, name: string): string | undefined {
  return detail.properties.find((candidate) => candidate.name === name)?.value;
}

function horizontalDistance(a: number[], b: number[]): number {
  return Math.hypot(a[0]! - b[0]!, a[2]! - b[2]!);
}

// Regression for #1068. ThreatenOG's concrete object script list deliberately
// disables inheritance, but Dark's creature/AI service is independent of that
// list. The port used to attach AnimatedMonsterAI only through inherited
// BaseMonster, leaving this live scripted hybrid in its bind pose, immune to
// Damage, and permanently blocking the camera-room route.
test(
  "medsci1 ThreatenOG keeps AI, damage, and its authored OGMoan movement",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      debugFlags: ["--vr"],
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
    });
    await game.step({ frames: 10 });

    // Mission-object ids are stable; runtime ids are discovered every launch.
    const threat = only(
      await game.entities.byTemplate(THREATEN_OG),
      "ThreatenOG mission object 1383",
    );
    const waypoint = only(
      await game.entities.byTemplate(OG_MOAN_WAYPOINT),
      "ogmoantrap mission object 760",
    );
    const before = await game.entities.detail(threat.id);
    const initialHp = Number(property(before, "HitPoints"));
    const initialWaypointDistance = horizontalDistance(
      before.position,
      waypoint.position,
    );

    assert.equal(initialHp, 12, "the authored hybrid must start alive");
    assert.equal(
      property(before, "AIBehavior"),
      "Idle",
      "the script override must not remove engine creature AI",
    );
    assert.ok(
      (await game.entities.animation(threat.id))?.clip,
      "the live creature must start in an authored motion rather than the bind pose",
    );

    // AI Signal Trap 756 delivers this exact production payload after the
    // normal Science route. Direct injection isolates the receiver here.
    await game.entities.sendMessage(threat.id, { type: "Signal", name: "OGMoan" });
    await game.step({ frames: 2 });
    assert.equal(
      property(await game.entities.detail(threat.id), "AIBehavior"),
      "ScriptedSequence",
      "OGMoan must start the authored response",
    );

    // A Wrench hit ultimately uses this Damage message path. Receiving it
    // proves the actor is still a normal live combat target during the beat.
    await game.entities.sendMessage(threat.id, { type: "Damage", amount: 1 });
    await game.step({ frames: 2 });
    assert.equal(
      Number(property(await game.entities.detail(threat.id), "HitPoints")),
      initialHp - 1,
      "ThreatenOG must retain ordinary creature damage handling",
    );

    let closestWaypointDistance = initialWaypointDistance;
    let farthestFromBlockingPose = 0;
    let observedAnimatedPose = false;
    for (let halfSecond = 0; halfSecond < 40; halfSecond += 1) {
      await game.step({ frames: 30 });
      const current = await game.entities.detail(threat.id);
      closestWaypointDistance = Math.min(
        closestWaypointDistance,
        horizontalDistance(current.position, waypoint.position),
      );
      farthestFromBlockingPose = Math.max(
        farthestFromBlockingPose,
        horizontalDistance(current.position, before.position),
      );
      observedAnimatedPose ||= Boolean((await game.entities.animation(threat.id))?.clip);
      if (farthestFromBlockingPose > 1.5) break;
    }

    assert.ok(observedAnimatedPose, "the OGMoan performance must remain animated");
    assert.ok(
      closestWaypointDistance < initialWaypointDistance - 1.5,
      `ThreatenOG must progress toward ogmoantrap; initial=${initialWaypointDistance}, closest=${closestWaypointDistance}`,
    );
    assert.ok(
      farthestFromBlockingPose > 1.5,
      `ThreatenOG must vacate its standing-route pose; moved=${farthestFromBlockingPose}`,
    );
  },
);
