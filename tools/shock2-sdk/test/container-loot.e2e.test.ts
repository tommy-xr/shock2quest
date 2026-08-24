import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { UiElement } from "../src/types.js";
import { teleportVerified } from "./helpers/teleport.js";

// End-to-end test for container loot (projects/flat-ui.md PR 3, resolves #433;
// supersedes PR #439, salvaging its interaction semantics and scenario shape):
//
//  1. Containment at creation: an entity with an incoming `Contains` link has
//     NO world presence (no physics body) until taken - it lives inside its
//     container's loot panel, not double-placed at its authored editor spot.
//  2. Loot MFD: frobbing a `ContainerScript` container (a corpse) opens its
//     ContainerGui panel in the flat MFD slot; a living `CreatureContainer`
//     AI does NOT open a panel (looting the living is not a thing - the rule
//     salvaged from #439).
//  3. Take: clicking a contained item in the panel transfers its `Contains`
//     link to the player backpack; the panel reflects the removal live.
//  4. /v1/ui semantics: loot elements carry the item's name as `label` and
//     its runtime `entity_id`, so tests click "Psi Amp" by meaning.
//  5. Save/load round-trip preserves containment state (contained stays
//     contained, taken stays taken, world-placed stays world-placed).
//
// Entity discovery is by TEMPLATE ID at run time (runtime entity ids are NOT
// stable across launches; the `template_id` reported by /v1/entities IS
// stable - the mission-file object id for level-authored entities):
//   - corpse 219 ("Male Corpse 1") -> Contains -> Psi Amp (mission id 1407)
//   - corpse 1177 ("MS Male Corpse") -> Contains -> Wrench (mission id 990),
//     the authored first MedSci interaction (#433 / #439's scenario)
//   - Cryo Card 1050: NO incoming Contains - the world-placed control item
//   - OG-Pipe 596: a living CreatureContainer AI (Contains -> OG Organ)
//
// Negative-first: on main (pre containment-at-creation) the Psi Amp is
// instantiated as a physical world prop at its authored position ~2.7 units
// from the corpse - the "no physics bodies" assertion fails; and the loot
// panel's elements carry no item name labels - the "Psi Amp element" lookup
// fails.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const CORPSE_PSI_AMP = 219; // Male Corpse 1 -> Contains -> Psi Amp
const CORPSE_WRENCH = 1177; // MS Male Corpse -> Contains -> Wrench (the #433 route)
const CRYO_CARD = 1050; // world-placed control item (no incoming Contains)
const LIVING_AI = 596; // OG-Pipe with Contains -> OG Organ (must NOT open)

test(
  "container loot: contained items have no world presence and are taken via the loot MFD",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
    });
    await game.step({ frames: 5 });

    const byTemplateOne = async (templateId: number, what: string) => {
      const matches = await game.entities.byTemplate(templateId);
      assert.equal(matches.length, 1, `expected exactly one ${what} (mission id ${templateId})`);
      return matches[0];
    };
    const bodyCount = async (entityId: number) =>
      (await game.physics.bodies({ entityId })).bodies.length;
    const containsLinks = async (entityId: number) => {
      const detail = await game.entities.detail(entityId);
      return detail.outgoing_links.filter((l) => l.link_type.startsWith("Contains"));
    };
    const clickElement = async (el: UiElement) => {
      const [x, y, w, h] = el.screen_rect;
      const center: [number, number] = [x + w / 2, y + h / 2];
      await game.input.set("pointer.position", center);
      await game.step({ frames: 2 }); // hover registers (edge detection needs a prior unpressed frame)
      await game.input.set("pointer.pressed", 1);
      await game.step({ frames: 2 });
      await game.input.set("pointer.pressed", 0);
      await game.step({ frames: 2 });
    };

    // --- Discover the corpse and its contained Psi Amp ---
    const corpse = await byTemplateOne(CORPSE_PSI_AMP, "Male Corpse 1");
    const ampLinks = await containsLinks(corpse.id);
    assert.equal(ampLinks.length, 1, "corpse 219 should contain exactly the Psi Amp");
    const ampId = ampLinks[0].target_id;
    assert.ok(
      ampLinks[0].target_name.includes("Psi Amp"),
      `corpse 219's contained item should be the Psi Amp, got ${ampLinks[0].target_name}`,
    );

    // --- (i) NEGATIVE KEY: contained items are NOT world-placed ---
    // On main both carry physics bodies at their authored editor positions
    // (the double-placed-loot bug, design doc §4.6).
    assert.equal(
      await bodyCount(ampId),
      0,
      "the corpse-contained Psi Amp must have no world presence (no physics body)",
    );
    const corpse1177 = await byTemplateOne(CORPSE_WRENCH, "MS Male Corpse");
    const wrenchLinks = await containsLinks(corpse1177.id);
    assert.equal(wrenchLinks.length, 1, "corpse 1177 should contain exactly the Wrench");
    assert.equal(
      await bodyCount(wrenchLinks[0].target_id),
      0,
      "the corpse-contained Wrench must have no world presence (no physics body)",
    );

    // --- (iv) control: a genuinely world-placed item keeps its presence ---
    const cryoCard = await byTemplateOne(CRYO_CARD, "Cryo Card");
    assert.ok(
      (await bodyCount(cryoCard.id)) > 0,
      "the world-placed Cryo Card (no incoming Contains) must keep its physics body",
    );

    // --- living CreatureContainer AI must NOT open a loot panel ---
    const pipe = await byTemplateOne(LIVING_AI, "OG-Pipe 596");
    await teleportVerified(game, {
      x: pipe.position[0] + 1.5,
      y: pipe.position[1] + 0.5,
      z: pipe.position[2] + 1.5,
    });
    await game.entities.sendMessage(pipe.id, { type: "Frob" });
    await game.step({ frames: 5 });
    assert.equal(
      (await game.ui.state()).active_panel,
      null,
      "frobbing a living CreatureContainer AI must not open a loot panel",
    );

    // --- (ii) frob the corpse -> loot MFD opens with a labeled Psi Amp ---
    await teleportVerified(game, {
      x: corpse.position[0] + 1.0,
      y: corpse.position[1] + 0.5,
      z: corpse.position[2] + 1.0,
    });
    await game.step({ frames: 5 });
    await game.screenshot("loot-before-frob.png");
    await game.entities.sendMessage(corpse.id, { type: "Frob" });
    await game.step({ frames: 5 });

    const opened = await game.ui.state();
    assert.ok(
      opened.active_panel,
      `frobbing the corpse should open its loot MFD panel (got ${JSON.stringify(opened)})`,
    );
    assert.equal(
      opened.active_panel.entity_id,
      corpse.id,
      "the active panel should be bound to the corpse entity",
    );
    const ampElement = opened.active_panel.elements.find(
      (e) => e.kind === "button" && e.label === "Psi Amp",
    );
    assert.ok(
      ampElement,
      `loot panel should expose a button labeled "Psi Amp" ` +
        `(got ${JSON.stringify(opened.active_panel.elements)})`,
    );
    assert.equal(
      ampElement.entity_id,
      ampId,
      "the Psi Amp element should carry the contained item's entity id",
    );
    await game.screenshot("loot-panel-open.png");

    // --- (iii) click the Psi Amp -> it transfers to the player backpack ---
    await clickElement(ampElement);

    const inventory = await game.player.inventory();
    assert.ok(
      inventory.items.some((i) => i.entity_id === ampId),
      `taking the Psi Amp should put it in the player inventory ` +
        `(got ${JSON.stringify(inventory.items)})`,
    );
    // The corpse's Contains link moved to the backpack...
    assert.equal(
      (await containsLinks(corpse.id)).length,
      0,
      "taking the Psi Amp should remove the corpse's Contains link",
    );
    // ...the panel reflects the removal live...
    const afterTake = await game.ui.state();
    assert.ok(afterTake.active_panel, "the loot panel stays open after taking an item");
    assert.ok(
      !afterTake.active_panel.elements.some((e) => e.entity_id === ampId),
      "the taken Psi Amp should disappear from the loot panel",
    );
    // ...and the item STILL has no world presence (it is carried, not spilled).
    assert.equal(
      await bodyCount(ampId),
      0,
      "the taken Psi Amp lives in the backpack, not in the world",
    );
    await game.screenshot("loot-taken.png");

    // --- (v) save/load round-trip preserves all three states ---
    const saveName = `container_loot_e2e_${Date.now()}`;
    await game.save(saveName);
    await game.load(saveName);
    await game.step({ frames: 5 });

    // Taken stays taken: the corpse has no Contains link and the amp is still
    // in the backpack (runtime ids changed across the load - rediscover).
    const corpseReloaded = await byTemplateOne(CORPSE_PSI_AMP, "Male Corpse 1 (after load)");
    assert.equal(
      (await containsLinks(corpseReloaded.id)).length,
      0,
      "after load the corpse must not regain its Contains link",
    );
    const inventoryReloaded = await game.player.inventory();
    const ampCarried = inventoryReloaded.items.find((i) => i.name === "Psi Amp");
    assert.ok(
      ampCarried,
      `after load the taken Psi Amp stays in the inventory ` +
        `(got ${JSON.stringify(inventoryReloaded.items)})`,
    );
    assert.equal(
      await bodyCount(ampCarried.entity_id),
      0,
      "after load the carried Psi Amp still has no world presence",
    );

    // Contained stays contained: corpse 1177 still holds its Wrench, with no
    // world presence...
    const corpse1177Reloaded = await byTemplateOne(CORPSE_WRENCH, "MS Male Corpse (after load)");
    const wrenchReloaded = await containsLinks(corpse1177Reloaded.id);
    assert.equal(wrenchReloaded.length, 1, "after load corpse 1177 still contains the Wrench");
    assert.ok(
      wrenchReloaded[0].target_name.includes("Wrench"),
      `corpse 1177's contained item should still be the Wrench, got ${wrenchReloaded[0].target_name}`,
    );
    assert.equal(
      await bodyCount(wrenchReloaded[0].target_id),
      0,
      "after load the contained Wrench still has no world presence",
    );
    // ...and its loot panel still works from the loaded state.
    await teleportVerified(game, {
      x: corpse1177Reloaded.position[0] + 1.0,
      y: corpse1177Reloaded.position[1] + 0.5,
      z: corpse1177Reloaded.position[2] + 1.0,
    });
    await game.entities.sendMessage(corpse1177Reloaded.id, { type: "Frob" });
    await game.step({ frames: 5 });
    const reloadedPanel = await game.ui.state();
    assert.ok(
      reloadedPanel.active_panel &&
        reloadedPanel.active_panel.elements.some(
          (e) => e.kind === "button" && e.label === "Wrench",
        ),
      `after load, frobbing corpse 1177 should open a loot panel listing the Wrench ` +
        `(got ${JSON.stringify(reloadedPanel.active_panel)})`,
    );

    // World-placed stays world-placed.
    const cryoCardReloaded = await byTemplateOne(CRYO_CARD, "Cryo Card (after load)");
    assert.ok(
      (await bodyCount(cryoCardReloaded.id)) > 0,
      "after load the world-placed Cryo Card keeps its physics body",
    );
  },
);
