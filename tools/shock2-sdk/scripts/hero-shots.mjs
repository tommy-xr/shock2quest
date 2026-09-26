// Reproducible README/website hero stills. Built SDK; run from tools/shock2-sdk:
//   npm run build && node scripts/hero-shots.mjs [--only hydro1] [--out <dir>]
// Each shot boots its mission fresh in VR presentation (no screen-space HUD),
// steps a fixed frame count so wandering AI lands in roughly the same place,
// then frames a free camera. Positions are world coordinates found by scouting.
import { mkdir } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { GameServer } from "../dist/src/index.js";

const SHOTS = [
  {
    name: "hydro1",
    mission: "hydro1.mis",
    camera: { position: [38.3, 1.6, -16.4], lookAt: [42.0, 1.1, -15.7] },
  },
  {
    name: "medsci1",
    mission: "medsci1.mis",
    camera: { position: [6.8, 1.55, -35.7], lookAt: [11.5, 1.35, -35.8] },
  },
  {
    name: "ops2",
    mission: "ops2.mis",
    camera: { position: [65.5, -6.5, 137.5], lookAt: [65.5, -7.0, 141.5] },
  },
  {
    name: "rec1",
    mission: "rec1.mis",
    camera: { position: [-10.6, -2.4, -101.0], lookAt: [-7.0, -3.0, -101.2] },
  },
];

const { values } = parseArgs({
  options: {
    out: { type: "string" },
    only: { type: "string" },
    "max-width": { type: "string", default: "1280" },
    frames: { type: "string", default: "120" },
  },
});
const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const out = values.out ? resolve(values.out) : resolve(repoRoot, "screenshots/hero");
await mkdir(out, { recursive: true });

const shots = values.only ? SHOTS.filter((s) => s.name === values.only) : SHOTS;
if (shots.length === 0) throw new Error(`no shot named '${values.only}'`);

for (const shot of shots) {
  const game = await GameServer.launch({
    mission: shot.mission,
    debugFlags: ["--vr", "--window-size", "1920x1080"],
    repoRoot,
  });
  try {
    // Park the VR hands out of frame; the free camera would otherwise catch them.
    await game.input.set("left_hand.position", [0, -100, 0]);
    await game.input.set("right_hand.position", [0, -100, 0]);
    await game.step({ frames: Number(values.frames) });
    // Cull from the camera, not the (distant) player.
    await game.devParams.set("free_camera_cull", 1);
    await game.camera.set(shot.camera);
    await game.step({ frames: 2 });
    const path = resolve(out, `${shot.name}.png`);
    const result = await game.screenshot(path, Number(values["max-width"]));
    console.log(`${shot.name}: ${result.full_path} ${result.resolution.join("x")}`);
  } finally {
    await game.shutdown();
  }
}
