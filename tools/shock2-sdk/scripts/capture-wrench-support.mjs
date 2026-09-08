// Node 22+, built SDK. Run from tools/shock2-sdk:
// node scripts/capture-wrench-support.mjs --output /tmp/astra-wrench-support/after
// --reference <prior data.json> replays exactly the saved camera and secondary inputs.
// Use a baseline checkout/runtime to produce matching before evidence. Runtime lifecycle
// remains SDK-owned. No grip resources are changed. Gallery statuses are diagnostic.
import assert from "node:assert/strict";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { GameServer } from "../dist/src/index.js";
import { aimVrHandAt } from "../dist/test/helpers/vr-hand.js";
import { parseArgs } from "node:util";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
const { values } = parseArgs({
  options: {
    output: { type: "string", default: "/tmp/astra-wrench-support" },
    reference: { type: "string" },
  },
});
const out = resolve(values.output),
  reference = values.reference
    ? JSON.parse(await readFile(resolve(values.reference), "utf8"))
    : null;
const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
await mkdir(out, { recursive: true });
const game = await GameServer.launch({
  mission: "debug_interactions",
  debugFlags: ["--vr"],
  repoRoot,
});
const data = { hands: {} };
try {
  for (const primary of ["right", "left"]) {
    const secondary = primary === "right" ? "left" : "right";
    await game.input.set("left_hand.squeeze", 0);
    await game.input.set("right_hand.squeeze", 0);
    await game.camera.attach();
    await game.input.trigger("DebugReloadLevel");
    await game.step({ frames: 90 });
    const item = (await game.entities.list({ limit: 100 })).entities.find(
      (e) => e.template_id === -928,
    );
    assert.ok(item);
    await game.player.teleport({ x: item.position[0], y: 1, z: 0 });
    await aimVrHandAt(game, item.position, 0.2, 0, 0, { hand: primary });
    await game.input.set(`${primary}_hand.squeeze`, 1);
    await game.step({ frames: 3 });
    assert.equal(
      (await game.info()).player[
        primary === "left" ? "wielded_entity_id" : "right_hand_entity_id"
      ],
      item.id,
    );
    await game.player.teleport({ x: 4, y: 1.25, z: 5 });
    await game.input.set(`${primary}_hand.position`, [0, 1, 0]);
    await game.input.set(`${primary}_hand.rotation`, [
      0,
      Math.SQRT1_2,
      0,
      Math.SQRT1_2,
    ]);
    await game.input.set(`${secondary}_hand.position`, [0, -100, 0]);
    await game.step({ frames: 3 });
    const info = await game.info(),
      g = info.player.hand_grips.find((g) => g.hand === primary),
      s = g.support,
      pawn = info.player.position;
    const vec = (v) => (Array.isArray(v) ? v : [v.x, v.y, v.z]);
    const pos =
      reference?.hands[primary].basePosition ??
      vec(s.controller_position).map((v, i) => v - pawn[i]);
    const rot = reference?.hands[primary].baseRotation ?? [
      s.controller_rotation.v.x,
      s.controller_rotation.v.y,
      s.controller_rotation.v.z,
      s.controller_rotation.s,
    ];
    const camera = reference?.hands[primary].camera ?? {
      position: [4.8, pawn[1] + 1.3, 5.7],
      lookAt: [3.96, pawn[1] + 1.16, 5],
    };
    await game.camera.set(camera);
    const record = {
      basePosition: pos,
      baseRotation: rot,
      camera,
      initialDiagnostic: g,
      initialPlayerPosition: pawn,
      steps: [],
    };
    data.hands[primary] = record;
    const steps = reference?.hands[primary].steps ?? [
      {
        name: "approach",
        position: pos.map((v, i) => v + (i === 0 ? 0.16 : 0)),
        squeeze: 0,
        frames: 6,
      },
      { name: "at-socket", position: pos, squeeze: 0, frames: 6 },
      ...Array.from({ length: 5 }, (_, i) => ({
        name: "attach-" + i,
        position: pos,
        squeeze: 1,
        frames: 3,
      })),
      ...Array.from({ length: 9 }, (_, i) => ({
        name: "steer-" + i,
        position: pos.map(
          (v, j) =>
            v +
            (j === 0
              ? 0.16 * Math.sin((i * Math.PI) / 4)
              : j === 2
                ? 0.07 * Math.sin((i * Math.PI) / 4)
                : 0),
        ),
        squeeze: 1,
        frames: 3,
      })),
      ...Array.from({ length: 6 }, (_, i) => ({
        name: "release-" + i,
        position: pos.map((v, j) => v + (j === 0 ? 0.16 : 0)),
        squeeze: 0,
        frames: 3,
      })),
    ];
    for (const step of steps) {
      await game.input.set(`${secondary}_hand.position`, step.position);
      await game.input.set(`${secondary}_hand.rotation`, rot);
      await game.input.set(`${secondary}_hand.squeeze`, step.squeeze);
      await game.step({ frames: step.frames });
      const state = await game.info();
      const file = `${primary}-${step.name}.png`;
      await game.screenshot(`${out}/${file}`, 1600);
      if (step.name === "attach-4") {
        const support = state.player.hand_grips.find(
          (g) => g.hand === primary,
        )?.support;
        const priorViews = reference?.hands[primary].closeViews;
        record.closeViews = {};
        for (const [view, delta] of Object.entries({
          front: [0.35, 0.08, 0.35],
          back: [-0.35, 0.08, -0.35],
          top: [0, 0.45, 0.01],
        })) {
          if (!support && !priorViews?.[view]) continue;
          const target = support
            ? vec(support.socket_position)
            : priorViews[view].lookAt;
          const pose = priorViews?.[view] ?? {
            position: target.map((v, i) => v + delta[i]),
            lookAt: target,
          };
          await game.camera.set(pose);
          await game.step({ frames: 1 });
          const closeFile = `${primary}-contact-${view}.png`;
          await game.screenshot(`${out}/${closeFile}`, 1600);
          record.closeViews[view] = { ...pose, file: closeFile };
        }
        await game.camera.set(camera);
      }
      record.steps.push({
        ...step,
        file,
        diagnostic: state.player.hand_grips,
        playerPosition: state.player.position,
      });
    }
    await writeFile(`${out}/data.json`, JSON.stringify(data, null, 2));
  }
} finally {
  await game.shutdown();
  await writeFile(`${out}/runtime.log`, game.logs().join("\n"));
}

const sections = [];
for (const [hand, record] of Object.entries(data.hands)) {
  for (const [view, shot] of Object.entries(record.closeViews ?? {})) {
    const image = await readFile(`${out}/${shot.file}`);
    sections.push(
      `<section><h2>${hand} primary / contact ${view}</h2><img alt="${hand} primary contact ${view}" src="data:image/png;base64,${image.toString("base64")}"></section>`,
    );
  }

  for (const step of record.steps) {
    const image = await readFile(`${out}/${step.file}`);
    const support = step.diagnostic.find((g) => g.hand === hand)?.support;
    const status = support
      ? `attached=${support.attached}, blend=${Number(support.blend).toFixed(3)}`
      : "baseline: no support diagnostics";
    sections.push(
      `<section><h2>${hand} primary / ${step.name}</h2><p>${status}</p><img alt="${hand} primary ${step.name}" src="data:image/png;base64,${image.toString("base64")}"></section>`,
    );
  }
}
await writeFile(
  `${out}/index.html`,
  `<!doctype html><meta charset="utf-8"><title>Astra wrench support</title><style>body{background:#18202a;color:white;font:16px system-ui;margin:24px}img{max-width:100%;max-height:80vh}section{margin-bottom:32px}</style><h1>Wrench support diagnostic capture</h1><p>Primary controller fixed; secondary approaches, acquires, steers and releases. Rendered images and state transitions are evidence, not automatic fit or headset approval.</p>${sections.join("")}`,
);
console.log(`${out}/index.html`);
