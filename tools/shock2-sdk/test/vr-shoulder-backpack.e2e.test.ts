import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import {
  aimVrHandAt,
  aimVrHandAtCanvas,
  quatConjugate,
  quatRotate,
  sub,
} from "./helpers/vr-hand.js";
import { ammoOf } from "./helpers/weapon.js";

const enabled = process.env.SHOCK2_E2E === "1";
const owner = (hand: "left" | "right") =>
  hand === "left" ? "wielded_entity_id" : "right_hand_entity_id";
async function reach(
  game: GameServer,
  hand: "left" | "right",
  shoulder: number,
) {
  const player = (await game.info()).player;
  const center = player.hand_feedback?.shoulder_backpack?.centers?.[shoulder];
  assert.ok(center, "tracked shoulder zones must be published");
  await game.input.set(
    `${hand}_hand.position`,
    quatRotate(quatConjugate(player.rotation), sub(center, player.position)),
  );
  await game.input.set(`${hand}_hand.rotation`, [0, 0, 0, 1]);
  await game.step({ frames: 5 });
  assert.equal(
    (await game.info()).player.hand_feedback?.shoulder_backpack?.near[
      hand === "left" ? 0 : 1
    ],
    true,
  );
}

for (const [hand, template] of [
  ["left", -1221],
  ["right", -17],
] as const) {
  test(
    `${hand} shoulder release stores the exact held item and permits retrieval`,
    { skip: !enabled, timeout: 180_000 },
    async () => {
      await using game = await GameServer.launch({
        mission: "debug_interactions",
        debugFlags: ["--vr"],
      });
      await game.step({ frames: 30 });
      const item = (await game.entities.list()).entities.find(
        (e) => e.template_id === template,
      )!;
      assert.ok(item);
      await aimVrHandAt(game, item.position, 0.2, 1, 0, { hand });
      await game.step({ frames: 5 });
      assert.equal((await game.info()).player[owner(hand)], item.id);
      const ammo =
        template === -17 ? ammoOf(await game.entities.detail(item.id)) : null;
      await reach(game, hand, hand === "left" ? 1 : 0); // either shoulder accepts either hand
      assert.equal(
        (await game.info()).player[owner(hand)],
        item.id,
        "reaching while squeezed must not stow",
      );
      await game.input.set(`${hand}_hand.squeeze`, 0);
      await game.step({ frames: 8 });
      assert.equal((await game.info()).player[owner(hand)], null);
      assert.equal(
        (await game.player.inventory()).items.find(
          (i) => i.entity_id === item.id,
        )?.location,
        "inventory",
      );
      assert.equal(
        (await game.physics.bodies({ entityId: item.id })).bodies.length,
        0,
        "stored item has no world body",
      );
      if (ammo !== null)
        assert.equal(
          ammoOf(await game.entities.detail(item.id)),
          ammo,
          "stowing preserves loaded ammo",
        );
      await game.input.trigger("ToggleUseMode");
      await game.step({ frames: 5 });
      const ui = await game.ui.state();
      const slot = ui.strip?.elements.find((e) => e.entity_id === item.id);
      assert.ok(slot, "stored item must occupy a visible backpack slot");
      await aimVrHandAtCanvas(
        game,
        ui.panel_pose!,
        [slot.rect[0] + slot.rect[2] / 2, slot.rect[1] + slot.rect[3] / 2],
        { hand, squeeze: 1 },
      );
      await game.step({ frames: 5 });
      assert.equal(
        (await game.info()).player[owner(hand)],
        item.id,
        "retrieval returns the same instance",
      );
    },
  );
}

test(
  "a full backpack keeps the rejected item in hand until regripped",
  { skip: !enabled, timeout: 180_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_interactions",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });
    const mug = (await game.entities.list()).entities.find(
      (e) => e.template_id === -1221,
    )!;
    await aimVrHandAt(game, mug.position, 0.2, 1);
    await game.step({ frames: 5 });
    assert.equal((await game.info()).player.right_hand_entity_id, mug.id);
    // The maximum backpack is15x3; distinct non-stackable mugs occupy one cell each.
    for (let i = 0; i < 45; i++) await game.player.spawnItem(-1221);
    await reach(game, "right", 1);
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 5 });
    assert.equal((await game.info()).player.right_hand_entity_id, mug.id);
    assert.equal(
      (await game.info()).player.hand_feedback?.shoulder_backpack?.retained[1],
      true,
    );
    await game.input.set("right_hand.position", [0.25, 1, -0.4]);
    await game.step({ frames: 5 });
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      mug.id,
      "moving away must not silently drop the refused item",
    );
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 5 });
    assert.equal((await game.info()).player.right_hand_entity_id, null);
    assert.ok(
      (await game.physics.bodies({ entityId: mug.id })).bodies.length > 0,
      "deliberate regrip/release still drops normally",
    );
  },
);

test(
  "releasing in front of the head remains a world drop",
  { skip: !enabled, timeout: 180_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_interactions",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });
    const mug = (await game.entities.list()).entities.find(
      (e) => e.template_id === -1221,
    )!;
    await aimVrHandAt(game, mug.position, 0.2, 1);
    await game.step({ frames: 5 });
    await game.input.set("right_hand.position", [0.2, 1, -0.4]);
    await game.step({ frames: 5 });
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 5 });
    assert.equal((await game.info()).player.right_hand_entity_id, null);
    assert.ok(
      (await game.physics.bodies({ entityId: mug.id })).bodies.length > 0,
    );
    assert.equal(
      (await game.player.inventory()).items.find((i) => i.entity_id === mug.id)
        ?.location,
      undefined,
    );
  },
);
