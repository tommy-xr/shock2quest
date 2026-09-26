// Reproducible README/website hero stills. Built SDK; run from tools/shock2-sdk:
//   npm run build && node scripts/hero-shots.mjs [--only hydro1] [--out <dir>]
// Each shot boots its mission fresh in VR presentation (no screen-space HUD),
// steps a fixed frame count so wandering AI lands in roughly the same place,
// then stands the player at `player` with a loadout in hand, aimed at the
// nearest entity matching `target` - a first-person VR view.
import { mkdir } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { aimHandsAt, attachSupportHand, faceTarget, GameServer } from "../dist/src/index.js";

// Hand offsets from the eye: x right, y up, -z toward the target.
const AIM_RIGHT = [0.1, -0.18, -0.55];
const AIM_LEFT = [-0.1, -0.2, -0.52];
const REST_LEFT = [-0.18, -0.35, -0.4];

const SHOTS = [
  {
    // Two-handed long gun.
    name: "hydro1",
    mission: "hydro1.mis",
    player: [38.3, 0.9, -16.4],
    target: { filter: "OG-Shotgun", height: 0.9 },
    loadout: { right: "Shotgun" },
    hands: { right: [0.15, -0.28, -0.45] },
    support: "right",
  },
  {
    // Weapon + melee.
    name: "medsci1",
    mission: "medsci1.mis",
    player: [6.8, 0.7, -35.7],
    target: { filter: "OG-Pipe", height: 0.9 },
    loadout: { right: "Pistol", left: "Wrench" },
    hands: { right: AIM_RIGHT, left: AIM_LEFT },
  },
  {
    // Psi amp + weapon.
    name: "ops2",
    mission: "ops2.mis",
    player: [65.5, -7.3, 137.5],
    target: { filter: "Protocol Droid", height: 1.0 },
    loadout: { right: "Laser Pistol", left: -247 },
    hands: { right: AIM_RIGHT, left: AIM_LEFT },
  },
  {
    // Single weapon.
    name: "rec1",
    mission: "rec1.mis",
    player: [-10.6, -2.9, -101.0],
    target: { filter: "Red Monkey", height: 0.5 },
    loadout: { right: "Pistol" },
    hands: { right: AIM_RIGHT, left: REST_LEFT },
  },
];

const { values } = parseArgs({
  options: {
    out: { type: "string" },
    only: { type: "string" },
    "max-width": { type: "string", default: "1280" },
    frames: { type: "string", default: "0" },
    fov: { type: "string", default: "85" },
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
    await game.step({ frames: Number(values.frames) });
    const { entities } = await game.entities.list({ filter: shot.target.filter, limit: 50 });
    const dist = (e) => Math.hypot(...e.position.map((v, i) => v - shot.player[i]));
    const subject = entities.sort((a, b) => dist(a) - dist(b))[0];
    if (!subject) throw new Error(`${shot.name}: no '${shot.target.filter}' in ${shot.mission}`);
    const [sx, sy, sz] = subject.position;
    const target = [sx, sy + shot.target.height, sz];

    // Square up at the spawn point, out of the subject's sight, along the
    // shot's heading (settling takes seconds - long enough to draw an attack).
    const spawn = (await game.info()).player.position;
    await faceTarget(game, target.map((v, i) => spawn[i] + v - shot.player[i]));
    const [x, y, z] = shot.player;
    await game.player.teleport({ x, y, z });
    await game.step({ frames: 5 });
    await aimHandsAt(game, target, shot.hands);
    await game.step({ frames: 2 });
    // Hold the grips: a VR hand releases whatever it holds when it lets go.
    for (const [hand, template] of Object.entries(shot.loadout)) {
      await game.input.set(`${hand}_hand.squeeze`, 1);
      await game.player.spawnItem(template, { hand });
    }
    await game.step({ frames: 3 });
    const { player } = await game.info();
    const held = { left: player.wielded_entity_id, right: player.right_hand_entity_id };
    for (const hand of Object.keys(shot.loadout)) {
      if (held[hand] == null) throw new Error(`${shot.name}: ${hand} hand holds nothing`);
    }
    if (shot.support) await attachSupportHand(game, shot.support);

    await game.devParams.set("fov_override_deg", Number(values.fov));
    await game.step({ frames: 2 });
    const path = resolve(out, `${shot.name}.png`);
    const result = await game.screenshot(path, Number(values["max-width"]));
    console.log(`${shot.name}: ${result.full_path} ${result.resolution.join("x")}`);
  } finally {
    await game.shutdown();
  }
}
