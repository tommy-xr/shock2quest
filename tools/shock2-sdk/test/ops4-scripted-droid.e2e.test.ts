import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";
const port = Number(process.env.SHOCK2_E2E_PORT ?? 8398);

const TRIPWIRE = 519;
const SIGNAL_TRAP = 1315;
const SECURITY_DROID = 325;
const DOORS = [514, 515] as const;

test(
  "ops4: IRobot response removes Docile and clears the Command Center entrance (#1108)",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "ops4.mis",
      port,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 5 });

    const [tripwire] = await game.entities.byTemplate(TRIPWIRE);
    const [signalTrap] = await game.entities.byTemplate(SIGNAL_TRAP);
    const [droid] = await game.entities.byTemplate(SECURITY_DROID);
    assert.ok(tripwire, `expected Ops4 tripwire ${TRIPWIRE}`);
    assert.ok(signalTrap, `expected Ops4 AI Signal Trap ${SIGNAL_TRAP}`);
    assert.ok(droid, `expected Ops4 Security Droid ${SECURITY_DROID}`);
    const initialDroid = await game.entities.detail(droid.id);

    // Spatial setup stays north of the tripwire. Enter it with the bounded,
    // collision-valid player movement path that drives production sensors.
    await game.player.teleport({ x: 66.0, y: -9.556, z: -87.0 });
    await game.input.set("head.look", [0, 0]);
    await game.step({ frames: 5 });
    const move = async (stick: [number, number], frames: number) => {
      await game.input.set("right_hand.thumbstick", stick);
      await game.step({ frames });
    };
    await move([0, 1], 30);
    await move([0, 0], 60);

    const trace = (await game.messages.recent()).messages;
    assert.ok(
      trace.some(
        (message) =>
          message.payload === "TurnOn" &&
          message.from?.template_id === TRIPWIRE &&
          message.to.template_id === SIGNAL_TRAP,
      ),
      `tripwire must activate the authored signal trap: ${JSON.stringify(trace)}`,
    );
    assert.ok(
      trace.some(
        (message) =>
          message.payload === "Signal" &&
          message.to.template_id === SECURITY_DROID,
      ),
      `signal trap must deliver IRobot to the droid: ${JSON.stringify(trace)}`,
    );

    const openedDoors = await Promise.all(
      DOORS.map(async (template) => (await game.entities.byTemplate(template))[0]),
    );
    assert.ok(openedDoors.every(Boolean), "expected both Command Center door leaves");
    assert.ok(
      openedDoors[0].position[0] < 63 && openedDoors[1].position[0] > 69,
      `tripwire should fully spread both door leaves: ${JSON.stringify(openedDoors)}`,
    );

    // Draw the scripted Goto target away from the 3.2-unit opening. Before
    // the fix the droid remains near z=-93.5 in Idle, filling the doorway.
    await move([0, -1], 40);
    await move([0, 0], 600);
    const movedDroid = await game.entities.detail(droid.id);
    assert.ok(
      movedDroid.position[2] > -90.5,
      `IRobot droid must leave the doorway toward the player; got ${JSON.stringify(movedDroid)}`,
    );
    assert.ok(
      movedDroid.position[2] - initialDroid.position[2] > 3,
      `the droid's full collider must clear its authored doorway position: ` +
        `${JSON.stringify(initialDroid.position)} -> ${JSON.stringify(movedDroid.position)}`,
    );
    // Walk around the advancing droid and through the actual opening. No
    // teleport, direct moveTo, or injected script message after initial setup.
    await move([-1, 0], 8);
    await move([0, 1], 120);
    // Avoid relying on incidental collision deflection to align us with
    // the narrow doorway. Steer from observed position if its jamb catches us.
    for (let attempt = 0; attempt < 60; attempt++) {
      const p = await game.player.position();
      if (p.z < -96) break;
      const dx = 66 - p.x;
      const dz = -98 - p.z;
      const length = Math.hypot(dx, dz);
      await move(p.z < -90.5 && Math.abs(dx) > 0.2
        ? [-Math.sign(dx) * 0.35, 0]
        : [-dx / length, -dz / length], 6);
    }
    await move([0, 0], 10);
    const player = (await game.info()).player;
    assert.ok(player.position[2]! < -96, `must enter the Command Center: ${JSON.stringify(player.position)}`);
  },
);
