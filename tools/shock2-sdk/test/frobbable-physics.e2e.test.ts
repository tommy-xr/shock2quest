import assert from "node:assert/strict";
import { test } from "node:test";

import { e2ePort } from "./helpers/e2e-port.js";
import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Command1 mission object 1520 (`Swarmer Floor Pod`) explicitly authors a
// SPHERE physics model. The frobbable selection path used to replace it with a
// kinematic 2.0 x 1.568 x 2.0 render-model box; this keeps the authored sphere.
//
// The strip between the pod and the east wall is NOT a player route in retail:
// the authored radius is 1.96 ft and the pod is location-controlled, leaving
// 1.59 ft to a wall that a 2.4-ft body (crouched or not) cannot pass. A
// crouched walk along the wall must stop at the pod. (Until #1815 our sphere is
// half size, and only the controller's contact offset closes the strip.)
test(
  "Command Floor Pod keeps its authored sphere and blocks the strip beside the wall",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "command1.mis",
      port: e2ePort(0, "SHOCK2_E2E_FROBBABLE_PHYSICS_PORT"),
    });
    await game.step({ frames: 5 });

    const [button] = await game.entities.byTemplate(840);
    const [door] = await game.entities.byTemplate(839);
    const [pod] = await game.entities.byTemplate(1520);
    assert.ok(button && door && pod, "command1 route objects should remain authored");
    assert.ok(
      pod.position[0] > -288.3 &&
        pod.position[0] < -288.1 &&
        pod.position[1] > -8.2 &&
        pod.position[1] < -7.5 &&
        pod.position[2] > 87.5 &&
        pod.position[2] < 87.8,
      `stable mission object 1520 should remain beside the reviewed portal, got ` +
        JSON.stringify(pod.position),
    );

    // Exercise the real control that admits the player to the ladder route.
    await game.player.teleport({ x: -291.5, y: -0.756, z: 84.4157 });
    await game.step({ frames: 5 });
    const aim = await game.player.aimAt(button, {
      hitbox: "center",
      visibility: "required",
    });
    assert.equal(aim.interaction_target_id, button.id, JSON.stringify(aim));
    const closedDoor = await game.entities.detail(door.id);
    await game.input.set("right_hand.squeeze_value", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze_value", 0);
    await game.step({ frames: 180 });
    const openDoor = await game.entities.detail(door.id);
    assert.ok(
      closedDoor.position[2] - openDoor.position[2] > 2,
      `button 840 should open door 839 (${JSON.stringify(closedDoor.position)} -> ` +
        `${JSON.stringify(openDoor.position)})`,
    );

    // Setup only: stage at the campaign-supported pose reached after the fast
    // crouched ladder descent. The walk itself uses look + locomotion.
    await game.input.set("crouch", 1);
    await game.step({ frames: 60 });
    await game.player.teleport({
      x: -287.3416,
      y: -7.796,
      z: 86.3136,
    });
    await game.step({ frames: 30 });
    const before = await game.player.position();
    const crouchedEye = (await game.info()).player.camera_offset[1];
    await game.input.lookAtWorldPoint(
      [before.x, before.y + crouchedEye, before.z + 6],
      { eyeHeight: crouchedEye },
    );
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.step({ frames: 120 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    const blocked = await game.player.position();
    // The render-model box spans z 86.67..88.67, so it would hold the body
    // short of z 87; the authored sphere lets it reach the pod's side, no further.
    assert.ok(
      blocked.z > 87 && blocked.z < pod.position[2] + 0.3 && blocked.y > -7.9 && blocked.y < -7.6,
      `the crouched body should stop at the pod's side, not pass it ` +
        `(${JSON.stringify(before)} -> ${JSON.stringify(blocked)})`,
    );

    await game.step({ frames: 120 });
    const supported = await game.player.position();
    assert.ok(
      Math.hypot(
        supported.x - blocked.x,
        supported.y - blocked.y,
        supported.z - blocked.z,
      ) < 0.05,
      `the blocked body must stay supported, got ${JSON.stringify(blocked)} -> ` +
        JSON.stringify(supported),
    );

    // The surface beside the route is the authored sphere's (centre x -288.22,
    // r 0.39 here), not the render box's face at x -287.22.
    const side = await game.raycast({
      start: [-286.9, -7.7, pod.position[2]],
      end: [-290, -7.7, pod.position[2]],
      collision_groups: ["entity"],
    });
    assert.ok(
      side.entity_id === pod.id && side.hit_point && side.hit_point[0] < -287.6,
      `the pod's physical surface should be its authored sphere, got ${JSON.stringify(side)}`,
    );

    const podBodies = (await game.physics.bodies({ entityId: pod.id })).bodies;
    assert.equal(podBodies.length, 1, "Floor Pod 1520 should own one physics body");
    assert.equal(
      podBodies[0].body_type,
      "kinematic",
      "the pod is a fixture: only its geometry comes from the authored model",
    );
    assert.equal(podBodies[0].blocks_player, true, "the live pod remains solid");
  },
);
