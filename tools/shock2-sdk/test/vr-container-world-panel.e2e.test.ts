import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";
import { aimVrHandAt, aimVrHandAtCanvas } from "./helpers/vr-hand.js";

// Production frob -> cyber-interface MFD -> physical loot grab. No third
// container surface or proxy collider should be created, even with the old flag.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const CORPSE_PSI_AMP = 219;

for (const experimental of [[], ["gui"]]) {
  test(
    `${experimental.length ? "retired gui flag" : "default VR"} opens loot in the cyber interface and grabs loot through the hand ray`,
    { skip: !e2eEnabled, timeout: 600_000 },
    async () => {
      await using game = await GameServer.launch({
        mission: "medsci1.mis",
        debugFlags: ["--vr"],
        experimental,
      });
      await game.step({ frames: 5 });

      const [corpse] = await game.entities.byTemplate(CORPSE_PSI_AMP);
      assert.ok(corpse, "expected medsci1 corpse 219");
      const corpseDetail = await game.entities.detail(corpse.id);
      const contained = corpseDetail.outgoing_links.filter((link) =>
        link.link_type.startsWith("Contains"),
      );
      assert.equal(contained.length, 1, "corpse 219 should contain exactly the Psi Amp");
      const ampId = contained[0].target_id;
      assert.match(contained[0].target_name, /Psi Amp/i);
      assert.equal(
        (await game.physics.bodies({ entityId: ampId })).bodies.length,
        0,
        "contained loot must begin with HasRefs(false) and no world body",
      );

      await teleportVerified(game, {
        x: corpse.position[0] + 1.2,
        y: corpse.position[1] + 0.5,
        z: corpse.position[2] + 1.2,
      });
      await game.step({ frames: 5 });
      const aim = await game.player.aimAt(corpse, {
        hitbox: "center",
        visibility: "required",
      });
      assert.equal(
        aim.target_confirmed,
        true,
        `corpse must be reachable by the production ray: ${JSON.stringify(aim)}`,
      );
      await aimVrHandAt(game, aim.world_point);

      const uiBodiesBefore = (await game.physics.bodies()).bodies.filter((body) =>
        body.collision_groups.includes("ui"),
      );
      assert.equal(uiBodiesBefore.length, 0, "no VR panel should exist before the frob");

      await game.input.set("right_hand.trigger", 1);
      await game.step({ frames: 2 });
      await game.input.set("right_hand.trigger", 0);
      await game.step({ frames: 5 });

      const uiBodiesAfter = (await game.physics.bodies()).bodies.filter((body) =>
        body.collision_groups.includes("ui"),
      );
      assert.equal(uiBodiesAfter.length, 0, "loot must not create a world-panel collider");
      const ui = await game.ui.state();
      assert.equal(ui.mode, "use");
      assert.equal(ui.active_panel?.entity_id, corpse.id);
      assert.ok(ui.panel_pose, "the cyber interface must own the loot canvas");
      const slot = ui.active_panel.elements.find(e => e.kind === "button" && e.entity_id === ampId);
      assert.ok(slot);
      const [x, y, w, h] = slot.rect;
      await aimVrHandAtCanvas(game, ui.panel_pose, [x + w / 2, y + h / 2]);

      await game.input.set("right_hand.squeeze", 1);
      await game.step({ frames: 10 });

      assert.equal(
        (await game.info()).player.right_hand_entity_id,
        ampId,
        "squeezing the rendered loot icon must grab the contained Psi Amp",
      );
      assert.equal(
        (await game.entities.detail(corpse.id)).outgoing_links.filter((link) =>
          link.link_type.startsWith("Contains"),
        ).length,
        0,
        "grabbing the amp must sever the corpse Contains link",
      );

      const saveName = `vr_container_panel_${Date.now()}`;
      assert.equal((await game.save(saveName)).success, true);
      assert.equal((await game.load(saveName)).success, true);
      await game.step({ frames: 5 });

      const loaded = await game.info();
      assert.ok(loaded.player.right_hand_entity_id, "save/load must preserve the held loot");
      const held = await game.entities.detail(loaded.player.right_hand_entity_id);
      assert.match(held.name ?? "", /Psi Amp/i);
      assert.equal(
        (await game.physics.bodies()).bodies.filter((body) =>
          body.collision_groups.includes("ui"),
        ).length,
        0,
        "the transient panel closes on load instead of leaking a serialized proxy",
      );
    },
  );
}
