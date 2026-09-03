import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/types.js";
import { teleportVerified } from "./helpers/teleport.js";
import {
  LOOT_PANEL_SIZE_PX as PANEL_SIZE_PX,
  LOOT_SLOT_CENTER_PX,
  add,
  aimVrHandAt,
  quatRotate,
} from "./helpers/vr-hand.js";

// Default-VR regression for #940. The mission data and gameplay path are the
// real ones:
//
//   medsci1 corpse 219 --Contains--> Psi Amp 1407
//   production hand trigger -> ContainerScript/GuiScript::OpenPanel
//   production hand ray + squeeze -> ProxyGuiScript::GUIHover -> GrabEntity
//
// No direct Frob, Give, or experimental `gui` flag is used. Before the fix the
// trigger reaches the corpse but no UI collider is created, so the first
// post-frob UI-body assertion is the negative key.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const CORPSE_PSI_AMP = 219;
const GUI_PIXEL_TO_WORLD_SIZE = 1 / 250;


test(
  "default VR opens a container world panel and grabs contained loot through the hand ray",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      debugFlags: ["--vr"],
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
    assert.equal(
      uiBodiesAfter.length,
      1,
      "a production VR frob must create one interactable world-panel collider",
    );
    const panel = uiBodiesAfter[0];
    assert.equal(panel.body_type, "kinematic");

    // ProxyGuiScript maps a panel hit back with:
    //   u = 1 - (local.x + width/2) / width
    //   v = 1 - (local.y + height/2) / height
    // Invert that mapping for the center of the first 35x34 loot slot. The
    // corpse has exactly one item, so the real ContainerGui places the amp
    // there through its normal Inventory packing path.
    const panelSize: Vec3 = [
      PANEL_SIZE_PX[0] * GUI_PIXEL_TO_WORLD_SIZE,
      PANEL_SIZE_PX[1] * GUI_PIXEL_TO_WORLD_SIZE,
      0,
    ];
    const u = LOOT_SLOT_CENTER_PX[0] / PANEL_SIZE_PX[0];
    const v = LOOT_SLOT_CENTER_PX[1] / PANEL_SIZE_PX[1];
    const localSlot: Vec3 = [panelSize[0] * (0.5 - u), panelSize[1] * (0.5 - v), 0];
    const slotWorld = add(panel.position, quatRotate(panel.rotation, localSlot));

    const panelAim = await aimVrHandAt(game, slotWorld, 0.35);
    const panelHit = await game.raycast({
      start: panelAim.start,
      end: panelAim.target,
      collision_groups: ["ui"],
      max_distance: 1,
    });
    assert.equal(panelHit.entity_id, panel.entity_id, "the production hand ray must hit the panel");

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
