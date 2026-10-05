import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { setHandWorldPose } from "../src/vr-pose.js";
import { add, aimVrHandAt, quatConjugate, quatRotate, sub } from "./helpers/vr-hand.js";
import { drawPouchAmmo } from "./helpers/ammo-pouch.js";
import { ammoOf, pullTrigger } from "./helpers/weapon.js";

const enabled = process.env.SHOCK2_E2E === "1";
test("a full gun offers a spare clip whose B-cycle returns rounds to their stack", {
  skip: !enabled, timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  const gun = (await game.entities.list()).entities.find(e => e.template_id === -17)!;
  await aimVrHandAt(game, gun.position, .2, 1, 0, { hand: "left" });
  await game.input.set("left_hand.position", [-.3, 1, -.5]);
  await game.input.set("left_hand.rotation", [0, 0, 0, 1]);
  const standard = await game.player.spawnItem(-1358);
  await game.player.spawnItem(-31);
  await game.player.spawnItem(-31);
  const he = await game.player.spawnItem(-32);
  await game.step({ frames: 5 });
  assert.equal(ammoOf(await game.entities.detail(gun.id)), 12);
  const { offer, entityId: clip } = await drawPouchAmmo(game, "right");
  assert.equal(offer.rounds, 12, "a full gun still offers a full spare clip");
  await game.input.trigger("RightHandUpperButton"); // B on the clip hand.
  await game.step({ frames: 5 });
  assert.equal((await game.info()).player.right_hand_entity_id, he.entity_id);
  assert.equal(ammoOf(await game.entities.detail(gun.id)), 12, "cycling the held clip does not unload the gun");
  const returned = (await game.entities.detail(standard.entity_id)).properties.find(p => p.name === "StackCount");
  assert.equal(Number(returned?.value), 30, "cycling returns the spare rounds to the existing reserve stack");
  assert.ok(!(await game.entities.list()).entities.some(e => e.id === clip), "the returned split clip must merge away");
});

for (const [hand, spent] of [["left", 5], ["right", 9]] as const) {
  test(`${hand} pouch top-off consumes the drawn clip without leftovers`, { skip: !enabled, timeout: 180_000 }, async () => {
    await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
    await game.step({ frames: 30 });
    const primary = hand === "left" ? "right" : "left";
    const gun = (await game.entities.list()).entities.find(e => e.template_id === -17)!;
    await aimVrHandAt(game, gun.position, .2, 1, 0, { hand: primary });
    await game.input.set(`${primary}_hand.position`, [primary === "left" ? -.3 : .3, 1, -.5]);
    await game.input.set(`${primary}_hand.rotation`, [0, 0, 0, 1]);
    await game.step({ frames: 10 });
    const capacity = ammoOf(await game.entities.detail(gun.id));
    for (let i = 0; i < spent; i++) {
      await pullTrigger(game, primary);
      await game.step({ frames: 60 });
    }
    assert.equal(ammoOf(await game.entities.detail(gun.id)), capacity - spent);
    const reserve = await game.player.spawnItem(-1358);
    await game.player.spawnItem(-31);
    await game.step({ frames: 3 });
    const rounds = async (id: number) => Number((await game.entities.detail(id)).properties.find(p => p.name === "StackCount")?.value);
    const stock = await rounds(reserve.entity_id);
    const { offer, player: held, entityId: clip } = await drawPouchAmmo(game, hand);
    assert.equal(offer.rounds, spent, "draw the magazine's missing rounds even from pooled small boxes");
    assert.equal(await rounds(clip), spent);
    assert.equal(await rounds(reserve.entity_id), stock - spent);
    const grip = held.hand_grips.find(g => g.entity_id === clip)?.grip;
    assert.ok(grip);
    const anchor = (await game.entities.detail(gun.id)).magazine_anchor;
    assert.ok(anchor);
    const local = quatRotate(quatConjugate(held.rotation), sub(anchor, held.position));
    await game.input.set(`${hand}_hand.position`, sub(local, [grip.offset.x, grip.offset.y, grip.offset.z]));
    await game.step({ frames: 5 });
    assert.equal(ammoOf(await game.entities.detail(gun.id)), capacity);
    const final = (await game.info()).player;
    assert.equal(hand === "left" ? final.wielded_entity_id : final.right_hand_entity_id, null);
    assert.ok(!(await game.entities.list()).entities.some(e => e.id === clip));
    assert.equal(await rounds(reserve.entity_id) + capacity, stock + capacity - spent, "reload conserves rounds");
  });
}

for (const hand of ["left", "right"] as const) {
  test(`${hand} pouch draws real reserve and returns it without duplication`, { skip: !enabled, timeout: 180_000 }, async () => {
    await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
    await game.step({ frames: 30 });
    const gunHand = hand === "left" ? "right" : "left";
    const owner = hand === "left" ? "wielded_entity_id" : "right_hand_entity_id";
    const pistol = (await game.entities.list()).entities.find(e => e.template_id === -17);
    assert.ok(pistol);
    await aimVrHandAt(game, pistol.position, 0.2, 1, 0, { hand: gunHand });
    await game.step({ frames: 5 });
    const reserve = await game.player.spawnItem(-31);
    if (hand === "right") {
      let full = false;
      for (let i = 0; i < 50; i++) {
        try { await game.player.spawnItem(-1221); }
        catch (error) {
          assert.match(String(error), /could not add item to inventory/);
          full = true;
          break;
        }
      }
      assert.ok(full, "test must fill the backpack");
      await game.player.spawnItem(-31); // Matching ammo merges into the full pack.
    }
    await game.step({ frames: 5 });
    const player = (await game.info()).player;
    const center = player.hand_feedback?.ammo_pouch?.center;
    assert.ok(center);
    const i = hand === "left" ? 0 : 1;
    const offer = player.hand_feedback?.ammo_pouch?.offers[i];
    assert.ok(offer, "opposite gun must offer compatible reserve");
    assert.equal(offer.reserve, reserve.entity_id);
    await game.input.set(`${hand}_hand.position`, quatRotate(quatConjugate(player.rotation), sub(center, player.position)));
    await game.input.set(`${hand}_hand.rotation`, [0, 0, 0, 1]);
    await game.input.set(`${hand}_hand.squeeze`, 0);
    await game.step({ frames: 5 });
    // Body slots use the calibrated palm, not the controller origin. Center
    // that contact on the pouch so the nearby access card cannot win the grab.
    const reached = (await game.info()).player;
    const palm = reached.hand_feedback?.glove_contacts?.centers[i];
    const pouchCenter = reached.hand_feedback?.ammo_pouch?.center;
    assert.ok(palm && pouchCenter);
    await setHandWorldPose(game, reached, hand, add(center, sub(pouchCenter, palm)), reached.rotation);
    await game.step({ frames: 3 });
    assert.equal((await game.info()).player[owner], null, "reach alone must not draw");
    const pickupsBefore = (await game.audio.recent()).sounds.filter(s => s.sample === "pickup").length;
    assert.ok((await game.info()).player.hand_feedback!.anticipation[i].curls.some(c => c > 0.03), "available pouch prepares an empty hand");
    await game.input.set(`${hand}_hand.squeeze`, 1);
    await game.step({ frames: 8 });
    assert.equal((await game.audio.recent()).sounds.filter(s => s.sample === "pickup").length, pickupsBefore,
      "drawing owned reserve, including a split clip, must not add a world-pickup cue");
    const drawn = (await game.info()).player[owner];
    assert.ok(drawn !== null);
    if (hand === "left") assert.equal(drawn, reserve.entity_id, "one clip moves the exact reserve entity");
    else {
      assert.ok(offer.stock > offer.rounds, "large reserve must be split");
      assert.notEqual(drawn, reserve.entity_id);
      const remaining = await game.entities.detail(reserve.entity_id);
      assert.equal(Number(remaining.properties.find(x => x.name === "StackCount")?.value), offer.stock - offer.rounds);
    }
    assert.equal((await game.player.inventory()).items.find(x => x.entity_id === drawn)?.location, `${hand}_hand`);
    await game.step({ frames: 20 });
    assert.equal((await game.info()).player[owner], drawn, "held squeeze does not duplicate");
    await game.input.set(`${hand}_hand.squeeze`, 0);
    await game.step({ frames: 8 });
    assert.equal((await game.info()).player[owner], null);
    assert.equal((await game.player.inventory()).items.filter(x => x.entity_id === reserve.entity_id && x.location === "inventory").length, 1);
    if (hand === "right") assert.ok(!(await game.entities.list()).entities.some(x => x.id === drawn), "returned split merges into the full pack");
    const detail = await game.entities.detail(reserve.entity_id);
    assert.equal(Number(detail.properties.find(x => x.name === "StackCount")?.value), offer.stock);
  });
}

test("empty pouch refuses selected ammo without grabbing nearby objects", { skip: !enabled, timeout: 180_000 }, async () => {
  await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  const pistol = (await game.entities.list()).entities.find(e => e.template_id === -17)!;
  await aimVrHandAt(game, pistol.position, 0.2, 1);
  await game.step({ frames: 5 });
  // An incompatible reserve must never be silently substituted.
  await game.player.spawnItem(-42);
  await game.step({ frames: 5 });
  const player = (await game.info()).player;
  const center = player.hand_feedback?.ammo_pouch?.center;
  assert.ok(center);
  assert.equal(player.hand_feedback?.ammo_pouch?.offers[0], null);
  const stock = await game.player.inventory();
  await game.input.set("left_hand.position", quatRotate(quatConjugate(player.rotation), sub(center, player.position)));
  await game.input.set("left_hand.squeeze", 0);
  await game.step({ frames: 3 });
  const before = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
  await game.input.set("left_hand.squeeze", 1);
  await game.step({ frames: 5 });
  assert.equal((await game.info()).player.wielded_entity_id, null);
  assert.deepEqual(await game.player.inventory(), stock);
  const sounds = (await game.audio.recent()).sounds.filter(s => s.sequence > before);
  assert.ok(sounds.length > 0, "empty pouch must provide audible feedback");
});
