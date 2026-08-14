import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";
const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8223);

// MedSci1 object 102 is a Security Camera with an unobstructed view from the
// authored floor position used below. Runtime entity ids vary between runs.
const CAMERA_TEMPLATE_ID = 102;

function prop(detail: EntityDetailResult, name: string): string | undefined {
  return detail.properties.find((property) => property.name === name)?.value;
}

test(
  "security cameras honor the authored immediate-to-moderate awareness delay",
  { skip: !e2eEnabled, timeout: 900_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: basePort,
    });

    const [camera] = await game.entities.byTemplate(CAMERA_TEMPLATE_ID);
    assert.ok(camera, `medsci1 should contain camera ${CAMERA_TEMPLATE_ID}`);
    assert.equal(prop(await game.entities.detail(camera.id), "Model"), "camgrn");

    // Enter the camera's real scan cone. Template -367 authors to_two=-1,
    // which the original engine treats as an already-expired reaction timer,
    // and to_three=4000ms.
    const [cx, cy, cz] = camera.position;
    await game.player.teleport({ x: cx - 2.83, y: cy - 1.9, z: cz + 2.83 });

    await game.step({ frames: 1 });
    let detail = await game.entities.detail(camera.id);
    assert.equal(prop(detail, "AIAlertness"), "Moderate");
    assert.equal(prop(detail, "Model"), "camyel");

    // Three seconds later the authored four-second high-alert delay is still
    // running; after the fourth second it expires and the camera turns red.
    await game.step({ frames: 180 });
    detail = await game.entities.detail(camera.id);
    assert.equal(prop(detail, "AIAlertness"), "Moderate");
    assert.equal(prop(detail, "Model"), "camyel");

    await game.step({ frames: 61 });
    detail = await game.entities.detail(camera.id);
    assert.equal(prop(detail, "AIAlertness"), "High");
    assert.equal(prop(detail, "Model"), "camred");
  },
);
