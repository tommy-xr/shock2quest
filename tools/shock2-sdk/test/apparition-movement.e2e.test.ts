import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import type { EntityDetailResult, Vec3 } from "../src/index.js";

const visible = (entity: EntityDetailResult) =>
  entity.properties.find(p => p.name === "HasRefs")?.value === "true";
const distance = (a: Vec3, b: Vec3) => Math.hypot(a[0] - b[0], a[2] - b[2]);

// Authored watch triggers, actors and destination markers from the missions.
for (const fixture of [
  { mission: "medsci1.mis", name: "Grassi Rep", actor: 1730, goal: 1731, trigger: [31.25, -0.88, -34.58] },
  { mission: "hydro2.mis", name: "Miller", actor: 1811, goal: 1573, trigger: [69.36, -1.6, -33.54] },
  { mission: "ops3.mis", name: "Yount", actor: 1057, goal: 989, trigger: [9.01, -10.78, 129.75] },
]) {
  test(`${fixture.name} apparition walks to its authored marker and vanishes`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: fixture.mission });
    await game.step({ frames: 5 });
    const [actor] = await game.entities.byTemplate(fixture.actor);
    const [goal] = await game.entities.byTemplate(fixture.goal);
    assert.ok(actor && goal);
    const initial = await game.entities.detail(actor.id);
    assert.equal(visible(initial), false);
    const spawn = (await game.info()).player.position;
    const [x, y, z] = fixture.trigger;
    await game.player.teleport({ x, y, z });
    await game.step({ frames: 2 });
    // The watch is the real trigger. Move the player back to safety afterward.
    await game.player.teleport({ x: spawn[0], y: spawn[1], z: spawn[2] });
    let appeared = false, vanished = false, closest = Infinity, travelled = 0;
    for (let second = 0; second < 29; second++) {
      await game.step({ frames: 60 });
      const current = await game.entities.detail(actor.id);
      appeared ||= visible(current);
      closest = Math.min(closest, distance(current.position, goal.position));
      travelled = Math.max(travelled, distance(current.position, initial.position));
      if (appeared && !visible(current)) { vanished = true; break; }
    }
    assert.ok(appeared, "the authored watch materializes the ghost");
    assert.ok(travelled > 0.5, `${fixture.name} must move rather than animate in place: ${travelled}`);
    assert.ok(closest < 1.3, `${fixture.name} must reach its marker: closest ${closest}`);
    assert.ok(vanished, "the remaining performance reaches ApparEnd before the 30-second watchdog");
    assert.equal((await game.physics.bodies({ entityId: actor.id })).bodies.length, 0,
      "walking apparitions remain intangible");
    const ended = (await game.entities.detail(actor.id)).position;
    await game.step({ frames: 120 });
    assert.ok(distance((await game.entities.detail(actor.id)).position, ended) < 0.001,
      "a vanished apparition stays in place");
  });
}
