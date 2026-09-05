import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/index.js";
import type { UiElement, UiPanel } from "../src/types.js";
import { aimVrHandAt, aimVrHandAtCanvas } from "./helpers/vr-hand.js";
import { clickUiElement } from "./helpers/ui.js";
import { ammoOf, cycleToWeapon, fireOnce } from "./helpers/weapon.js";

// Firing wears a gun down: each shot costs the condition points the gun's
// reliability authors (the pistol loses 1 of its 100 per shot), in flatscreen
// and in VR alike - both presentations pull the same trigger into the same
// shared fire path.
//
// Negative-first: on the parent the runtime does not parse gun reliability at
// all, nothing decrements condition, and `/v1/info` has no
// `wielded_gun_condition` field - so every assertion below fails.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** The pistol: condition 100, 1 point lost per shot. */
const PISTOL = -17;
const PISTOL_DEGRADE_PER_SHOT = 1.0;

async function condition(game: GameServer): Promise<number> {
  const value = (await game.info()).player.wielded_gun_condition;
  assert.equal(
    typeof value,
    "number",
    "a wielded gun must report its condition",
  );
  return value as number;
}

test(
  "each flatscreen shot wears the gun down by its authored degrade rate",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons" });
    await game.step({ frames: 30 });

    // The pistol is the first weapon DebugCycleWeapon hands out, and flat mode
    // wields what it spawns.
    await game.input.trigger("DebugCycleWeapon");
    await game.step({ frames: 10 });

    assert.equal(await condition(game), 100, "a shipped gun starts pristine");

    const shots = 3;
    for (let i = 0; i < shots; i += 1) await fireOnce(game);

    assert.equal(
      await condition(game),
      100 - shots * PISTOL_DEGRADE_PER_SHOT,
      "every shot costs the gun its authored condition points",
    );
  },
);

test(
  "a VR trigger pull wears the gun down the same way",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    // In VR nothing is wielded until a hand grabs it.
    const weapon = await cycleToWeapon(game, (e) => e.template_id === PISTOL);
    await aimVrHandAt(game, weapon.position as Vec3, 0.3);
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 8 });
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      weapon.id,
      "the VR right hand must hold the pistol",
    );

    assert.equal(await condition(game), 100, "a shipped gun starts pristine");

    const shots = 2;
    for (let i = 0; i < shots; i += 1) await fireOnce(game);

    assert.equal(
      await condition(game),
      100 - shots * PISTOL_DEGRADE_PER_SHOT,
      "the VR trigger reaches the same shared fire path",
    );
  },
);

/** The `ObjectState` property of an entity detail - its working order. */
function objectStateOf(detail: {
  properties: { name: string; value: string }[];
}): string {
  const state = detail.properties.find((p) => p.name === "ObjectState");
  assert.ok(state, "a gun should expose an ObjectState property");
  return state.value;
}

/** Break `gun` outright and confirm the runtime says so. */
async function breakGun(game: GameServer, gun: number): Promise<void> {
  await game.entities.sendMessage(gun, {
    type: "SetObjectState",
    state: "Broken",
  });
  await game.step({ frames: 1 });
  assert.equal(
    objectStateOf(await game.entities.detail(gun)),
    "Broken",
    "the debug hook must break the gun",
  );
}

test(
  "a broken gun refuses to fire in flatscreen",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons" });
    await game.step({ frames: 30 });
    await game.input.trigger("DebugCycleWeapon");
    await game.step({ frames: 10 });

    const gun = (await game.info()).player.wielded_entity_id;
    assert.ok(gun, "flat mode must wield the pistol it spawned");

    // Wear it down to where breakage becomes possible at all...
    await game.entities.sendMessage(gun, {
      type: "SetGunCondition",
      condition: 5,
    });
    await game.step({ frames: 1 });
    assert.equal((await game.info()).player.wielded_gun_condition, 5);
    assert.equal(objectStateOf(await game.entities.detail(gun)), "Normal");

    // ...then back above the pistol's break threshold (10), so the control
    // shot below cannot roll a break and make the test flaky.
    await game.entities.sendMessage(gun, {
      type: "SetGunCondition",
      condition: 50,
    });
    await game.step({ frames: 1 });

    // A working gun spends a round, so what follows is the breakage and not a
    // fixture that never fired.
    const loaded = ammoOf(await game.entities.detail(gun));
    await fireOnce(game);
    assert.equal(
      ammoOf(await game.entities.detail(gun)),
      loaded - 1,
      "a working gun spends a round",
    );

    await breakGun(game, gun);
    const ammo = ammoOf(await game.entities.detail(gun));
    const condition = (await game.info()).player.wielded_gun_condition;

    for (let i = 0; i < 3; i += 1) await fireOnce(game);

    const detail = await game.entities.detail(gun);
    assert.equal(ammoOf(detail), ammo, "a broken gun spends no ammo");
    assert.equal(
      (await game.info()).player.wielded_gun_condition,
      condition,
      "a broken gun does not wear any further",
    );
    assert.equal(objectStateOf(detail), "Broken", "and it stays broken");
  },
);

test(
  "a broken gun refuses a VR trigger pull too",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    const weapon = await cycleToWeapon(game, (e) => e.template_id === PISTOL);
    await aimVrHandAt(game, weapon.position as Vec3, 0.3);
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 8 });
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      weapon.id,
      "the VR right hand must hold the pistol",
    );

    const loaded = ammoOf(await game.entities.detail(weapon.id));
    await fireOnce(game);
    assert.equal(
      ammoOf(await game.entities.detail(weapon.id)),
      loaded - 1,
      "a working gun spends a round",
    );

    await breakGun(game, weapon.id);
    const ammo = ammoOf(await game.entities.detail(weapon.id));
    const condition = (await game.info()).player.wielded_gun_condition;

    for (let i = 0; i < 3; i += 1) await fireOnce(game);

    assert.equal(
      ammoOf(await game.entities.detail(weapon.id)),
      ammo,
      "a broken gun spends no ammo in VR either",
    );
    assert.equal(
      (await game.info()).player.wielded_gun_condition,
      condition,
      "and does not wear any further",
    );
  },
);

// --- Slice 4: the maintenance tool ---
//
// Using a maintenance tool on a gun restores 10 condition points per level of
// the player's Maintain skill (debug_weapons maxes the sheet at 6, so 60),
// capped at full condition, and uses the tool up. VR applies it as a physical
// gesture - the tool is released against the gun held in the other hand; flat
// has no drag, so using the tool from the inventory strip applies it to the
// wielded gun (#817, partially).
//
// Negative-first: on the parent nothing recognizes the tool at all - the flat
// double-click just Frobs it (a no-op), the VR release drops it, and the gun's
// condition never moves.

const MAINTENANCE_TOOL = -2949;
/** debug_weapons maxes every skill, so Maintain is 6: ten points a level. */
const RESTORED_POINTS = 60;

async function toolInWorld(game: GameServer) {
  const tools = await game.entities.byTemplate(MAINTENANCE_TOOL);
  assert.equal(
    tools.length,
    1,
    "debug_weapons should bench one maintenance tool",
  );
  return tools[0];
}

test(
  "releasing the maintenance tool onto the gun in the other hand restores its condition",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run", timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    // Right hand: the pistol, worn down to 20.
    const weapon = await cycleToWeapon(game, (e) => e.template_id === PISTOL);
    const gunHand = await aimVrHandAt(game, weapon.position as Vec3, 0.3);
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 8 });
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      weapon.id,
      "the VR right hand must hold the pistol",
    );
    await game.entities.sendMessage(weapon.id, {
      type: "SetGunCondition",
      condition: 20,
    });
    await game.step({ frames: 1 });
    assert.equal(await condition(game), 20, "the gun starts worn");

    // Left hand: the tool off the bench.
    const tool = await toolInWorld(game);
    await aimVrHandAt(game, tool.position as Vec3, 0.3, 0, 0, { hand: "left" });
    await game.input.set("left_hand.squeeze", 1);
    await game.step({ frames: 8 });
    const carried = await game.player.inventory();
    assert.ok(
      carried.items.some(
        (i) => i.entity_id === tool.id && i.location === "left_hand",
      ),
      `the VR left hand must hold the maintenance tool (got ${JSON.stringify(
        carried.items,
      )})`,
    );

    // KEY: bring the tool to the gun and let go. The release ray is not
    // pointed at the gun - the hands are simply side by side, which is the
    // gesture a player makes.
    await game.input.set("left_hand.position", [
      gunHand.local[0] + 0.1,
      gunHand.local[1],
      gunHand.local[2],
    ]);
    await game.step({ frames: 2 });
    await game.input.set("left_hand.squeeze", 0);
    await game.step({ frames: 8 });

    assert.equal(
      await condition(game),
      20 + RESTORED_POINTS,
      "the released tool should restore ten condition points per Maintain level",
    );
    assert.equal(
      (await game.entities.byTemplate(MAINTENANCE_TOOL)).length,
      0,
      "the tool is used up by the work it does",
    );
  },
);

test(
  "using the maintenance tool from the flat inventory restores the wielded gun",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run", timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons" });
    await game.step({ frames: 30 });

    await game.input.trigger("DebugCycleWeapon");
    await game.step({ frames: 10 });
    const gun = (await game.info()).player.wielded_entity_id;
    assert.ok(gun, "flat mode must wield the pistol it spawned");
    await game.entities.sendMessage(gun, {
      type: "SetGunCondition",
      condition: 20,
    });
    await game.step({ frames: 1 });

    const tool = await toolInWorld(game);
    await game.player.give(tool.id);
    await game.step({ frames: 5 });

    // KEY: the strip's use gesture (double-click) on the tool.
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    const toolEl = (await game.ui.state()).strip?.elements.find(
      (e) => e.kind === "button" && e.entity_id === tool.id,
    );
    assert.ok(
      toolEl,
      `the strip should list the carried tool (got ${JSON.stringify(
        (await game.ui.state()).strip?.elements,
      )})`,
    );
    await clickUiElement(game, toolEl); // lift...
    await clickUiElement(game, toolEl); // ...and use
    await game.step({ frames: 5 });

    assert.equal(
      await condition(game),
      20 + RESTORED_POINTS,
      "using the tool should restore the wielded gun",
    );
    assert.equal(
      (await game.player.inventory()).items.filter(
        (i) => i.entity_id === tool.id,
      ).length,
      0,
      "and use the tool up",
    );
    const sounds = await game.audio.recent();
    assert.ok(
      sounds.sounds.some((s) => s.sample === "maintool"),
      `the use should play the tool's activation schema (got ${JSON.stringify(
        sounds.sounds.map((s) => s.sample),
      )})`,
    );
  },
);

test(
  "the maintenance tool refuses a gun already in good condition, and survives",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run", timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons" });
    await game.step({ frames: 30 });

    await game.input.trigger("DebugCycleWeapon");
    await game.step({ frames: 10 });
    const gun = (await game.info()).player.wielded_entity_id;
    assert.ok(gun, "flat mode must wield the pistol it spawned");
    assert.equal(await condition(game), 100, "a shipped gun starts pristine");

    const tool = await toolInWorld(game);
    await game.player.give(tool.id);
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    const toolEl = (await game.ui.state()).strip?.elements.find(
      (e) => e.kind === "button" && e.entity_id === tool.id,
    );
    assert.ok(toolEl, "the strip should list the carried tool");
    await clickUiElement(game, toolEl);
    await clickUiElement(game, toolEl);
    await game.step({ frames: 5 });

    assert.equal(
      await condition(game),
      100,
      "a gun in good condition is left alone",
    );
    assert.equal(
      (await game.player.inventory()).items.filter(
        (i) => i.entity_id === tool.id,
      ).length,
      1,
      "and the refused tool is not spent",
    );
    const messages = await game.ui.state();
    assert.ok(
      (messages.messages ?? []).some((m) =>
        m.includes("already in good condition"),
      ),
      `the refusal should say why (got ${JSON.stringify(messages.messages)})`,
    );
  },
);

// --- Slice 5: repairing a broken gun on the HRM board ---
//
// A broken gun cannot be fired and cannot be maintained; it is repaired on the
// same node-connecting board a crate or a keypad is hacked on, played against
// the Repair skill and the gun's own `P$RepairDif` terms. Using a broken gun
// from the inventory opens that board instead of wielding it - in flat and in
// the VR cyber interface, which present one canvas. Winning returns the gun to
// working order plus ten condition points.
//
// Negative-first: on the parent `P$RepairDif` is not parsed and no repair mode
// exists, so using a broken pistol just wields it and no panel ever opens.

/** A big nanite pile: the board charges the pistol's authored cost per deal. */
const BIG_NANITE_PILE = -1591;
/** The pistol's authored `P$RepairDif` nanite cost. */
const REPAIR_COST = "3";
/** A second bench gun, for the "one panel, many guns" case. */
const SHOTGUN = -19;
/** Points a won repair gives back on top of restoring working order. */
const REPAIR_BONUS = 10;

function hasTexture(panel: UiPanel, texture: string): boolean {
  return panel.elements.some(
    (element) => element.texture?.toLowerCase() === texture,
  );
}

function boardButton(panel: UiPanel, label: string): UiElement {
  const found = panel.elements.find(
    (element) => element.kind === "button" && element.label === label,
  );
  assert.ok(
    found,
    `the board should expose ${label} (got ${JSON.stringify(
      panel.elements.map((e) => e.label ?? e.texture),
    )})`,
  );
  return found;
}

async function repairPanel(game: GameServer): Promise<UiPanel> {
  const panel = (await game.ui.state()).active_panel;
  assert.ok(panel, "using a broken gun should keep the repair board open");
  return panel;
}

function conditionOf(detail: {
  properties: { name: string; value: string }[];
}): number {
  const condition = detail.properties.find((p) => p.name === "Condition");
  assert.ok(condition, "a gun should expose a Condition property");
  return Number(condition.value);
}

/**
 * Take one bench gun into the backpack and break it. It is worn to 20 first,
 * so the ten points a win gives back are visible against a value that is not
 * already full.
 */
async function brokenGunInBackpack(
  game: GameServer,
  template: number,
): Promise<{ id: number }> {
  const [gun] = await game.entities.byTemplate(template);
  assert.ok(gun, `debug_weapons should bench template ${template}`);
  await game.player.give(gun.id);
  await game.step({ frames: 5 });

  await game.entities.sendMessage(gun.id, {
    type: "SetGunCondition",
    condition: 20,
  });
  await breakGun(game, gun.id);
  await game.step({ frames: 5 });
  return gun;
}

/** A broken pistol plus the wallet the board's per-deal cost comes out of. */
async function brokenPistolInBackpack(
  game: GameServer,
): Promise<{ id: number }> {
  const pistol = await brokenGunInBackpack(game, PISTOL);
  await game.player.spawnItem(BIG_NANITE_PILE);
  await game.step({ frames: 5 });
  return pistol;
}

/** The strip's use gesture on one carried item: lift it, then use it. */
async function useFromStrip(
  game: GameServer,
  entityId: number,
  click: (element: UiElement) => Promise<void>,
): Promise<void> {
  const slot = (await game.ui.state()).strip?.elements.find(
    (e) => e.kind === "button" && e.entity_id === entityId,
  );
  assert.ok(
    slot,
    `the strip should list the carried item (got ${JSON.stringify(
      (await game.ui.state()).strip?.elements.map((e) => [
        e.kind,
        e.entity_id,
        e.label,
      ]),
    )})`,
  );
  await click(slot);
  await click(slot);
  await game.step({ frames: 5 });
}

/**
 * Genuinely play the board until the repair lands. debug_weapons maxes Repair
 * and Cyber Affinity, so the deal carries no mines; a board that burns itself
 * out is re-dealt, exactly as retail does, at the authored cost again.
 */
async function playRepairBoardToWin(
  game: GameServer,
  click: (element: UiElement) => Promise<void>,
): Promise<void> {
  const routes = [
    ["node-2-0", "node-3-0", "node-4-0"],
    ["node-2-1", "node-2-2", "node-2-3"],
    ["node-0-1", "node-0-2", "node-0-3"],
    ["node-4-0", "node-4-1", "node-4-2"],
    ["node-0-3", "node-1-3", "node-2-3"],
  ];
  for (let attempt = 0; attempt < 15; attempt += 1) {
    let panel = await repairPanel(game);
    if (hasTexture(panel, "winr.pcx")) return;
    assert.ok(
      !hasTexture(panel, "loser.pcx"),
      "a critical failure destroyed the gun; max Repair should leave no mines",
    );
    assert.ok(
      !hasTexture(panel, "payr.pcx"),
      "the test wallet should always cover the authored repair cost",
    );

    const inPlay = panel.elements.some((el) => el.label === "reset-hack");
    if (!inPlay || hasTexture(panel, "failr.pcx")) {
      const deal = panel.elements.find(
        (el) => el.label === "start-hack" || el.label === "reset-hack",
      );
      assert.ok(deal, "an unwon board should offer START/RESET");
      await click(deal);
    }

    for (const label of routes[attempt % routes.length]) {
      panel = await repairPanel(game);
      if (hasTexture(panel, "winr.pcx")) return;
      if (hasTexture(panel, "failr.pcx") || hasTexture(panel, "loser.pcx")) {
        break;
      }
      await click(boardButton(panel, label));
    }
  }
  assert.fail("the repair board should win within the attempt budget");
}

test(
  "using a broken gun from the flat inventory opens the repair board, and winning it repairs the gun",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run", timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons" });
    await game.step({ frames: 30 });

    const pistol = await brokenPistolInBackpack(game);

    // KEY: the strip's use gesture on a BROKEN gun opens the board rather than
    // wielding it.
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    const click = (element: UiElement) => clickUiElement(game, element);
    await useFromStrip(game, pistol.id, click);

    const board = await repairPanel(game);
    assert.ok(
      hasTexture(board, "iface/repair.pcx"),
      `a broken gun should present the repair board (got ${JSON.stringify(
        board.elements.map((e) => e.texture ?? e.label),
      )})`,
    );
    assert.equal(
      board.elements
        .filter((e) => e.kind === "text")
        .map((e) => e.text ?? "")
        .find((text) => /^\d+$/.test(text)),
      REPAIR_COST,
      "the board should show the pistol's authored RepairDiff cost",
    );
    assert.equal(
      (await game.info()).player.wielded_entity_id ?? null,
      null,
      "a broken gun opens the board instead of being wielded",
    );
    await game.screenshot("repair-board.png");

    // KEY: three connected nodes put the gun back into working order, ten
    // condition points better off than it broke.
    await playRepairBoardToWin(game, click);

    const detail = await game.entities.detail(pistol.id);
    assert.equal(
      objectStateOf(detail),
      "Normal",
      "a won repair should return the gun to working order",
    );
    assert.equal(
      conditionOf(detail),
      20 + REPAIR_BONUS,
      "a won repair should give condition points back",
    );

    // KEY: one synthetic panel host serves every gun, so a finished board must
    // not be left standing on it. A second broken gun in the same session gets
    // its own fresh board and repairs just as well.
    const shotgun = await brokenGunInBackpack(game, SHOTGUN);
    await useFromStrip(game, shotgun.id, click);
    assert.ok(
      hasTexture(await repairPanel(game), "iface/repair.pcx"),
      "a second broken gun should get a board of its own",
    );
    await playRepairBoardToWin(game, click);
    assert.equal(
      objectStateOf(await game.entities.detail(shotgun.id)),
      "Normal",
      "the second gun should repair as well as the first",
    );
  },
);

test(
  "the VR cyber interface presents the same repair board",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run", timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    const pistol = await brokenPistolInBackpack(game);

    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    const ui = await game.ui.state();
    assert.equal(ui.mode, "use", "the cyber interface should be up");
    const panelPose = ui.panel_pose;
    assert.ok(panelPose, "the VR cyber interface must report its panel pose");

    /** Release, then pull: a clean rising edge on the controller trigger. */
    const clickAt = async (canvas: [number, number]) => {
      for (const trigger of [0, 1, 0]) {
        await aimVrHandAtCanvas(game, panelPose, canvas, { trigger });
        await game.step({ frames: 2 });
      }
    };
    const clickElement = (element: UiElement) =>
      clickAt([
        element.rect[0] + element.rect[2] / 2,
        element.rect[1] + element.rect[3] / 2,
      ]);

    // KEY: the same use gesture on the same canvas, driven by a controller ray.
    await useFromStrip(game, pistol.id, clickElement);

    const board = await repairPanel(game);
    assert.ok(
      hasTexture(board, "iface/repair.pcx"),
      `the cyber interface should present the repair board (got ${JSON.stringify(
        board.elements.map((e) => e.texture ?? e.label),
      )})`,
    );
    await game.screenshot("repair-board-vr.png");

    await playRepairBoardToWin(game, clickElement);
    assert.equal(
      objectStateOf(await game.entities.detail(pistol.id)),
      "Normal",
      "a won repair works the same way in VR",
    );
  },
);

// ---------------------------------------------------------------------------
// Slice 6: modify.
//
// A working gun can be improved twice on the same board, entered from the
// weapon settings panel's MODIFY control - flat and in the VR cyber interface,
// which present one canvas. Each win raises the gun's modification level, and
// the level is what the gun's firing description is derived from, so the
// magazine and the reload change with it. There is no third modification.
//
// Negative-first: on the parent no modify mode exists, the settings panel has
// no MODIFY control, and the pistol's clip never leaves 12.

/** The pistol's authored `P$ModifyDif` nanite cost, per deal. */
const MODIFY_COST = "20";
/** Piles of 50: the board charges 20 a deal and may be re-dealt. */
const NANITE_PILES = 10;
/** The pistol's authored magazine, and what each modification makes of it. */
const PISTOL_CLIP = [12, 24, 24];
/** The pistol's authored reload, and what each modification makes of it. */
const PISTOL_RELOAD_MS = [500, 500, 167];

function gunNumber(
  detail: { properties: { name: string; value: string }[] },
  name: string,
): number {
  const found = detail.properties.find((p) => p.name === name);
  assert.ok(found, `a gun should expose a ${name} property`);
  return Number(found.value);
}

async function modifyPanel(game: GameServer): Promise<UiPanel> {
  const ui = await game.ui.state();
  assert.ok(
    ui.active_panel,
    `the modify board should be open (mode=${ui.mode})`,
  );
  return ui.active_panel;
}

/** Play the board until the modification lands, exactly as repair does. */
async function playModifyBoardToWin(
  game: GameServer,
  click: (element: UiElement) => Promise<void>,
): Promise<void> {
  const routes = [
    ["node-2-0", "node-3-0", "node-4-0"],
    ["node-2-1", "node-2-2", "node-2-3"],
    ["node-0-1", "node-0-2", "node-0-3"],
    ["node-4-0", "node-4-1", "node-4-2"],
    ["node-0-3", "node-1-3", "node-2-3"],
  ];
  for (let attempt = 0; attempt < 15; attempt += 1) {
    let panel = await modifyPanel(game);
    if (hasTexture(panel, "winm.pcx")) return;
    assert.ok(
      !hasTexture(panel, "losem.pcx"),
      "a critical failure broke the gun; max Modify should leave no mines",
    );
    assert.ok(
      !hasTexture(panel, "paym.pcx"),
      "the test wallet should always cover the authored modify cost",
    );

    const inPlay = panel.elements.some((el) => el.label === "reset-hack");
    if (!inPlay || hasTexture(panel, "failm.pcx")) {
      const deal = panel.elements.find(
        (el) => el.label === "start-hack" || el.label === "reset-hack",
      );
      assert.ok(
        deal,
        `an unwon board should offer START/RESET (got ${JSON.stringify(
          panel.elements.map((e) => e.label ?? e.texture),
        )})`,
      );
      await click(deal);
    }

    for (const label of routes[attempt % routes.length]) {
      panel = await modifyPanel(game);
      if (hasTexture(panel, "winm.pcx")) return;
      if (hasTexture(panel, "failm.pcx") || hasTexture(panel, "losem.pcx")) {
        break;
      }
      await click(boardButton(panel, label));
    }
  }
  assert.fail("the modify board should win within the attempt budget");
}

/** The wielded pistol, plus a wallet the board's per-deal cost comes out of. */
async function wieldedPistolWithNanites(
  game: GameServer,
): Promise<{ id: number }> {
  await game.input.trigger("DebugCycleWeapon");
  await game.step({ frames: 5 });
  const wielded = (await game.info()).player.wielded_entity_id;
  assert.ok(wielded, "DebugCycleWeapon should wield the bench pistol");
  for (let i = 0; i < NANITE_PILES; i += 1) {
    await game.player.spawnItem(BIG_NANITE_PILE);
  }
  await game.step({ frames: 5 });
  return { id: wielded };
}

/** Open the settings MFD the way a player does: the readout's SETTING button. */
async function openSettings(
  game: GameServer,
  click: (element: UiElement) => Promise<void>,
): Promise<void> {
  const setting = ((await game.ui.state()).readout ?? []).find(
    (e) => e.label === "gun_setting",
  );
  assert.ok(setting, "the readout should carry a SETTING button");
  await click(setting);
  await game.step({ frames: 3 });
}

/** SETTING, then MODIFY: the whole way in to the board. */
async function openModifyBoard(
  game: GameServer,
  click: (element: UiElement) => Promise<void>,
): Promise<void> {
  await openSettings(game, click);
  const panel = (await game.ui.state()).active_panel;
  assert.ok(panel, "SETTING should dock the settings panel");
  const modify = panel.elements.find(
    (e) => e.kind === "button" && e.label === "modify",
  );
  assert.ok(
    modify,
    `the settings panel should offer MODIFY (got ${JSON.stringify(
      panel.elements.map((e) => e.label ?? e.texture),
    )})`,
  );
  await click(modify);
  await game.step({ frames: 3 });
}

/** Dismiss whatever is docked, so the next SETTING press opens afresh. */
async function closePanel(
  game: GameServer,
  click: (element: UiElement) => Promise<void>,
): Promise<void> {
  const panel = (await game.ui.state()).active_panel;
  if (!panel) return;
  const close = panel.elements.find((e) => e.label === "close");
  assert.ok(close, "a docked panel should carry a close button");
  await click(close);
  await game.step({ frames: 3 });
}

test(
  "the settings panel's MODIFY control opens the board, and winning it modifies the gun",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run", timeout: 900_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons" });
    await game.step({ frames: 30 });

    const pistol = await wieldedPistolWithNanites(game);
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    const click = (element: UiElement) => clickUiElement(game, element);

    const unmodified = await game.entities.detail(pistol.id);
    assert.equal(gunNumber(unmodified, "Modification"), 0);
    assert.equal(gunNumber(unmodified, "ClipSize"), PISTOL_CLIP[0]);
    assert.equal(gunNumber(unmodified, "ReloadTimeMs"), PISTOL_RELOAD_MS[0]);

    // KEY: the settings panel is the way in, and the board it opens is the HRM
    // board played on the pistol's authored ModifyDiff terms.
    await openModifyBoard(game, click);
    const board = await modifyPanel(game);
    assert.ok(
      hasTexture(board, "modify.pcx"),
      `MODIFY should present the modify board (got ${JSON.stringify(
        board.elements.map((e) => e.texture ?? e.label),
      )})`,
    );
    assert.equal(
      board.elements
        .filter((e) => e.kind === "text")
        .map((e) => e.text ?? "")
        .find((text) => /^\d+$/.test(text)),
      MODIFY_COST,
      "the board should show the pistol's authored ModifyDiff cost",
    );
    // KEY: the board says what this modification will do, which is the only
    // place the gun's `P$Modify1` text is ever shown.
    assert.match(
      board.elements
        .filter((e) => e.kind === "text")
        .map((e) => e.text ?? "")
        .join(" "),
      /clip size/i,
      "the board should show the pistol's first modification text",
    );
    await game.screenshot("modify-board.png");

    // KEY: a win is one modification, and the gun's magazine follows it.
    await playModifyBoardToWin(game, click);
    const once = await game.entities.detail(pistol.id);
    assert.equal(gunNumber(once, "Modification"), 1);
    assert.equal(
      gunNumber(once, "ClipSize"),
      PISTOL_CLIP[1],
      "the first modification doubles the pistol's clip",
    );

    // KEY: a second modification is played on the harder `P$Modify2Di` terms
    // and changes something else again.
    await closePanel(game, click);
    await openModifyBoard(game, click);
    await playModifyBoardToWin(game, click);
    const twice = await game.entities.detail(pistol.id);
    assert.equal(gunNumber(twice, "Modification"), 2);
    assert.equal(
      gunNumber(twice, "ClipSize"),
      PISTOL_CLIP[2],
      "the clip stays doubled at the second modification",
    );
    assert.equal(
      gunNumber(twice, "ReloadTimeMs"),
      PISTOL_RELOAD_MS[2],
      "the second modification cuts the pistol's reload",
    );

    // KEY: there is no third. The control is still there - it is where the
    // player is told so - but it refuses rather than dealing a board.
    await closePanel(game, click);
    await openSettings(game, click);
    const settings = (await game.ui.state()).active_panel;
    assert.ok(settings);
    const modify = settings.elements.find((e) => e.label === "modify");
    assert.ok(modify, "a fully modified gun still carries the control");
    await click(modify);
    await game.step({ frames: 3 });
    assert.ok(
      !hasTexture(await modifyPanel(game), "modify.pcx"),
      "a third modification should be refused, not dealt",
    );
    assert.equal(
      gunNumber(await game.entities.detail(pistol.id), "Modification"),
      2,
      "and the gun is unchanged",
    );
  },
);

test(
  "the VR cyber interface presents the same modify board",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run", timeout: 900_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    // The VR hand takes the pistol off the bench, the way a player does.
    const bench = await cycleToWeapon(game, (e) => e.template_id === PISTOL);
    await aimVrHandAt(game, bench.position as Vec3, 0.3);
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 8 });
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      bench.id,
      "the VR right hand must hold the pistol",
    );
    for (let i = 0; i < NANITE_PILES; i += 1) {
      await game.player.spawnItem(BIG_NANITE_PILE);
    }
    await game.step({ frames: 5 });

    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    assert.equal(
      (await game.ui.state()).mode,
      "use",
      "the cyber interface should be up",
    );

    /**
     * Release, then pull: a clean rising edge on the controller trigger. The
     * panel pose is re-read per click, because the canvas it reports is the
     * canvas of whatever is currently docked - and the settings panel and the
     * board are not the same size.
     */
    const clickElement = async (element: UiElement) => {
      const pose = (await game.ui.state()).panel_pose;
      assert.ok(pose, "the VR cyber interface must report its panel pose");
      const canvas: [number, number] = [
        element.rect[0] + element.rect[2] / 2,
        element.rect[1] + element.rect[3] / 2,
      ];
      for (const trigger of [0, 1, 0]) {
        // Squeeze held throughout: releasing it would drop the pistol, and a
        // gun that is no longer in hand takes its settings panel with it.
        await aimVrHandAtCanvas(game, pose, canvas, { trigger, squeeze: 1 });
        await game.step({ frames: 2 });
      }
    };

    // In VR the forearm readout draws no buttons, so the settings panel is
    // asked for by the action instead - the same panel flat's SETTING button
    // docks, presented on the cyber interface's canvas.
    await game.input.trigger("OpenWeaponSettings");
    await game.step({ frames: 5 });
    const settings = (await game.ui.state()).active_panel;
    assert.ok(settings, "the action should dock the gun's settings panel");
    const modify = settings.elements.find(
      (e) => e.kind === "button" && e.label === "modify",
    );
    assert.ok(
      modify,
      `the settings panel should offer MODIFY (got ${JSON.stringify(
        settings.elements.map((e) => e.label ?? e.texture),
      )})`,
    );
    await clickElement(modify);
    await game.step({ frames: 3 });
    assert.ok(
      hasTexture(await modifyPanel(game), "modify.pcx"),
      "the cyber interface should present the modify board",
    );
    await game.screenshot("modify-board-vr.png");

    await playModifyBoardToWin(game, clickElement);
    assert.equal(
      gunNumber(await game.entities.detail(bench.id), "Modification"),
      1,
      "a won modification works the same way in VR",
    );
  },
);
