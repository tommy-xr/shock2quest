import assert from "node:assert/strict";
import { existsSync, rmSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";

import { GameServer, findRepoRoot } from "../src/index.js";
import type { EntityDetailResult } from "../src/index.js";

// End-to-end: a calm AI flagged to patrol (P$AI_Patrol) walks its authored
// route of AIPatrol-linked points, instead of standing idle. eng1 ships a
// substantial patrol network; several of its native creatures are patrollers.
//
// Opt-in (needs Data/ assets + compiles the runtime): npm run test:e2e
const e2eEnabled = process.env.SHOCK2_E2E === "1";

function findSavePath(saveName: string): string | undefined {
  const repoRoot = findRepoRoot(process.cwd()) ?? process.cwd();
  const roots = [
    process.env.DARK_ASSET_PATH,
    join(repoRoot, "Data"),
    join(repoRoot, "..", "Data"),
  ].filter((directory): directory is string => Boolean(directory));
  return roots
    .map((root) => join(root, "saves", `${saveName}.sav`))
    .find(existsSync);
}

function aiProp(detail: EntityDetailResult, name: string): string | undefined {
  return detail.properties.find((p) => p.name === name)?.value;
}

function distXZ(
  a: [number, number, number],
  b: [number, number, number],
): number {
  return Math.hypot(a[0] - b[0], a[2] - b[2]);
}

function dist3(
  a: [number, number, number],
  b: [number, number, number],
): number {
  return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
}

// eng1's native AI archetypes that carry patrol flags
const CREATURE_NAMES = ["OG-Pipe", "OG-Shotgun", "Blue Monkey"];

test(
  "a calm patroller walks its authored route",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "eng1.mis",
    });

    // Let the level's AIs instantiate.
    await game.step({ frames: 60 });

    // Gather eng1's native creatures.
    const creatures: number[] = [];
    for (const name of CREATURE_NAMES) {
      const list = await game.entities.list({ filter: name, limit: 100 });
      for (const e of list.entities) {
        if (e.name === name) creatures.push(e.id);
      }
    }
    assert.ok(creatures.length > 0, "eng1 should have native creatures");

    // Force every creature calm, then find one that enters Patrol - only a
    // creature with P$AI_Patrol and a reachable route does (the rest go Idle),
    // so this both discovers a patroller and proves the behavior is selected.
    for (const id of creatures) {
      await game.entities.sendMessage(id, {
        type: "SetAlertness",
        level: "Lowest",
      });
    }
    await game.step({ frames: 20 });

    let patroller: number | undefined;
    for (const id of creatures) {
      const detail = await game.entities.detail(id);
      if (aiProp(detail, "AIBehavior") === "Patrol") {
        patroller = id;
        break;
      }
    }
    assert.ok(
      patroller !== undefined,
      "expected at least one eng1 creature to enter Patrol when calm",
    );

    // Get the patroller out of the player's sight so it stays calm on its own -
    // teleport the player far off, then calm the patroller once. This lets a
    // SINGLE PatrolBehavior instance run uninterrupted, so the observed motion
    // really comes from arriving at a point and advancing to the next (not from
    // the behavior being rebuilt each tick).
    await game.player.teleport({ x: 300, y: 0, z: 300 });
    await game.step({ frames: 5 });
    await game.entities.sendMessage(patroller, {
      type: "SetAlertness",
      level: "Lowest",
    });
    await game.step({ frames: 10 });

    // Walk the route. Sum per-tick displacement (cumulative path length) so the
    // check holds even when a loop brings the AI back near where it started.
    let prev = (await game.entities.detail(patroller)).position;
    let traveled = 0;
    let stayedPatrolling = true;
    for (let tick = 0; tick < 10; tick++) {
      await game.step({ frames: 120 });
      const detail = await game.entities.detail(patroller);
      traveled += distXZ(detail.position, prev);
      prev = detail.position;
      if (aiProp(detail, "AIBehavior") !== "Patrol") stayedPatrolling = false;
    }

    assert.ok(
      stayedPatrolling,
      "an out-of-sight patroller should stay in Patrol the whole time",
    );
    // A standing Idle AI accumulates ~0; a patroller walks point to point
    // (route legs are tens of units apart), so it covers real ground.
    assert.ok(
      traveled > 8,
      `patroller should walk a meaningful distance along its route, traveled ${traveled.toFixed(1)}`,
    );
  },
);

test(
  "a patroller resumes its live target across alertness and save/load (#410)",
  { skip: !e2eEnabled, timeout: 600_000 },
  async (t) => {
    const saveName = `patrol_target_resume_${Date.now()}`;
    t.after(() => {
      const path = findSavePath(saveName);
      if (path) rmSync(path, { force: true });
    });

    await using game = await GameServer.launch({
      mission: "medsci1.mis",
    });
    // Leave the player at the authored spawn in the sealed cryo recovery room:
    // it has valid walkable support and is isolated from this patrol route.
    await game.step({ frames: 2 });

    const findPatroller = async () => {
      const pipes = await game.entities.list({ filter: "OG-Pipe", limit: 100 });
      const patroller = pipes.entities.find(
        (entity) => entity.template_id === MEDSCI1_PATROLLER_OBJ,
      );
      assert.ok(patroller, "medsci1 native patroller should exist");
      return patroller;
    };
    let patroller = await findPatroller();
    let detail = await game.entities.detail(patroller.id);
    const current = detail.outgoing_links.find(
      (link) => link.link_type === "AICurrentPatrol",
    );
    assert.ok(current, "active patrol should publish its live destination");
    const targetTemplate = (await game.entities.detail(current.target_id)).template_id;

    await game.entities.sendMessage(patroller.id, {
      type: "SetAlertness",
      level: "Moderate",
    });
    await game.step({ frames: 5 });
    detail = await game.entities.detail(patroller.id);
    assert.equal(aiProp(detail, "AIBehavior"), "Chase");
    assert.equal(
      detail.outgoing_links.find(
        (link) => link.link_type === "AICurrentPatrol",
      )?.target_id,
      current.target_id,
      "an alertness interruption must not discard the patrol destination",
    );

    assert.equal((await game.save(saveName)).success, true);
    assert.equal((await game.load(saveName)).success, true);
    await game.step({ frames: 1 });

    patroller = await findPatroller();
    detail = await game.entities.detail(patroller.id);
    const restoredCurrent = detail.outgoing_links.find(
      (link) => link.link_type === "AICurrentPatrol",
    );
    assert.ok(restoredCurrent, "save/load should retain the current patrol link");
    assert.equal(
      (await game.entities.detail(restoredCurrent.target_id)).template_id,
      targetTemplate,
      "the restored relation must point to the same authored patrol marker",
    );
    assert.equal(aiProp(detail, "AIBehavior"), "Patrol");

    // A second live interruption after hydration proves the restored relation
    // is also the handback target, not merely inert serialized metadata.
    await game.entities.sendMessage(patroller.id, {
      type: "SetAlertness",
      level: "Moderate",
    });
    await game.step({ frames: 5 });
    await game.entities.sendMessage(patroller.id, {
      type: "SetAlertness",
      level: "Lowest",
    });
    await game.step({ frames: 1 });
    detail = await game.entities.detail(patroller.id);
    const resumed = detail.outgoing_links.find(
      (link) => link.link_type === "AICurrentPatrol",
    );
    assert.ok(resumed);
    assert.equal(
      (await game.entities.detail(resumed.target_id)).template_id,
      targetTemplate,
    );
    assert.equal(aiProp(detail, "AIBehavior"), "Patrol");
  },
);

// #807: the test above starts its patroller with a SetAlertness, and that
// forced transition is the ONLY reason patrol used to engage - behavior was
// picked exclusively on an alertness LEVEL CHANGE, so a creature that spawns
// calm and is never alerted kept the constructor's Idle for the whole mission.
// This one never touches the AI: it loads the level and watches.
//
// medsci1's OG-Pipe object 163 is a native patroller whose route is the
// two-point AIPatrol loop of "Patrol Path" markers beside it. Object ids are
// stable (`template_id` in the runtime's entity list); runtime entity ids are
// not, so it is discovered by that.
const MEDSCI1_PATROLLER_OBJ = 163;
/** Close enough to count as having reached a route point (arrival is ~0.3). */
const WAYPOINT_REACHED = 3.0;

test(
  "a patroller starts its authored route on a fresh load, unalerted (#807)",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
    });

    // Move the player off the deck before anything can be seen, so the run is
    // unambiguously about an AI nobody ever alerted. No alertness is forced
    // here, and none is expected: the assertions below check it stays Lowest.
    await game.player.teleport({ x: 300, y: 0, z: 300 });
    await game.step({ frames: 60 });

    const pipes = await game.entities.list({ filter: "OG-Pipe", limit: 100 });
    const patroller = pipes.entities.find(
      (e) => e.template_id === MEDSCI1_PATROLLER_OBJ,
    );
    assert.ok(
      patroller,
      `medsci1 should have its native patroller (object ${MEDSCI1_PATROLLER_OBJ})`,
    );

    // Resolve the two-point authored loop around it. Dark measures the initial
    // distance to an AIPatrol link's source but targets that link's destination;
    // following the edge also avoids unrelated markers on the walkway above.
    const candidates = await Promise.all(
      (await game.entities.list({ limit: 5000 })).entities
        .filter((e) => e.name === "Patrol Path")
        .sort(
          (a, b) =>
            dist3(a.position, patroller.position) -
            dist3(b.position, patroller.position),
        )
        .slice(0, 8)
        .map((e) => game.entities.detail(e.id)),
    );
    const firstDetail = candidates.find((d) =>
      d.outgoing_links.some((l) => l.link_type === "AIPatrol"),
    );
    assert.ok(firstDetail, "medsci1 should have a patrol network beside it");
    const nextId = firstDetail.outgoing_links.find(
      (l) => l.link_type === "AIPatrol",
    )!.target_id;
    const markers = [firstDetail, await game.entities.detail(nextId)];

    let detail = await game.entities.detail(patroller.id);
    assert.equal(
      aiProp(detail, "AIAlertness"),
      "Lowest",
      "setup: the patroller must never have been alerted",
    );
    assert.equal(
      aiProp(detail, "AIBehavior"),
      "Patrol",
      "a flagged patroller with a route patrols straight out of the load",
    );

    // ...and it actually walks it: reaching BOTH ends of the loop is what
    // separates a route being followed from a creature milling on the spot.
    const reached = markers.map(() => false);
    let prev = detail.position;
    let traveled = 0;
    for (let tick = 0; tick < 40 && !reached.every(Boolean); tick++) {
      await game.step({ frames: 120 });
      detail = await game.entities.detail(patroller.id);
      traveled += distXZ(detail.position, prev);
      prev = detail.position;
      markers.forEach((m, i) => {
        if (distXZ(detail.position, m.position) < WAYPOINT_REACHED)
          reached[i] = true;
      });
      assert.equal(
        aiProp(detail, "AIAlertness"),
        "Lowest",
        "nothing should have alerted the patroller",
      );
      assert.equal(
        aiProp(detail, "AIBehavior"),
        "Patrol",
        "an unalerted patroller should stay on its route",
      );
    }

    assert.ok(
      reached.every(Boolean),
      `patroller should visit both route points; reached ${JSON.stringify(reached)} after traveling ${traveled.toFixed(1)}`,
    );
  },
);

// Random-sequence patrol (P$AI_PtrlRnd) is the mode most shipped patrollers
// use - eng1 flags 14 of its 17, medsci1 13 of its 14 - yet every other test
// here targets an ordinary patroller (medsci1 object 163 is that mission's
// only one). Random mode does not step to the adjacent marker: it may pick
// ANY node in the connected patrol graph, so this is the only coverage that
// the graph-wide selection actually drives a creature in a real mission.
//
// eng1 object 1672 is a live random patroller on a healthy stretch of the
// network. (Not every patroller in the shipped data can walk its route - some
// sit on nav islands the engine cannot path across, with or without random
// mode - so this deliberately picks one that can.)
const ENG1_RANDOM_PATROLLER_OBJ = 1672;

test(
  "a random-sequence patroller walks its graph (P$AI_PtrlRnd)",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "eng1.mis",
    });

    // Keep the player far away: this is about an AI nobody ever alerted.
    await game.player.teleport({ x: 300, y: 0, z: 300 });
    await game.step({ frames: 60 });

    const all = await game.entities.list({ limit: 5000 });
    const patroller = all.entities.find(
      (e) => e.template_id === ENG1_RANDOM_PATROLLER_OBJ,
    );
    assert.ok(
      patroller,
      `eng1 should have its random patroller (object ${ENG1_RANDOM_PATROLLER_OBJ})`,
    );

    let detail = await game.entities.detail(patroller.id);
    assert.equal(
      aiProp(detail, "AIBehavior"),
      "Patrol",
      "a random-sequence patroller patrols straight out of the load",
    );

    // Random mode picks a fresh target from the whole connected graph on every
    // arrival, so the route it walks differs run to run - assert on what must
    // hold for ANY selection: it keeps patrolling, keeps covering ground, and
    // its live AICurrentPatrol target moves through more than one node.
    let prev = detail.position;
    let traveled = 0;
    const targets: number[] = [];
    for (let tick = 0; tick < 40; tick++) {
      await game.step({ frames: 120 });
      detail = await game.entities.detail(patroller.id);
      traveled += distXZ(detail.position, prev);
      prev = detail.position;
      const current = detail.outgoing_links.find(
        (l) => l.link_type === "AICurrentPatrol",
      );
      if (current && !targets.includes(current.target_id)) {
        targets.push(current.target_id);
      }
      assert.equal(
        aiProp(detail, "AIAlertness"),
        "Lowest",
        "nothing should have alerted the random patroller",
      );
    }

    assert.equal(
      aiProp(detail, "AIBehavior"),
      "Patrol",
      "a random-sequence patroller should stay on its route, not fall to Idle",
    );
    assert.ok(
      traveled > WAYPOINT_REACHED * 4,
      `random patroller should cover ground; traveled ${traveled.toFixed(1)}`,
    );
    assert.ok(
      targets.length > 1,
      `random patroller should retarget as it goes; saw ${JSON.stringify(targets)}`,
    );
  },
);
