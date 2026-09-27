import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { winHack } from "./helpers/hack.js";
import { clickUiElement } from "./helpers/ui.js";
import { clickWorldPanelElement } from "./helpers/vr-hand.js";
import { carriedNaniteTotal } from "./helpers/nanites.js";

const enabled = process.env.SHOCK2_E2E === "1";

test("a paid MedSci turret hack survives save/load", { skip: !enabled, timeout: 300_000 }, async () => {
  await using game = await GameServer.launch({ mission: "medsci1.mis", port: 0 });
  await game.player.setStats({ endurance: 6, cyber_affinity: 6, skills: { hack: 6 } });
  await game.player.spawnItem(-1591);
  const [turret] = await game.entities.byTemplate(610);
  assert.ok(turret, "authored MedSci turret 610 must be present");
  const [x, y, z] = turret.position;
  await game.player.teleport({ x: x - 1.5, y, z });
  await game.input.set("left_hand.thumbstick", [0, 0.5]);
  await game.entities.sendMessage(turret.id, { type: "Frob" });
  await game.step({ frames: 5 });
  assert.equal((await game.ui.state()).active_panel?.entity_id, turret.id);
  await winHack(game);
  const wallet = await carriedNaniteTotal(game);
  const saveName = `medsci_turret_hacked_${Date.now()}`;
  assert.equal((await game.save(saveName)).success, true);
  assert.equal((await game.load(saveName)).success, true);
  await game.step({ frames: 5 });
  const [restored] = await game.entities.byTemplate(610);
  assert.ok(restored);
  await game.entities.sendMessage(restored.id, { type: "Frob" });
  await game.step({ frames: 5 });
  assert.notEqual((await game.ui.state()).active_panel?.entity_id, restored.id,
    "the restored friendly turret must not offer another hack");
  assert.equal(await carriedNaniteTotal(game), wallet);
});

for (const vr of [false, true]) {
  test(`paid turret hacking redirects actual shots to a hostile (${vr ? "VR" : "flat"})`,
    { skip: !enabled, timeout: 300_000 }, async () => {
      await using game = await GameServer.launch({ mission: "debug_turret", port: 0, debugFlags: vr ? ["--vr"] : [] });
      await game.player.setStats({ endurance: 6, cyber_affinity: 6, skills: { hack: 6 } });
      await game.player.spawnItem(-1591);
      const [turret] = await game.entities.byTemplate(-168);
      assert.ok(turret);
      const [x, y, z] = turret.position;
      await game.player.teleport({ x: x - 1.5, y, z: z + 2 });
      await game.step({ frames: 60 });
      await game.entities.sendMessage(turret.id, { type: "Frob" });
      await game.step({ frames: 5 });
      assert.equal((await game.ui.state()).active_panel?.entity_id, turret.id,
        "frobbing a hostile turret must open the shared paid board");
      const click: typeof clickUiElement = vr
        ? (g, element) => clickWorldPanelElement(g, [188, 296, 0], 1, element)
        : clickUiElement;
      const wallet = await carriedNaniteTotal(game);
      await winHack(game, click);
      assert.ok(await carriedNaniteTotal(game) < wallet, "turret hacking must charge the authored cost");
      const close = (await game.ui.state()).active_panel?.elements.find((element) => element.label === "close");
      if (close) await click(game, close);
      // World panels have no flat MFD close chrome; walking away dismisses
      // their completed result before testing a fresh frob.
      await game.player.teleport({ x: x - 20, y, z });
      await game.step({ frames: 5 });
      assert.equal((await game.ui.state()).active_panel, null);
      await game.player.teleport({ x: x - 1.5, y, z: z + 2 });
      await game.step({ frames: 30 });
      await game.entities.sendMessage(turret.id, { type: "Frob" });
      await game.step({ frames: 5 });
      assert.notEqual((await game.ui.state()).active_panel?.entity_id, turret.id,
        "a friendly turret must not sell another hack");

      // The debug turret's authored forward is world +X. Put a real spawned
      // hybrid between it and the player using the existing debug action.
      await game.player.teleport({ x: x + 12, y, z });
      const hp = (await game.info()).player.hit_points;
      await game.step({ frames: 180 });
      assert.equal((await game.info()).player.hit_points, hp, "a hacked turret must leave the player alone");
      const player = await game.player.position();
      await game.input.lookAtWorldPoint([x, player.y + 1.6, z]);
      await game.input.trigger("SpawnDebugMonster");
      await game.step({ frames: 2 });
      const [hostile] = await game.entities.byTemplate(-397);
      assert.ok(hostile, "the fixture must spawn an actual hostile creature");
      const health = async () => {
        const [remaining] = await game.entities.byTemplate(-397);
        if (!remaining) return 0;
        const detail = await game.entities.detail(remaining.id);
        return Number(detail.properties.find((property) => property.name === "HitPoints")?.value ?? 0);
      };
      const before = await health();
      assert.ok(before > 0);
      await game.step({ frames: 360 });
      assert.ok(await health() < before, "a hacked turret's actual projectiles must hit the hostile");
    });
}

test("a broken turret stops acquiring and firing at the player", { skip: !enabled, timeout: 300_000 }, async () => {
  await using game = await GameServer.launch({ mission: "debug_turret", port: 0 });
  await game.player.setStats({ endurance: 6 });
  const [turret] = await game.entities.byTemplate(-168);
  assert.ok(turret);
  const [x, y, z] = turret.position;
  await game.player.teleport({ x: x + 12, y, z });
  const before = (await game.info()).player.hit_points!;
  await game.step({ frames: 180 });
  assert.ok((await game.info()).player.hit_points! < before, "the untampered turret must first demonstrate live fire");
  await game.entities.sendMessage(turret.id, { type: "SetObjectState", state: "Broken" });
  await game.step({ frames: 60 }); // Let the already-launched shot finish.
  const stopped = (await game.info()).player.hit_points!;
  assert.ok(stopped > 0);
  await game.step({ frames: 180 });
  assert.equal((await game.info()).player.hit_points, stopped, "breaking the turret must stop further shots");
});
