import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { UiElement } from "../src/types.js";
import { teleportVerified } from "./helpers/teleport.js";

// End-to-end test for the flat-mode keypad MFD (projects/flat-ui.md PR 2,
// resolves #435): frobbing a keypad in flat presentation opens its MFD panel
// (drawn at the original left-MFD anchor on the 640x480 canvas), the panel's
// digit buttons are clickable via the pointer channels, entering the code
// sends TurnOn over the keypad's SwitchLink, and the linked door opens.
//
// Entity discovery is by TEMPLATE ID + LINK TOPOLOGY at run time (runtime
// entity ids are NOT stable across launches; the `template_id` reported by
// /v1/entities IS stable - for level-authored entities it is the mission-file
// object id): the cryo-exit keypad is the medsci1 "Keypad" with template_id
// 1681 (archetype -258) whose SwitchLink targets "Sci Med Door" (mission id
// 1739, archetype -206). The code (45100) is authored in the mission data
// (PropKeypadCode) - the test never enters it through any side channel, it
// clicks the on-screen digits.
//
// Negative-first: on main (pre flat-UI host) the Frob reaches KeyPadGui's
// GuiScript but falls through to NoEffect - /v1/ui reports no active panel,
// so the "panel opened" assertion fails.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "flat keypad MFD: frob opens panel, clicking 45100 opens the door",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
    });
    await game.step({ frames: 5 });

    const uiState = () => game.ui.state();

    // --- Discover the cryo-exit keypad + its door by name/template/links ---
    const keypads = (await game.entities.list({ filter: "Keypad", limit: 50 }))
      .entities;
    assert.ok(keypads.length > 0, "medsci1 should contain Keypad entities");

    let keypadId: number | undefined;
    let doorId: number | undefined;
    for (const candidate of keypads) {
      if (candidate.template_id !== 1681) continue; // stable mission id
      const detail = await game.entities.detail(candidate.id);
      const doorLink = detail.outgoing_links.find(
        (l) =>
          l.link_type.toLowerCase().includes("switch") &&
          l.target_name.includes("Sci Med Door"),
      );
      if (doorLink) {
        keypadId = candidate.id;
        doorId = doorLink.target_id;
        break;
      }
    }
    assert.ok(
      keypadId !== undefined && doorId !== undefined,
      "should find the keypad (mission id 1681) SwitchLinked to 'Sci Med Door'",
    );

    const keypad = await game.entities.detail(keypadId);
    const doorBefore = await game.entities.detail(doorId);

    // --- Get the player next to the keypad (test setup; teleport is fine) ---
    const [kx, ky, kz] = keypad.position;
    await teleportVerified(game, { x: kx + 1.0, y: ky + 0.5, z: kz + 1.0 });
    await game.step({ frames: 10 });

    // Sanity: no panel is open before the frob.
    const before = await uiState();
    assert.ok(
      !before.active_panel,
      "no panel should be active before frobbing the keypad",
    );
    await game.screenshot("keypad-before-frob.png");

    // --- Frob the keypad: the panel must open (THE #435 fix) ---
    await game.entities.sendMessage(keypadId, { type: "Frob" });
    await game.step({ frames: 5 });

    const opened = await uiState();
    assert.ok(
      opened.active_panel,
      `frobbing the keypad should open its MFD panel (got ${JSON.stringify(opened)})`,
    );
    assert.equal(
      opened.active_panel.entity_id,
      keypadId,
      "the active panel should be bound to the keypad entity",
    );
    assert.equal(
      opened.active_panel.template_id,
      1681,
      "the active panel should report the keypad's stable template id",
    );

    // The panel must expose semantically-labeled digit buttons so clients can
    // click digits without hardcoded pixels.
    const digitButton = (d: string) => {
      const el = opened.active_panel!.elements.find(
        (e) => e.kind === "button" && e.label === d,
      );
      assert.ok(el, `panel should expose a button labeled "${d}"`);
      return el;
    };
    for (const d of ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "clear"]) {
      digitButton(d);
    }
    // Redundant visual check: the panel visibly appears.
    await game.screenshot("keypad-panel-open.png");

    // --- Click 4-5-1-0-0 via the pointer channels ---
    const audioBefore = await game.audio.recent();
    const lastAudioSequence = audioBefore.sounds.at(-1)?.sequence ?? 0;
    // Rects come from /v1/ui in normalized screen space (letterbox-corrected),
    // so the click point is just the rect center.
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

    for (const d of ["4", "5", "1", "0", "0"]) {
      // Re-read the panel each click (hover swaps button textures; rects are
      // stable but re-reading keeps the loop honest about live state).
      const state = await uiState();
      assert.ok(state.active_panel, `panel should stay open while typing ${d}`);
      const el = state.active_panel.elements.find(
        (e) => e.kind === "button" && e.label === d,
      );
      assert.ok(el, `panel should expose digit "${d}"`);
      await clickElement(el);
    }
    await game.screenshot("keypad-code-entered.png");

    // Each digit emits the real `keypad` schema, whose P$SchPlayPa authors
    // -1500 millibels and fixed -1000 pan. Both must reach the non-spatial
    // sink rather than playing at full, centered volume.
    const recentAudio = (await game.audio.recent()).sounds.filter(
      (sound) => sound.sequence > lastAudioSequence,
    );
    const keypadSound = recentAudio.find(
      (sound) =>
        sound.sample === "bkeypad" &&
        sound.volume_millibels === -1500 &&
        sound.pan_millibels === -1000,
    );
    assert.ok(
      keypadSound,
      `entering a digit should resolve keypad's authored volume and fixed pan; got ${JSON.stringify(recentAudio)}`,
    );
    assert.ok(Math.abs(keypadSound.gain - 0.17782794) < 0.000001);
    assert.equal(keypadSound.pan_applied, true);

    // --- The SwitchLinked door must open (TranslatingDoor slides ~2.4 units) ---
    await game.step({ frames: 120 }); // ~2s for the door to travel
    const doorAfter = await game.entities.detail(doorId);
    const moved = Math.hypot(
      doorAfter.position[0] - doorBefore.position[0],
      doorAfter.position[1] - doorBefore.position[1],
      doorAfter.position[2] - doorBefore.position[2],
    );
    assert.ok(
      moved > 1.0,
      `entering 45100 should open the Sci Med Door (door moved ${moved.toFixed(2)} units; ` +
        `before=${doorBefore.position}, after=${doorAfter.position})`,
    );
    await game.screenshot("keypad-door-open.png");

    // --- Close behaviors ---
    // LMB on the bare 3D view (outside the panel) closes it.
    const stillOpen = await uiState();
    assert.ok(stillOpen.active_panel, "panel should still be open after success");
    await game.input.set("pointer.position", [0.9, 0.9]);
    await game.step({ frames: 2 });
    await game.input.set("pointer.pressed", 1);
    await game.step({ frames: 2 });
    await game.input.set("pointer.pressed", 0);
    await game.step({ frames: 2 });
    const afterBareClick = await uiState();
    assert.ok(
      !afterBareClick.active_panel,
      "LMB on the bare 3D view should close the panel",
    );

    // Walk-away auto-close: reopen, then teleport far from the keypad.
    await game.entities.sendMessage(keypadId, { type: "Frob" });
    await game.step({ frames: 5 });
    assert.ok((await uiState()).active_panel, "panel should reopen on frob");
    await game.player.teleport({ x: kx + 20, y: ky + 0.5, z: kz + 20 });
    await game.step({ frames: 5 });
    const afterWalkAway = await uiState();
    assert.ok(
      !afterWalkAway.active_panel,
      "walking away from the keypad should auto-close its panel",
    );
  },
);
