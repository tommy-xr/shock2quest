import assert from "node:assert/strict";
import { test } from "node:test";

import { e2ePort } from "./helpers/e2e-port.js";
import { GameServer } from "../src/index.js";
import type { UiElement, UiPanelPose, UiState } from "../src/index.js";
import { canvasCenter, clickCanvasWithRay, requirePanelPose } from "./helpers/ui.js";
import { aimVrHandAtCanvas } from "./helpers/vr-hand.js";
import { ammoOf, fireOnce } from "./helpers/weapon.js";

// A VR-held gun has no viewmodel to tilt through a reload, so the cyber
// interface's ammo readout shows a spinner in the count well for the reload's
// firing lockout. RELOAD and the ammo-cycle arrow both reload from reserve
// behind that lockout; the cycle arrow used to leave a VR gun empty for a clip
// inserted by hand.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const PISTOL = -17;
const STD_CLIP = -31;
const AP_CLIP = -307;
/** Spinner segments the readout draws (`ammo_panel::SPINNER_SEGMENTS`). */
const SEGMENTS = 8;

type Rect = UiElement["rect"];

function inside([x, y, w, h]: Rect, [wx, wy, ww, wh]: Rect): boolean {
  return x >= wx && y >= wy && x + w <= wx + ww && y + h <= wy + wh;
}

/** What the count well draws: its spinner segments and its text. */
function well(ui: UiState, rect: Rect): { segments: number; text: string[] } {
  const drawn = ui.readout_elements.filter((e) => inside(e.rect, rect));
  return {
    segments: drawn.filter((e) => e.kind === "fill").length,
    text: drawn.filter((e) => e.kind === "text").map((e) => e.text ?? ""),
  };
}

function control(ui: UiState, label: string): UiElement {
  const found = ui.readout.find((e) => e.label === label);
  assert.ok(found, `expected a "${label}" control, got ${ui.readout.map((e) => e.label)}`);
  return found;
}

async function ammo(game: GameServer, id: number): Promise<number> {
  return ammoOf(await game.entities.detail(id));
}

/** Step until the reload ends, checking every frame that the spinner is drawn
 * exactly while the gun is locked. Returns the frames it ran. */
async function spinUntilReady(game: GameServer, rect: Rect): Promise<number> {
  for (let frame = 1; frame <= 600; frame += 1) {
    await game.step({ frames: 1 });
    const reloading = (await game.info()).player.reloading;
    const { segments } = well(await game.ui.state(), rect);
    assert.equal(
      segments,
      reloading ? SEGMENTS : 0,
      `frame ${frame}: the spinner must show exactly while firing is locked`,
    );
    if (!reloading) return frame;
  }
  throw new Error("the reload never finished");
}

test(
  "VR readout: RELOAD and the cycle arrow spin the count well for the reload lockout",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: e2ePort(0, "SHOCK2_E2E_VR_RELOAD_SPINNER_PORT"),
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });
    // A fresh medsci1 character lacks the pistol's Standard Weapons requirement.
    await game.player.setStats({ skills: { standard_weapons: 1 } });

    await game.input.set("right_hand.squeeze", 1);
    const pistol = (await game.player.spawnItem(PISTOL, { hand: "right" })).entity_id;
    await game.step({ frames: 8 });
    assert.equal((await game.info()).player.right_hand_entity_id, pistol);
    await game.player.spawnItem(STD_CLIP);
    if ((await ammo(game, pistol)) === 0) {
      await game.input.trigger("Reload");
      await game.step({ frames: 300 });
    }
    const full = await ammo(game, pistol);
    assert.ok(full > 0, "the pistol is loaded");
    await fireOnce(game);
    await game.step({ frames: 5 });
    assert.equal(await ammo(game, pistol), full - 1, "a round is fired");

    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 8 });
    const pose: UiPanelPose = requirePanelPose(await game.ui.state());
    // Park the gun hand off the panel so the free left hand's ray clicks.
    await aimVrHandAtCanvas(game, pose, [320, 240], { hand: "right", squeeze: 1, facing: "away" });
    await game.step({ frames: 3 });
    let ui = await game.ui.state();
    const countText = ui.readout_elements.find(
      (e) => e.kind === "text" && e.text === String(full - 1),
    );
    assert.ok(countText, "the readout shows the pistol's count");
    const countWell = countText.rect;
    assert.equal(well(ui, countWell).segments, 0, "no spinner before a reload");

    // RELOAD: the rounds move on the press, the spinner holds the well.
    await clickCanvasWithRay(game, pose, canvasCenter(control(ui, "reload")), "left");
    assert.equal((await game.info()).player.reloading, true, "RELOAD starts the lockout");
    assert.equal(await ammo(game, pistol), full);
    ui = await game.ui.state();
    assert.deepEqual(well(ui, countWell), { segments: SEGMENTS, text: [] });
    assert.ok(
      ui.readout.some((e) => e.label === "select_right_hand" && e.text === "RIGHT --"),
      "the hand tab claims no rounds mid-reload",
    );
    const reloadFrames = await spinUntilReady(game, countWell);
    assert.ok(reloadFrames > 30, `the pistol's reload takes time (${reloadFrames} frames)`);
    assert.deepEqual(well(await game.ui.state(), countWell).text, [String(full)]);

    // The cycle arrow: eject to reserve, select AP, reload AP from reserve.
    await game.player.spawnItem(AP_CLIP);
    await game.player.spawnItem(AP_CLIP);
    ui = await game.ui.state();
    assert.equal((await game.info()).player.wielded_ammo_type, "std");
    await clickCanvasWithRay(game, pose, canvasCenter(control(ui, "cycle_ammo")), "left");
    const info = (await game.info()).player;
    assert.equal(info.wielded_ammo_type, "ap", "the arrow selects the next type");
    assert.equal(info.reloading, true, "the arrow reloads the new type from reserve");
    const loaded = await ammo(game, pistol);
    assert.ok(loaded > 0, "AP rounds are in the gun");
    assert.equal(well(await game.ui.state(), countWell).segments, SEGMENTS);
    await spinUntilReady(game, countWell);
    assert.deepEqual(well(await game.ui.state(), countWell).text, [String(loaded)]);

    // The belt pouch, which serves the other hand's gun, now offers AP.
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    const pouch = (await game.info()).player.hand_feedback?.ammo_pouch;
    assert.equal(pouch?.offers[0]?.template, AP_CLIP, "the pouch offers the selected type");
  },
);
