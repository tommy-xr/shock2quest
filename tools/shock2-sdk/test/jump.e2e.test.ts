import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, PLAYER_EYE_HEIGHT_WORLD } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

async function pulseJump(game: GameServer): Promise<void> {
  await game.input.setJump(true);
  await game.step({ frames: 1 });
  await game.input.setJump(false);
}

// The old #708 fixture jumped through the ceiling of the enclosed outer
// corridor at (28.14,-0.48,31.64), then descended outside the real shaft.
// Retail enters beside log 3, follows the spiral, and jumps into its opening.
// Keep this positive route when tightening ceiling rejection (#2071).
test(
  "ordinary VR movement follows SHODAN's spiral and jumps into the log 4 passage",
  { skip: !e2eEnabled, timeout: 600_000 },
  async (t) => {
    await using game = await GameServer.launch({
      mission: "shodan.mis",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 5 });
    // Setup at log 3 only. Every subsequent position uses production input.
    await game.player.teleport({ x: 33.6, y: 1.25, z: 15 });
    await game.step({ frames: 30 });
    await game.save("jump-shodan-authored-entrance");
    await game.load("jump-shodan-authored-entrance");
    await game.step({ frames: 30 });

    async function walkTo(x: number, z: number, expectedY: number): Promise<void> {
      for (let frame = 0; frame < 80; frame++) {
        const position = await game.player.position();
        if (Math.hypot(x - position.x, z - position.z) < 0.2) break;
        await game.input.lookAtWorldPoint([x, position.y + PLAYER_EYE_HEIGHT_WORLD, z]);
        await game.input.set("right_hand.thumbstick", [0, 1]);
        await game.step({ frames: 1 });
      }
      await game.input.set("right_hand.thumbstick", [0, 0]);
      await game.step({ frames: 90 });
      const landed = await game.player.position();
      assert.ok(
        Math.hypot(x - landed.x, z - landed.z) < 0.25 &&
          Math.abs(landed.y - expectedY) < 0.15,
        `authored step (${x},${expectedY},${z}) must support the player: ${JSON.stringify(landed)}`,
      );
    }

    // Seven descending treads wrap around the column twice. The eighth tread
    // lies below the passage entrance; jump from the seventh through the
    // visible opening instead of dropping past it.
    for (const [x, z, y] of [
      [33.2, 9, 1.244], [35, 7.5, -1.956], [35, 4.8, -1.956],
      [32, 3.2, -5.156], [29, 4.8, -8.356], [29, 7, -8.356],
      [32, 9.5, -11.556], [35, 7.5, -14.756], [35, 4.8, -14.756],
      [32, 3.2, -17.956], [29, 4.8, -21.156], [29, 7, -21.156],
    ]) await walkTo(x!, z!, y!);

    const seventhStep = await game.player.position();
    await game.input.lookAtWorldPoint([32, seventhStep.y + PLAYER_EYE_HEIGHT_WORLD, 14]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await pulseJump(game);
    await game.step({ frames: 59 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.step({ frames: 120 });
    const entrance = await game.player.position();
    assert.ok(
      entrance.x > 31 && entrance.x < 34 && entrance.z > 11.2 &&
        Math.abs(entrance.y + 21.156) < 0.15,
      `ordinary jump must land inside the open passage: ${JSON.stringify(entrance)}`,
    );
    await walkTo(32, 20, -22.31);
    await walkTo(32, 27, -26.63);
    const passage = await game.player.position();
    await game.step({ frames: 120 });
    const supported = await game.player.position();
    assert.ok(
      Math.hypot(supported.x - passage.x, supported.y - passage.y, supported.z - passage.z) < 0.05,
      `log 4 approach must remain supported: ${JSON.stringify({ passage, supported })}`,
    );
    const [log4] = await game.entities.byTemplate(290);
    assert.ok(log4?.position, "authored Delacroix log 4 must exist");
    assert.ok(Math.hypot(supported.x - log4.position[0]!, supported.y - log4.position[1]!, supported.z - log4.position[2]!) < 2);
    assert.equal((await game.info()).player.life_state, "alive");
    t.diagnostic(`authored spiral: ${JSON.stringify({ seventhStep, entrance, supported })}`);
  },
);

// Issue #1085: Engineering's reverse route near Aux Storage 5 has a normal
// one-foot riser on the upper floor and a disconnected floor about 30.4 SS2
// feet below the same forward footprint. The old stacked-terrain fallback
// compressed a crouched player and scripted them through the still-solid upper
// floor. Setup teleport only stages the campaign-confirmed pre-jump pose; the
// crossing itself is the exact production VR crouch/jump/locomotion sequence.
test(
  "crouched jump at Engineering reverse lip stays on the upper route",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "eng1.mis",
      port: Number(process.env.SHOCK2_E2E_ENG1_REVERSE_LIP_PORT ?? 8143),
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 5 });

    // Stage left of the nearby hybrid so this probes terrain, not an actor.
    // Standing center over the authored y=-13.2 start floor. Crouching plants
    // the feet and produces the campaign-observed center near y=-12.596.
    await game.player.teleport({ x: 6.75, y: -11.956, z: -128.4 });
    await game.step({ frames: 30 });
    await game.input.set("crouch", 1);
    await game.step({ frames: 5 });
    const start = await game.player.position();

    // Make two ordinary crouched jumps across the 0.4-world-unit upper lip.
    // At this unobstructed approach the first jump already selects the
    // disconnected lower floor on the unfixed controller.
    await game.input.lookAtWorldPoint([
      start.x,
      start.y + PLAYER_EYE_HEIGHT_WORLD,
      start.z - 4,
    ]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await pulseJump(game);
    await game.step({ frames: 7 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.step({ frames: 120 });
    const upperLip = await game.player.position();
    assert.ok(
      upperLip.y > -12.3 && upperLip.z < -129.2,
      `first jump must stage the authored upper lip (${JSON.stringify(start)} -> ${JSON.stringify(upperLip)})`,
    );

    await game.input.lookAtWorldPoint([
      upperLip.x,
      upperLip.y + PLAYER_EYE_HEIGHT_WORLD,
      upperLip.z - 4,
    ]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await pulseJump(game);
    await game.step({ frames: 7 });
    await game.input.set("right_hand.thumbstick", [0, 0]);

    let minimumCenterY = (await game.player.position()).y;
    for (let frame = 0; frame < 300; frame += 1) {
      await game.step({ frames: 1 });
      minimumCenterY = Math.min(minimumCenterY, (await game.player.position()).y);
    }
    const settled = await game.player.position();
    await game.step({ frames: 120 });
    const supported = await game.player.position();

    assert.ok(
      minimumCenterY > -12.8 &&
        settled.y > -12.8 &&
        supported.y > -12.8 &&
        Math.abs(supported.y - settled.y) < 0.05,
      `ordinary jump must remain above Engineering's solid y=-12.8 upper floor ` +
        `(${JSON.stringify(start)} -> ${JSON.stringify(upperLip)} -> ` +
        `${JSON.stringify(settled)} -> ` +
        `${JSON.stringify(supported)}, minimum center y=${minimumCenterY})`,
    );
  },
);

// Engineering campaign `engineering · none · legacy · seed 1378752978`
// reached this authored route from a clean playthrough. The shipped walk graph
// continues through the Cargo 2 crate stack, but the parented mission crates
// close its sub-player-width seam. Retail's ordinary jump is required to get
// over the stack and continue toward Sanger on the top floor of Cargo 2B.
test(
  "ordinary jump clears Engineering Cargo 2's route-blocking crate stack",
  { skip: !e2eEnabled, timeout: 600_000 },
  async (t) => {
    await using game = await GameServer.launch({
      mission: "eng2.mis",
    });
    await game.step({ frames: 5 });

    const entities = (await game.entities.list()).entities;
    const bigCrate = entities.find((entity) => entity.template_id === 434);
    const lowerSmallCrate = entities.find((entity) => entity.template_id === 435);
    const upperSmallCrate = entities.find((entity) => entity.template_id === 436);
    const sangerCorpse = entities.find((entity) => entity.template_id === 507);
    assert.ok(bigCrate, "eng2 should contain mission big crate 434");
    assert.ok(lowerSmallCrate, "eng2 should contain mission small crate 435");
    assert.ok(upperSmallCrate, "eng2 should contain mission small crate 436");
    assert.ok(sangerCorpse, "eng2 should contain Sanger corpse mission 507");

    // Setup only: exact cold-loadable campaign frontier immediately south of
    // the crates. Everything after settling here uses production look,
    // locomotion, and jump channels.
    await game.player.teleport({
      x: 26.339931,
      y: -11.795994,
      z: -202.49998,
    });
    await game.step({ frames: 30 });
    const before = await game.player.position();
    const initialCorpseDistance = Math.hypot(
      before.x - sangerCorpse.position[0],
      before.z - sangerCorpse.position[2],
    );

    // Aim over the stacked small crates toward the first shipped-path point
    // beyond them, then keep ordinary forward locomotion active for the jump.
    await game.input.lookAtWorldPoint([
      lowerSmallCrate.position[0],
      before.y + PLAYER_EYE_HEIGHT_WORLD,
      -195.8,
    ]);
    await game.input.set("right_hand.thumbstick", [0, 1]);

    // The authentic stack is a two-hop route: the first jump reaches the
    // stable top of the small crates, replacing the unavailable one-shot
    // mantle the campaign player initially tried.
    await pulseJump(game);
    await game.step({ frames: 180 });
    const onCrates = await game.player.position();
    assert.ok(
      onCrates.y > before.y + 2 &&
        onCrates.z > -199 &&
        onCrates.z < -197,
      `first production jump should reach the stable crate-top foothold ` +
        `(${JSON.stringify(before)} -> ${JSON.stringify(onCrates)})`,
    );

    // Jump again from that production-reached foothold into Cargo 2. No
    // relocation or validated debug move occurs after the campaign frontier.
    await pulseJump(game);
    let crossed = onCrates;
    for (let frame = 0; frame < 180; frame += 1) {
      await game.step({ frames: 1 });
      crossed = await game.player.position();
      if (crossed.z > -196.5) {
        break;
      }
    }
    await game.input.set("right_hand.thumbstick", [0, 0]);

    assert.ok(
      crossed.z > -196.5,
      `production jump should clear crates 434/435/436 ` +
        `(${JSON.stringify(before)} -> ${JSON.stringify(crossed)})`,
    );

    // The crossing must finish on the ordinary supported Cargo 2 floor, not
    // merely sample an airborne point above or beside the crates. Two long
    // zero-input windows also make this checkpoint safe for campaign replay.
    await game.step({ frames: 300 });
    const landed = await game.player.position();
    await game.step({ frames: 120 });
    const supported = await game.player.position();
    const finalCorpseDistance = Math.hypot(
      supported.x - sangerCorpse.position[0],
      supported.z - sangerCorpse.position[2],
    );
    assert.ok(
      landed.z > -196.5 &&
        supported.z > -196.5 &&
        Math.hypot(
          supported.x - landed.x,
          supported.y - landed.y,
          supported.z - landed.z,
        ) < 0.05 &&
        finalCorpseDistance < initialCorpseDistance - 4,
      `post-stack Cargo 2 checkpoint should remain supported and closer to ` +
        `Sanger corpse 507 (${JSON.stringify(landed)} -> ` +
        `${JSON.stringify(supported)}, corpse distance ` +
        `${initialCorpseDistance.toFixed(2)} -> ${finalCorpseDistance.toFixed(2)})`,
    );
    t.diagnostic(
      `Engineering Cargo 2: frontier ${JSON.stringify(before)}, ` +
        `crate top ${JSON.stringify(onCrates)}, crossing ${JSON.stringify(crossed)}, ` +
        `supported ${JSON.stringify(landed)} -> ${JSON.stringify(supported)}, ` +
        `corpse distance ${initialCorpseDistance.toFixed(2)} -> ` +
        `${finalCorpseDistance.toFixed(2)}`,
    );
  },
);

// The same ordinary jump-through semantics are also required by the optional
// Delacroix log 2 route: the upper floors are ceilings to the stacked corridor
// cells below. Teleport only stages the player on the real lower floor; both
// ascents and the log frob use production input.
test(
  "ordinary jumps reach and collect shodan's two-platform Delacroix log",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "shodan.mis",
    });
    await game.step({ frames: 5 });

    const log = (
      await game.entities.list({ filter: "Audio Log", limit: 80 })
    ).entities.find((entity) => entity.template_id === 606);
    assert.ok(log, "shodan should contain Delacroix deck 9/log 2");

    await game.player.teleport({ x: 5, y: 0.2, z: 9 });
    await game.step({ frames: 30 });
    let position = await game.player.position();

    // First authored platform: cross its south lip toward world +Z.
    await game.input.lookAtWorldPoint([5, position.y + PLAYER_EYE_HEIGHT_WORLD, 13]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await pulseJump(game);
    await game.step({ frames: 90 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.step({ frames: 20 });
    position = await game.player.position();
    assert.ok(
      position.y > 3.9 && position.y < 4.2,
      `first jump should stand on the y=2.8 platform, got ${JSON.stringify(position)}`,
    );

    // Second platform: face the discovered log, jump toward it, and arrive on
    // the y=6 authored floor without teleporting between platforms.
    await game.input.lookAtWorldPoint([
      log.position[0],
      position.y + PLAYER_EYE_HEIGHT_WORLD,
      log.position[2],
    ]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await pulseJump(game);
    await game.step({ frames: 90 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.step({ frames: 20 });
    position = await game.player.position();
    assert.ok(
      position.y > 7.1 &&
        position.y < 7.4 &&
        Math.hypot(position.x - log.position[0], position.z - log.position[2]) <
          2,
      `second jump should reach the log platform, got ${JSON.stringify(position)}`,
    );

    // Resolve the entity again because runtime ids are launch-local, then use
    // the normal flat crosshair + squeeze interaction to collect it.
    const liveLog = (
      await game.entities.list({ filter: "Audio Log", limit: 80 })
    ).entities.find((entity) => entity.template_id === 606);
    assert.ok(liveLog, "Delacroix log should remain present before collection");
    await game.player.aimAt(liveLog, { visibility: "required" });
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 1 });
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 5 });
    assert.ok(
      (await game.info()).player.collected_logs.some(
        (entry) => entry.deck === 9 && entry.log === 2,
      ),
      "production squeeze should collect Delacroix deck 9/log 2",
    );
  },
);
