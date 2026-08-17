import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { PhysicsBodySummary, UiPanel, Vec3 } from "../src/types.js";
import { teleportVerified } from "./helpers/teleport.js";
import { add, aimVrHandAt, quatRotate } from "./helpers/vr-hand.js";

// Exact MedSci campaign regression for #921. This uses the authentic production
// VR chain rather than a debug Frob shortcut:
//
//   corpse 1680 --Contains(0)--> Amanpour Log 1608
//   hand trigger -> corpse ContainerGui world panel
//   hand trigger on the rendered slot -> LogDiscScript collection
//   Quest Y's semantic action -> player-owned MediaGui world panel
//
// Negative-first on PR #967's head: collection auto-plays LOG0220, the semantic
// action marks it read and plays it again, but the corpse panel stays active and
// no VR transcript ever renders. There is also no Oculus Y binding.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const AMANPOUR_CORPSE = 1680;
const AMANPOUR_LOG = 1608;
const PANEL_SIZE_PX: Vec3 = [188, 296, 0];
const GUI_PIXEL_TO_WORLD_SIZE = 1 / 250;
const LOG_SLOT_CENTER_PX: Vec3 = [15 + 35 / 2, 153 + 34 / 2, 0];

const uiBodies = async (game: GameServer): Promise<PhysicsBodySummary[]> =>
  (await game.physics.bodies()).bodies.filter((body) =>
    body.collision_groups.includes("ui"),
  );

const panelText = (panel: UiPanel): string =>
  panel.elements
    .filter((element) => element.kind === "text" && element.text)
    .map((element) => element.text)
    .join(" ");

const log0220Count = async (game: GameServer): Promise<number> =>
  (await game.audio.recent()).sounds.filter(
    (sound) => sound.sample.toLowerCase() === "log0220",
  ).length;

async function collectAmanpourThroughVrCorpse(game: GameServer): Promise<void> {
  const [corpse] = await game.entities.byTemplate(AMANPOUR_CORPSE);
  assert.ok(corpse, "medsci1 must contain corpse 1680");
  const contains = (await game.entities.detail(corpse.id)).outgoing_links.filter(
    (link) => link.link_type.startsWith("Contains"),
  );
  assert.equal(contains.length, 1, "corpse 1680 should contain exactly one object");
  const logId = contains[0].target_id;
  assert.equal(
    (await game.entities.detail(logId)).template_id,
    AMANPOUR_LOG,
    "corpse 1680 must contain the Amanpour mission object 1608",
  );

  await teleportVerified(game, {
    x: corpse.position[0] + 1.2,
    y: corpse.position[1] + 0.5,
    z: corpse.position[2] + 1.2,
  });
  const corpseAim = await game.player.aimAt(corpse, {
    hitbox: "center",
    visibility: "required",
  });
  assert.equal(corpseAim.target_confirmed, true, "corpse 1680 must be hand-reachable");
  await aimVrHandAt(game, corpseAim.world_point);
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.trigger", 0);
  await game.step({ frames: 5 });

  const [corpsePanel] = await uiBodies(game);
  assert.ok(corpsePanel, "production corpse frob must open its VR loot panel");
  assert.equal((await uiBodies(game)).length, 1, "only the corpse panel should be open");

  // Invert ProxyGuiScript's world -> normalized-canvas mapping for Contains(0),
  // the first authored 35x34 slot in the 188x296 corpse canvas.
  const panelSize: Vec3 = [
    PANEL_SIZE_PX[0] * GUI_PIXEL_TO_WORLD_SIZE,
    PANEL_SIZE_PX[1] * GUI_PIXEL_TO_WORLD_SIZE,
    0,
  ];
  const u = LOG_SLOT_CENTER_PX[0] / PANEL_SIZE_PX[0];
  const v = LOG_SLOT_CENTER_PX[1] / PANEL_SIZE_PX[1];
  const localSlot: Vec3 = [panelSize[0] * (0.5 - u), panelSize[1] * (0.5 - v), 0];
  const slotWorld = add(
    corpsePanel.position,
    quatRotate(corpsePanel.rotation, localSlot),
  );
  const panelAim = await aimVrHandAt(game, slotWorld, 0.35);
  const hit = await game.raycast({
    start: panelAim.start,
    end: panelAim.target,
    collision_groups: ["ui"],
    max_distance: 1,
  });
  assert.equal(hit.entity_id, corpsePanel.entity_id, "hand ray must hit Log1608's slot");

  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.trigger", 0);
  await game.step({ frames: 5 });
  assert.deepEqual((await game.info()).player.collected_logs, [
    { deck: 2, log: 20, read: false },
  ]);
}

async function assertAmanpourReader(game: GameServer): Promise<UiPanel> {
  const panel = (await game.ui.state()).active_panel;
  assert.ok(panel, "ReadLastUnreadLog must expose the active VR MediaGui panel");
  assert.equal(panel.template_id, -1, "the reader must use its player-owned host");
  const text = panelText(panel);
  assert.ok(text.includes("45100"), `reader transcript must visibly contain 45100: ${text}`);
  assert.ok(text.toUpperCase().includes("AMANPOUR"), `reader must identify Amanpour: ${text}`);
  const textures = panel.elements
    .filter((element) => element.kind === "image")
    .map((element) => element.texture?.toLowerCase());
  assert.ok(textures.includes("iface/log.pcx"));
  assert.ok(textures.includes("amanpour.pcx"));
  assert.ok(textures.includes("medicon.pcx"));
  assert.equal((await uiBodies(game)).length, 1, "reader must replace, not overlap, corpse UI");
  return panel;
}

test(
  "Quest VR collects Amanpour through corpse 1680 and reopens its 45100 reader across save/load and decks",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8571),
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 5 });

    const beforeCollectionAudio = await log0220Count(game);
    await collectAmanpourThroughVrCorpse(game);
    assert.equal(
      await log0220Count(game),
      beforeCollectionAudio,
      "VR pickup must only file the log; the production reader action owns playback",
    );

    await game.input.trigger("ReadLastUnreadLog");
    await game.step({ frames: 5 });
    await assertAmanpourReader(game);
    assert.deepEqual((await game.info()).player.collected_logs, [
      { deck: 2, log: 20, read: true },
    ]);
    assert.equal(await log0220Count(game), beforeCollectionAudio + 1);
    await game.screenshot("vr-amanpour-reader-45100.png");

    // Quest Y toggles the reader closed, then reopens/replays the latest entry
    // even though it is already read.
    await game.input.trigger("ReadLastUnreadLog");
    await game.step({ frames: 5 });
    assert.equal((await game.ui.state()).active_panel, null);
    assert.equal((await uiBodies(game)).length, 0);
    await game.input.trigger("ReadLastUnreadLog");
    await game.step({ frames: 5 });
    await assertAmanpourReader(game);
    assert.equal(await log0220Count(game), beforeCollectionAudio + 2);

    const saveName = `issue921_vr_log_${Date.now()}`;
    assert.equal((await game.save(saveName)).success, true);
    assert.equal((await game.load(saveName)).success, true);
    await game.step({ frames: 5 });
    assert.equal((await game.ui.state()).active_panel, null, "load removes transient panel");
    await game.input.trigger("ReadLastUnreadLog");
    await game.step({ frames: 5 });
    await assertAmanpourReader(game);

    await game.transitionLevel("medsci2.mis");
    await game.step({ frames: 5 });
    assert.equal((await game.info()).mission, "medsci2.mis");
    await game.input.trigger("ReadLastUnreadLog");
    await game.step({ frames: 5 });
    await assertAmanpourReader(game);

    // PR #967's X/backpack affordance stays independent of Y. It replaces the
    // reader through the same one-panel lifecycle and toggles away cleanly.
    await game.input.trigger("MoveInventory");
    await game.step({ frames: 5 });
    const backpack = (await game.ui.state()).active_panel;
    assert.ok(backpack, "Quest X must still open the real backpack panel");
    assert.ok(
      backpack.elements.some(
        (element) => element.kind === "image" && element.texture?.toLowerCase() === "invback.pcx",
      ),
      "Quest X must show the authored INVBACK canvas",
    );
    assert.equal((await uiBodies(game)).length, 1, "backpack replaces reader without overlap");
    await game.input.trigger("MoveInventory");
    await game.step({ frames: 5 });
    assert.equal((await uiBodies(game)).length, 0);
  },
);
