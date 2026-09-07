import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/types.js";
import { teleportVerified } from "./helpers/teleport.js";
import { clickUiElement } from "./helpers/ui.js";
import { vrClimbPull } from "./helpers/vr-climb.js";

const enabled = process.env.SHOCK2_E2E === "1";
const planar = (a: Vec3, b: Vec3) => Math.hypot(a[0] - b[0], a[2] - b[2]);
const close = (actual: number, expected: number, description: string) =>
  assert.ok(
    Math.abs(actual - expected) < 0.04,
    `${description}: ${actual} != ${expected}`,
  );

// The shipped GAMEPARAM speed table is [1.2,1.3,1.4,1.5,1.6,1.7,1.85,2].
// The port's Agility 1 input pace is 25 Dark units/s = 10 world units/s.
// Negative-first: the baseline moves 10 world units at both Agility 1 and 6.
for (const vr of [false, true]) {
  test(
    `Agility changes shared ${vr ? "VR" : "flat"} movement, without scaling turn or developer flight`,
    { skip: !enabled, timeout: 600_000 },
    async () => {
      await using game = await GameServer.launch({
        mission: "debug_minimal",
        debugFlags: vr ? ["--vr"] : [],
      });
      await game.step({ frames: 30 });
      const travel = async (
        agility: number,
        crouch: boolean,
        stick: [number, number],
      ) => {
        await game.input.set("right_hand.thumbstick", [0, 0]);
        await game.input.set("crouch", crouch ? 1 : 0);
        await game.player.setStats({ agility });
        await teleportVerified(game, { x: 0, y: 1.5, z: 0 });
        await game.input.set("head.look", [0, 0]);
        await game.step({ frames: 30 });
        await game.input.set("right_hand.thumbstick", stick);
        // Prime the queued kinematic target before measuring 60 full steps.
        await game.step({ frames: 1 });
        const before = (await game.info()).player.position;
        await game.step({ frames: 60 });
        await game.input.set("right_hand.thumbstick", [0, 0]);
        const after = (await game.info()).player.position;
        return planar(before, after);
      };
      const independentMotion = async () => {
        const before = (await game.info()).player.rotation;
        await game.input.set("left_hand.thumbstick", [1, 0]);
        await game.step({ frames: 15 });
        await game.input.set("left_hand.thumbstick", [0, 0]);
        const after = (await game.info()).player.rotation;
        const dot = Math.abs(
          before.reduce((sum, value, i) => sum + value * after[i], 0),
        );
        const turn = 2 * Math.acos(Math.min(1, dot));
        await game.camera.set({ position: [0, 5, 0], lookAt: [-10, 5, 0] });
        const pawn = (await game.info()).player.position;
        const start = (await game.camera.state()).eye_position!;
        await game.input.set("right_hand.thumbstick", [0, 1]);
        await game.step({ frames: 60 });
        await game.input.set("right_hand.thumbstick", [0, 0]);
        const flight = planar(start, (await game.camera.state()).eye_position!);
        close(
          planar(pawn, (await game.info()).player.position),
          0,
          "free camera keeps pawn still",
        );
        await game.camera.attach();
        return { turn, flight };
      };
      const low = await travel(1, false, [0, 1]);
      const initial = await independentMotion();
      const middle = await travel(2, false, [0, 1]);
      close(
        await travel(2, true, [1, 0]),
        (10 * 1.3) / 1.2,
        "crouched strafe keeps Agility scaling",
      );
      close(
        await travel(2, false, [0, -0.5]),
        (5 * 1.3) / 1.2,
        "partial reverse stays proportional",
      );
      const high = await travel(6, false, [0, 1]);
      const upgraded = await independentMotion();
      console.info("Measured Agility movement", {
        vr,
        level1: low,
        level2: middle,
        level6: high,
      });
      close(low, 10, "established Agility1 pace");
      close(middle / low, 1.3 / 1.2, "Agility2 authored ratio");
      close(high, (10 * 1.7) / 1.2, "Agility6 pace");
      close(initial.turn, 0.5, "ordinary turn rate");
      close(upgraded.turn, initial.turn, "Agility leaves turn unchanged");
      close(initial.flight, 10, "developer flight retains its pace");
      close(
        upgraded.flight,
        initial.flight,
        "Agility leaves developer flight unchanged",
      );
    },
  );
}

test(
  "native Agility upgrade charges authored cost, changes movement, and survives current-build save/load",
  { skip: !enabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "medsci1.mis" });
    await game.step({ frames: 5 });
    await game.player.setStats({ cyber_modules: 3, agility: 1 });
    const trainer = (
      await game.entities.list({ filter: "Trainer" })
    ).entities.find((e) => e.template_id === 1352);
    assert.ok(trainer);
    const [x, y, z] = (await game.entities.detail(trainer.id)).position;
    // A short unobstructed path in the same trainer room avoids replacing the
    // purchased character with a debug scene. No campaign save is involved.
    const measure = async () => {
      await game.input.set("right_hand.thumbstick", [0, 0]);
      await teleportVerified(game, { x, y: y + 0.5, z: z + 1.2 });
      await game.step({ frames: 30 });
      const player = (await game.info()).player;
      await game.input.lookAtWorldPoint([
        player.position[0],
        player.position[1] + player.camera_offset[1],
        player.position[2] + 10,
      ]);
      await game.input.set("right_hand.thumbstick", [0, 0.1]);
      await game.step({ frames: 1 });
      const before = (await game.info()).player.position;
      await game.step({ frames: 12 });
      await game.input.set("right_hand.thumbstick", [0, 0]);
      return planar(before, (await game.info()).player.position);
    };
    const baseMoved = await measure();
    close(baseMoved, 0.2, "level-1 movement before the native purchase");
    await teleportVerified(game, { x, y: y + 0.5, z: z + 1.2 });
    await game.step({ frames: 30 });
    await game.entities.sendMessage(trainer.id, { type: "Frob" });
    await game.step({ frames: 5 });
    const panel = (await game.ui.state()).active_panel;
    assert.ok(panel);
    const button = panel.elements.find(
      (e) => e.kind === "button" && e.label === "Agility",
    );
    assert.ok(button);
    await clickUiElement(game, button);
    assert.equal(
      (await game.info()).player.stats?.agility,
      2,
      "native purchase must apply Agility",
    );
    assert.equal(
      (await game.info()).player.stats?.cyber_modules,
      0,
      "1→2 costs three authored modules",
    );
    const moved = await measure();
    assert.ok(
      Math.abs(moved / baseMoved - 1.3 / 1.2) < 0.005,
      `native purchase must increase movement by the authored ratio: ${baseMoved}→${moved}`,
    );
    close(
      moved,
      (0.2 * 1.3) / 1.2,
      "purchased Agility changes actual movement",
    );
    await game.step({ frames: 10 });
    const name = `agility_e2e_${Date.now()}`;
    await game.save(name);
    await game.player.setStats({ agility: 6 });
    await game.load(name);
    assert.equal((await game.info()).player.stats?.agility, 2);
    assert.equal((await game.info()).player.stats?.cyber_modules, 0);
    const loadedMoved = await measure();
    assert.ok(
      Math.abs(loadedMoved / moved - 1) < 0.005,
      `loaded Agility keeps actual movement effect: ${moved}→${loadedMoved}`,
    );
  },
);

test(
  "Agility preserves physical VR hand pull distance and ordinary flat ladder redirection",
  { skip: !enabled, timeout: 600_000 },
  async () => {
    for (const vr of [true, false]) {
      await using game = await GameServer.launch({
        mission: "debug_ladder",
        debugFlags: vr ? ["--vr"] : [],
      });
      for (const agility of [1, 6]) {
        await game.input.set("right_hand.squeeze", 0);
        await game.input.set("right_hand.thumbstick", [0, 0]);
        await game.step({ frames: 1 });
        await game.player.setStats({ agility });
        await teleportVerified(game, { x: -5.5, y: 1.5, z: 0 });
        await game.step({ frames: 30 });
        if (vr) {
          const result = await vrClimbPull(game, {
            grabAt: [-6.8, 2.6, 0],
            pull: [0, -1, 0],
            frames: 30,
          });
          close(
            result.after[1] - result.before[1],
            1,
            "Agility cannot amplify a physical hand pull",
          );
          assert.equal(
            (await game.info()).player.climb.grips[0]?.kind,
            "ladder",
          );
        } else {
          const before = (await game.info()).player.position;
          const eye = (await game.info()).player.camera_offset[1];
          await game.input.lookAtWorldPoint([-7, before[1] + eye, 0]);
          await game.input.set("right_hand.thumbstick", [0, 1]);
          await game.step({ frames: 30 });
          await game.input.set("right_hand.thumbstick", [0, 0]);
          const after = (await game.info()).player.position;
          assert.ok(
            after[1] - before[1] > 0.7,
            "flat input still redirects up the authored ladder",
          );
          assert.ok(after[0] > -7, "ladder backing remains solid");
        }
      }
    }
  },
);
