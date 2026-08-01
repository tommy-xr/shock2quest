import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

async function pulseJump(game: GameServer): Promise<void> {
  await game.input.setJump(true);
  await game.step({ frames: 1 });
  await game.input.setJump(false);
}

// Issue #708: shodan.mis's mandatory final descent begins behind a low
// non-climbable barrier at z=32. Ordinary walking, crouching, and the
// collision-valid move endpoint all stop at its face; retail expects the
// player to jump over it and land on the authored ledges below.
test(
  "ordinary jumps traverse shodan's final descent into the log 4 passage",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "shodan.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8136),
    });
    await game.step({ frames: 5 });

    // Setup only: place the player at the campaign-confirmed approach. The
    // crossing itself uses only the production jump and locomotion channels.
    await game.player.teleport({
      x: 28.14371,
      y: -0.5959,
      z: 31.63964,
    });
    await game.step({ frames: 30 });
    const before = await game.player.position();

    // A save at the authored frontier must reload to a normal grounded player,
    // not preserve a stale airborne edge or lose the newly available input.
    await game.save("jump-shodan-final-barrier");
    await game.step({ frames: 2 });
    await game.load("jump-shodan-final-barrier");
    await game.step({ frames: 30 });
    const restored = await game.player.position();
    assert.ok(
      Math.hypot(restored.x - before.x, restored.z - before.z) < 0.1,
      `save/load should preserve the jump approach (${JSON.stringify(before)} -> ` +
        `${JSON.stringify(restored)})`,
    );

    // Face world +Z irrespective of the mission's authored pawn rotation,
    // then press ordinary forward locomotion through the jump.
    await game.input.lookAtWorldPoint([
      restored.x,
      restored.y + 1.6,
      restored.z + 4,
    ]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await pulseJump(game);
    await game.step({ frames: 59 });
    await game.input.set("right_hand.thumbstick", [0, 0]);

    const crossed = await game.player.position();
    assert.ok(
      crossed.z > 32.3,
      `jump should clear the z=32 barrier (started ${JSON.stringify(before)}, ` +
        `ended ${JSON.stringify(crossed)})`,
    );

    // Fall straight onto the first authored sloping ledge. Contact sharply
    // slows the old 0.2-unit/frame free fall; five more frames remain within
    // one half unit vertically while the walkable surface slides the
    // controller a little down-slope.
    await game.step({ frames: 111 });
    const landed = await game.player.position();
    await game.step({ frames: 5 });
    const supported = await game.player.position();
    assert.ok(
      landed.y > -19 && landed.y < -18 &&
        Math.abs(supported.y - landed.y) < 0.5,
      `player should land on the first authored lower ledge ` +
        `(contact ${JSON.stringify(landed)}, after five frames ${JSON.stringify(supported)})`,
    );

    // A short diagonal jump reaches the stacked upper floor. From here the
    // mandatory route wraps under its east lip onto a finite lower side ring;
    // a continuous standing capsule cannot expose that ring through an
    // ordinary ballistic edge fall.
    await game.input.lookAtWorldPoint([31, supported.y + 1.6, 28.5]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await pulseJump(game);
    await game.step({ frames: 24 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.step({ frames: 60 });
    let position = await game.player.position();
    assert.ok(
      position.y > -18.2 && position.y < -17.7,
      `diagonal jump should reach the stacked upper floor, got ${JSON.stringify(position)}`,
    );

    // Walk to the real east lip and align over the narrow z=30..32 side ring.
    // There is no setup placement after the initial campaign frontier: every
    // pose in the crossing is reached through production locomotion.
    await game.input.lookAtWorldPoint([35, position.y + 1.6, 31]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.step({ frames: 29 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    position = await game.player.position();
    assert.ok(
      position.x > 34.7 &&
        position.x < 35.8 &&
        position.z > 30.8 &&
        position.z < 31.3,
      `production movement should stage at the east lip, got ${JSON.stringify(position)}`,
    );

    // The lower ring immediately enters a crouch-only passage. Crouch before
    // jumping so the bounded sparse-body transition can restore the same
    // collision profile under the stacked upper-floor ceiling; it must never
    // expand a standing capsule into that headroom.
    await game.input.set("crouch", 1);
    await game.step({ frames: 5 });
    position = await game.player.position();

    // A forward jump at the parentless lip uses the bounded sparse-body
    // transition to the first all-collider-valid crouched pose below. Before
    // the downward transition this pulse simply landed back on y=-19.2.
    await game.input.lookAtWorldPoint([40, position.y + 1.6, position.z]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await pulseJump(game);
    await game.input.set("right_hand.thumbstick", [0, 0]);
    // Do not mistake the scripted transition passing through the ring's
    // height for a landing. With no steering input, it must settle on authored
    // collision for ten seconds and remain stationary for two more.
    await game.step({ frames: 600 });
    const lowerRing = await game.player.position();
    await game.step({ frames: 120 });
    const supportedLowerRing = await game.player.position();
    const onLowerRing = (sample: typeof lowerRing): boolean =>
      sample.x > 35.2 &&
      sample.x < 36.8 &&
      sample.y > -32.5 &&
      sample.y < -31.5 &&
      sample.z > 30.2 &&
      sample.z < 31.8;
    assert.ok(
      onLowerRing(lowerRing) &&
        onLowerRing(supportedLowerRing) &&
        Math.hypot(
          supportedLowerRing.x - lowerRing.x,
          supportedLowerRing.y - lowerRing.y,
          supportedLowerRing.z - lowerRing.z,
        ) < 0.05,
      `jump should remain supported on the finite lower side ring ` +
        `(${JSON.stringify(lowerRing)} -> ${JSON.stringify(supportedLowerRing)})`,
    );
    position = supportedLowerRing;

    // Follow the authored crouch-only continuation around the outer north edge
    // at z~=29, then west toward Delacroix log 4. This proves the landing opens
    // the real route rather than merely finding an isolated point below.
    await game.input.lookAtWorldPoint([
      position.x,
      position.y + 1.6,
      28,
    ]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.step({ frames: 18 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.step({ frames: 5 });
    position = await game.player.position();
    assert.ok(
      position.z > 28.5 && position.z < 28.8,
      `crouched movement should round the outer north edge, got ${JSON.stringify(position)}`,
    );
    await game.input.lookAtWorldPoint(
      [30, position.y + 1.1, position.z],
      { eyeHeight: 1.1 },
    );
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.step({ frames: 20 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    // The central passage floor is a finite authored tread at z=28.5..28.8.
    // Release input and prove the apparent entry is support rather than a
    // mid-fall sample over the adjacent void.
    await game.step({ frames: 600 });
    const passage = await game.player.position();
    await game.step({ frames: 120 });
    const supportedPassage = await game.player.position();
    const inPassage = (sample: typeof passage): boolean =>
      sample.x > 31 &&
      sample.x < 33 &&
      // Player positions are capsule centers: crouched half-height is 0.56
      // world units, putting the feet on the mapped y=-33.6..-33.3 tread.
      sample.y > -33.04 &&
      sample.y < -32.74 &&
      sample.z > 28.5 &&
      sample.z < 28.8;
    assert.ok(
      inPassage(passage) &&
        inPassage(supportedPassage) &&
        Math.hypot(
          supportedPassage.x - passage.x,
          supportedPassage.y - passage.y,
          supportedPassage.z - passage.z,
        ) < 0.05,
      `crouched production movement should remain supported in the log 4 passage ` +
        `(${JSON.stringify(passage)} -> ${JSON.stringify(supportedPassage)})`,
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
      port: Number(process.env.SHOCK2_E2E_ENG2_JUMP_PORT ?? 8138),
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
      before.y + 1.6,
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
      port: Number(process.env.SHOCK2_E2E_LOG_PORT ?? 8137),
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
    await game.input.lookAtWorldPoint([5, position.y + 1.6, 13]);
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
      position.y + 1.6,
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
