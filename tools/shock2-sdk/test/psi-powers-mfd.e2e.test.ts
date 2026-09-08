import assert from "node:assert/strict";
import { test } from "node:test";

import { e2ePort } from "./helpers/e2e-port.js";
import { GameServer, type UiElement, type Vec3 } from "../src/index.js";
import { clickUiElement } from "./helpers/ui.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

// End-to-end coverage for the psi power selection MFD.
//
// The original's psi screen: a five-tab tier strip over a 2x4 grid of
// discipline icons, each icon its own art in three variants (untrained /
// trained / selected). Clicking a trained power selects it; the tier tabs only
// change what is browsed. It opens from the AMMOFULL readout's power badge on
// flat, and from the psi-amp hand's LOWER face button in VR - which brings up
// the cyber interface around it. While it is docked, one thumbstick is
// captured - flat's LEFT (arrow-key turn), VR's off hand - so flicks step the
// selection live and that stick stops driving the player.
//
// Negative-first: on the parent branch there is no `psi_select` readout button
// at all, `SelectPsiPower` is not an action, the amp hand's buttons resolve to
// nothing, and no panel named "Psi Powers" exists - so every assertion below
// fails.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** The synthetic host entity's name, as `/v1/ui` reports it. */
const PANEL_NAME = "Psi Powers";
/** The Psi Amp player weapon. */
const PSI_AMP = -247;

/** `debug_psi` trains every power, so tier 1's grid is ids 1..7. */
const TIER_1_POWERS = [1, 2, 3, 4, 5, 6, 7];

async function panelElements(game: GameServer): Promise<UiElement[]> {
  const panel = (await game.ui.state()).active_panel;
  assert.ok(panel, "the psi powers panel should be open");
  assert.equal(panel.name, PANEL_NAME);
  return panel.elements;
}

async function element(game: GameServer, label: string): Promise<UiElement> {
  const found = (await panelElements(game)).find((e) => e.label === label);
  assert.ok(found, `expected a "${label}" element on the psi panel`);
  return found;
}

/** The tier strip art currently drawn (`iface/psi<tier>.pcx`). */
async function browsedTier(game: GameServer): Promise<number> {
  const strip = (await panelElements(game)).find((e) =>
    /iface\/psi[1-5]\.pcx/.test(e.texture ?? ""),
  );
  assert.ok(strip, "the panel draws one tier strip");
  return Number(/psi([1-5])\.pcx/.exec(strip.texture ?? "")?.[1]);
}

/** Every drawn icon, as `power id -> variant` (0 untrained, 1 trained, 2 selected). */
async function iconVariants(game: GameServer): Promise<string[]> {
  return (await panelElements(game))
    .map((e) => e.texture ?? "")
    .filter((texture) => /picn\d+_[012]\.pcx/.test(texture));
}

async function selectedPower(game: GameServer): Promise<string | null> {
  return (await game.info()).player.selected_psi_power ?? null;
}

/** Open the panel the way a flat player does: use mode, then the psi readout. */
async function openFromReadout(game: GameServer): Promise<void> {
  const readout = (await game.ui.state()).readout ?? [];
  const select = readout.find((e) => e.label === "psi_select");
  assert.ok(
    select,
    `the psi readout should be clickable: ${readout.map((e) => e.label)}`,
  );
  await clickUiElement(game, select);
  await game.step({ frames: 3 });
}

/** One edge-triggered flick of a thumbstick, then back to rest. */
async function flick(
  game: GameServer,
  hand: "left" | "right",
  value: [number, number],
): Promise<void> {
  await game.input.set(`${hand}_hand.thumbstick`, value);
  await game.step({ frames: 5 });
  await game.input.set(`${hand}_hand.thumbstick`, [0, 0]);
  await game.step({ frames: 5 });
}

test(
  "the psi readout opens a tier-snapped power grid, and a click selects a power",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_psi",
      port: e2ePort(0, "SHOCK2_E2E_PSI_PORT"),
    });
    await game.step({ frames: 30 });
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });

    assert.equal(
      (await game.ui.state()).active_panel,
      null,
      "nothing is docked before the readout is clicked",
    );
    await openFromReadout(game);

    // The default OSA loadout is Projected Cryokinesis - power id 6, tier 1 -
    // so the panel opens on tier 1 with that icon in its selected variant.
    assert.equal(await browsedTier(game), 1, "the panel opens on the selection's tier");
    const variants = await iconVariants(game);
    assert.equal(
      variants.filter((texture) => texture.endsWith("_2.pcx")).length,
      1,
      `exactly one icon is drawn selected: ${variants}`,
    );
    assert.ok(
      variants.some((texture) => texture === "iface/picn07_2.pcx"),
      `Cryokinesis (icon picn07) is the selected one: ${variants}`,
    );

    // Seven disciplines are clickable; the tier's capacity marker is drawn but
    // carries no hit target at all.
    const labels = (await panelElements(game))
      .map((e) => e.label)
      .filter((label): label is string => typeof label === "string");
    for (const id of TIER_1_POWERS) {
      assert.ok(labels.includes(`psi_power_${id}`), `power ${id} is clickable`);
    }
    assert.ok(
      !labels.includes("psi_power_0"),
      `the tier marker is not selectable: ${labels}`,
    );

    // The tier marker's cell is drawn but carries no hit target, so clicking
    // it changes nothing. It is the cell one row above power 1, in the same
    // column - one 30px grid row on the 480px canvas.
    const powerOne = await element(game, "psi_power_1");
    const markerRect: [number, number, number, number] = [
      powerOne.screen_rect[0],
      powerOne.screen_rect[1] - 30 / 480,
      powerOne.screen_rect[2],
      powerOne.screen_rect[3],
    ];
    const initial = await selectedPower(game);
    await clickUiElement(game, { ...powerOne, label: null, screen_rect: markerRect });
    await game.step({ frames: 3 });
    assert.equal(
      await selectedPower(game),
      initial,
      "the tier marker cell is inert",
    );

    // Clicking another trained power selects it.
    const before = await selectedPower(game);
    await clickUiElement(game, await element(game, "psi_power_1"));
    await game.step({ frames: 3 });
    const after = await selectedPower(game);
    assert.notEqual(after, before, "clicking a trained power selects it");
    assert.ok(
      (await iconVariants(game)).some((t) => t === "iface/picn01_2.pcx"),
      "and its own icon becomes the selected variant",
    );

    // A tab click browses only - the amp keeps the power it had.
    await clickUiElement(game, await element(game, "psi_tier_3"));
    await game.step({ frames: 3 });
    assert.equal(await browsedTier(game), 3, "the tab pages the grid to tier 3");
    assert.equal(
      await selectedPower(game),
      after,
      "browsing a tier does not change the selection",
    );
    const tier3 = (await panelElements(game))
      .map((e) => e.label)
      .filter((label): label is string => typeof label === "string");
    assert.ok(tier3.includes("psi_power_17"), `tier 3's block is ids 17..23: ${tier3}`);
    assert.ok(!tier3.includes("psi_power_1"), "tier 1's powers are gone");

  },
);

test(
  "the docked panel captures a thumbstick: flicks step the selection and do not walk",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_psi",
      port: e2ePort(1, "SHOCK2_E2E_PSI_PORT"),
    });
    await game.step({ frames: 30 });
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    await openFromReadout(game);

    // Flat captures the LEFT stick - the arrow-key turn axis. The right stick
    // is WASD, and taking that would stop the player walking while a panel they
    // drive with the mouse is open.
    const rotation = (await game.info()).player.rotation;
    const first = await selectedPower(game);

    await flick(game, "left", [1, 0]);
    const second = await selectedPower(game);
    assert.notEqual(second, first, "a right flick steps to the next power");
    assert.deepEqual(
      (await game.info()).player.rotation,
      rotation,
      "and the captured stick does not turn the player",
    );

    // Edge-triggered: a held stick steps once, not once per frame.
    await game.input.set("left_hand.thumbstick", [1, 0]);
    await game.step({ frames: 60 });
    const third = await selectedPower(game);
    await game.input.set("left_hand.thumbstick", [0, 0]);
    await game.step({ frames: 5 });
    assert.notEqual(third, second, "the held push registers its own rising edge");
    // With seven trained powers in the tier, sixty frames of per-frame stepping
    // would have wrapped several times and could land anywhere; one step lands
    // on the immediate neighbour, which is the power a second flick reaches.
    await flick(game, "left", [1, 0]);
    const fourth = await selectedPower(game);
    assert.notEqual(fourth, third);

    // Up steps the tier, and the browsed tier follows the selection.
    await flick(game, "left", [0, 1]);
    assert.equal(await browsedTier(game), 2, "an up flick steps to the next tier");

    // The other stick is untouched: WASD still walks with the panel open.
    const start = (await game.info()).player.position;
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.step({ frames: 20 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.step({ frames: 2 });
    assert.notDeepEqual(
      (await game.info()).player.position,
      start,
      "only the captured stick is withheld",
    );
  },
);

test(
  "in VR the amp hand's upper button opens the cyber interface onto the panel",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_psi",
      port: e2ePort(2, "SHOCK2_E2E_PSI_PORT"),
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    // In VR the scene leaves the amp on the floor: take it in the right hand.
    const amp = (await game.entities.list()).entities.find(
      (e) => e.template_id === PSI_AMP,
    );
    assert.ok(amp, "debug_psi stocks a psi amp");
    await aimVrHandAt(game, amp.position as Vec3, 0.35);
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 8 });
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      amp.id,
      "the VR right hand must hold the amp",
    );

    assert.equal((await game.ui.state()).mode, "shooter");
    await game.input.trigger("RightHandUpperButton");
    await game.step({ frames: 8 });

    const ui = await game.ui.state();
    assert.equal(ui.mode, "use", "the press brings up the cyber interface");
    assert.equal(
      ui.active_panel?.name,
      PANEL_NAME,
      "...docked onto the psi power panel",
    );

    // The off hand's stick navigates live - the amp hand keeps aiming.
    const before = await selectedPower(game);
    await flick(game, "left", [1, 0]);
    assert.notEqual(await selectedPower(game), before, "the off-hand stick steps");
    await flick(game, "left", [0, 1]);
    assert.equal(await browsedTier(game), 2, "up steps the tier");

    // The interface takes both buttons back while it is up, so the LOWER
    // button closes the whole thing rather than jumping - the upper one is the
    // log reader in there, not a second way out.
    await game.input.trigger("RightHandLowerButton");
    await game.step({ frames: 8 });
    const closed = await game.ui.state();
    assert.equal(closed.mode, "shooter", "the lower button closes the interface");
    assert.equal(closed.active_panel, null);
  },
);
