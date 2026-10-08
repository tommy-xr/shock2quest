import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

const enabled = process.env.SHOCK2_E2E === "1";
for (const vr of [false, true]) {
  test(`Engineering auxiliary survives a missing main bed and follows pause/range/scene changes (${vr ? "VR" : "flat"})`, {
    skip: !enabled, timeout: 300_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "eng1.mis", port: 0, debugFlags: vr ? ["--vr"] : [] });
    const [region] = await game.entities.byTemplate(138);
    assert.ok(region);
    const near = { x: region.position[0]!, y: region.position[1]!, z: region.position[2]! };
    await game.player.teleport(near);
    await game.step({ frames: 2 });
    const before = (await game.audio.loops()).loops;
    const aux = before.find(loop => /amb_hor1/i.test(loop.sample));
    const bed = before.find(loop => loop.owner === "environmental");
    assert.ok(aux, "authored eng_hor1 auxiliary must play");
    // This retail marker names eng_pump1, which is absent from the gamesys
    // (eng_pump exists). A missing main bed must not suppress eng_hor1.
    assert.equal(bed, undefined, "the authored missing main schema stays silent");
    await game.step({ frames: 30 });
    assert.ok((await game.audio.loops()).loops.some(loop => loop.handle === aux.handle));
    await game.input.trigger("TogglePauseMenu");
    await game.step({ frames: 2 });
    assert.equal((await game.audio.loops()).loops.find(loop => loop.handle === aux.handle)?.paused, true);
    await game.input.trigger("TogglePauseMenu");
    await game.step({ frames: 2 });
    assert.equal((await game.audio.loops()).loops.find(loop => loop.handle === aux.handle)?.paused, false);
    await game.player.teleport({ x: 1000, y: 1000, z: 1000 });
    await game.step({ frames: 2 });
    assert.ok(!(await game.audio.loops()).loops.some(loop => loop.handle === aux.handle));
    await game.player.teleport(near);
    await game.step({ frames: 2 });
    const restarted = (await game.audio.loops()).loops.find(loop => /amb_hor1/i.test(loop.sample));
    assert.ok(restarted);
    assert.notEqual(restarted.handle, aux.handle);
    await game.transitionLevel("medsci1.mis");
    await game.step({ frames: 2 });
    assert.ok(!(await game.audio.loops()).loops.some(loop => loop.handle === restarted.handle));
  });
}

test("Med/Sci stress layer uses authored intervals instead of looping a sample every frame", {
  skip: !enabled, timeout: 300_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "medsci1.mis", port: 0 });
  const [region] = await game.entities.byTemplate(1299);
  assert.ok(region);
  await game.player.teleport({ x: region.position[0]!, y: region.position[1]!, z: region.position[2]! });
  await game.step({ frames: 2 });
  const plays = async () => (await game.audio.recent()).sounds.filter(sound =>
    sound.tags.some(([key, value]) => key === "environmental_aux" && value === "ms_stress"));
  const first = await plays();
  assert.equal(first.length, 1);
  assert.match(first[0]!.sample, /^stres_m[1-7]$/i);
  assert.equal(first[0]!.volume_millibels, -1200);
  assert.ok((await game.audio.loops()).loops.some(loop => loop.owner === "environmental"));
  assert.ok(!(await game.audio.loops()).loops.some(loop => /stres_m/i.test(loop.sample)), "interval clips must not repeat seamlessly");
  await game.step({ frames: 300 });
  assert.equal((await plays()).length, 1, "no repeat before six seconds");
  await game.step({ frames: 330 });
  const repeated = await plays();
  assert.equal(repeated.length, 2);
  const gap = repeated[1]!.sim_time - repeated[0]!.sim_time;
  assert.ok(gap >= 6 && gap <= 10.05, `authored 6–10 second cadence, got ${gap}`);
  await game.player.teleport({ x: 1000, y: 1000, z: 1000 });
  await game.step({ frames: 2 });
  // A clip that already ended naturally has no explicit stop timestamp.
  assert.ok((await plays()).every(sound => !sound.still_playing), "no auxiliary voice survives region exit");
  await game.step({ frames: 660 });
  assert.equal((await plays()).length, 2, "leaving cancels the interval scheduler");
});
